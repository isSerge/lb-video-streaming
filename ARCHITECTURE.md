# Video streaming service — architecture design

## Goal

A minimal private video streaming service. Users upload video files (up to 1 GB) anonymously and receive a shareable link. Anyone with the link can stream the video in a browser.

Time-to-stream is prioritised over video quality.

---

## Core requirements

- Upload size limited to 1 GB; common formats supported (MP4, MKV, MOV, WebM, AVI)
- Anonymous uploads — no authentication required
- System generates a shareable link per video (`/watch/<ulid>`)
- Videos are streamable as soon as possible after upload

---

## Architecture overview

Single Rust binary at MVP. Two concurrent async tasks share an in-process channel. External dependencies: Postgres and Cloudflare R2.

```
browser  →  Axum HTTP task  →  Postgres
                ↓ mpsc
         Worker task  →  R2 (transmux / HLS segments)
                ↑
browser  →  Cloudflare CDN  →  R2
```

### Internal structure — ports and adapters

Core business logic is isolated from infrastructure behind traits: `Storage`, `VideoRepository`, `MediaProbe`, `MediaTranscoder`. Adapters (`R2Storage`, `PgVideoRepository`, `Ffprobe`, `FfmpegTranscoder`) implement these traits. Migrating from `mpsc` to NATS (Stage 2), or swapping `ffprobe` for a pure-Rust demuxer, requires no changes to orchestration logic.

---

## Streaming phases

Time-to-stream depends on the uploaded file's container and codec. A `ffprobe` probe (see Upload flow) determines the path immediately after upload.

| Container | Codec | Phase 1 | Notes |
|---|---|---|---|
| MP4, MOV | H.264 + AAC | 1a — serve raw | Works in all browsers |
| WebM | VP8 / VP9 | 1a — serve raw | Chrome/Firefox native |
| MKV, AVI, MOV | H.264 + AAC | 1b — transmux to MP4 | Seconds, no re-encode |
| MKV, AVI | VP9 | 1b — transmux to WebM | Seconds, no re-encode |
| Any | H.265 / HEVC | None — wait for HLS | Unsupported in Chrome/Firefox |
| Any | AV1, ProRes, etc. | None — wait for HLS | Full re-encode required |

**Phase 1a (instant):** raw file served via Cloudflare range requests. No processing.

**Phase 1b (seconds):** FFmpeg repackages into a browser-compatible container without re-encoding (`-c copy`). Fast regardless of file size.

**Phase 2 (minutes, always):** FFmpeg produces a single 720p H.264 HLS output. Once ready the player upgrades transparently. Single rendition only — adaptive bitrate multiplies storage 3–4× and is deferred.

### Known limitations

**MP4 moov atom:** if the `moov` atom is at the end of the file, the browser cannot begin Phase 1a playback until the full file is downloaded. Phase 2 HLS does not have this issue. Phase 1 is best-effort — the UI labels it "processing" not "watch now".

**Single PUT fragility:** a dropped connection on a 1 GB upload requires a full restart. Acceptable at MVP. Upgrade path: S3 presigned multipart uploads (deferred — significantly more complex).

---

## Component decisions

### Upload flow — presigned PUT to R2

Uploads go directly from the browser to R2. The API server never touches video bytes, avoiding memory pressure and Cloudflare's 100 MB proxied body limit on Free/Pro plans.

```
Browser                 API                      R2
   |-- POST /upload-url >|                        |
   |<-- presigned PUT ---|                        |
   |-- PUT video bytes -------------------------->|
   |-- POST /upload-complete/<ulid> >|            |
   |                    |-- ffprobe (range req) -->|
   |<-- { url } --------|                        |
```

`ffprobe` is run against a short-lived presigned GET URL — it uses HTTP range requests internally and reads only container headers (first/last few MB). The file is never downloaded to disk at this step.

### Queue — in-process `mpsc` with startup reconciliation

`tokio::sync::mpsc` is the correct primitive when producer and consumer are in the same process. Zero infra, zero latency.

On startup, before HTTP accepts requests:
1. **Wipe `$WORKER_TEMP_DIR`** — prevents disk exhaustion from partial files left by a prior crash
2. **Reset stuck jobs** — `UPDATE videos SET status = 'uploaded' WHERE status IN ('transmuxing', 'transcoding')`
3. **Re-enqueue** — push all `uploaded` rows into the channel

Partial R2 output is safely overwritten on re-run.

**Upgrade path:** replace `mpsc` sender/receiver with NATS JetStream. Job struct unchanged.

### Concurrency limit

A `tokio::sync::Semaphore` caps concurrent FFmpeg processes. Default: 1 on a 4 vCPU machine — FFmpeg uses all threads internally so parallel jobs contend without throughput gain. Configurable via `MAX_CONCURRENT_TRANSCODES`.

### Transmux file lifecycle

The transmuxed file is a temporary bridge to Phase 2. Once HLS is ready it has no value.

**Current implementation (MVP):** Explicit deletion from R2 immediately after HLS success.
- **Trade-off:** Minimal storage overhead, but risks a "404 Not Found" for a viewer who started watching the transmuxed file seconds before the HLS transcode completed.
- **Future Resolution:** Set `transmux_expires_at = now() + 1 hour` and let R2 lifecycle delete it. This eliminates the race condition for a negligible storage cost (~$0.01/month).

### Zombie job handling

A `tokio::time::interval` task runs every minute (configurable via `SWEEP_INTERVAL_SECS`):

```sql
UPDATE videos SET status = 'failed'
WHERE status IN ('transmuxing', 'transcoding')
AND updated_at < NOW() - INTERVAL '2 hours'
```

`updated_at` is bumped periodically by the worker during long FFmpeg runs to prevent false positives.

### Object storage — Cloudflare R2

R2 is S3-compatible. Zero egress fees when served through Cloudflare's network. Direct presigned URL access from the browser incurs egress at ~$0.015/GB — all video reads are therefore served through a Cloudflare-proxied domain (`videos.yourdomain.com`), not presigned URLs. In place from day one.

HLS segments are immutable — cache-forever headers are correct and segments are edge-cached on repeat views. Raw files over 512 MB may bypass Cloudflare's edge cache (Free/Pro/Biz limit) but Phase 1 is a temporary state so this is acceptable.

### Raw file retention

30-day TTL via R2 lifecycle rule. `raw_expires_at` is set at upload. Provides a recovery window for transcode bugs or future reprocessing without permanent storage of superseded data.

Upgrade path if long-term raw recovery is needed: move to S3 Glacier Instant Retrieval (~$0.004/GB vs R2's $0.015/GB) via server-side `CopyObject` after a successful transcode. ~4× cheaper, millisecond retrieval.

### Database — Postgres

Single instance, `sqlx` compile-time checked queries. All state in Postgres + R2 — API servers are stateless and horizontally scalable without coordination.

### Network Resiliency and Backpressure
Operating over distributed object storage (R2) requires treating the network as hostile. The system employs several DDIA-aligned fault tolerance patterns:
- **Circuit Breakers:** A state-machine circuit breaker (`failsafe`) protects the worker loop. If R2 or Postgres suffer an outage, the breaker trips, rejecting jobs instantly and re-queuing them with a delay, preventing cascading failures.
- **Exponential Backoff:** Streaming 1 GB files over HTTP is inherently fragile. The `FileTransfer` adapter utilizes exponential backoff with jitter to recover from transient TCP drops seamlessly.
- **Graceful Teardown:** `CancellationToken`s orchestrate bounded graceful shutdowns. If a deployment triggers a `SIGTERM`, Axum stops accepting uploads, and active FFmpeg processes are gracefully aborted without corrupting state.

---

## Data model

### Status state machine

```
[pending_upload] → [uploaded] → [transmuxing]? → [transcoding] → [ready]
                                                                     ↑
                      (startup reconciler resets crashed jobs back to uploaded)

Any state → [failed]  (zombie sweeper or fatal FFmpeg error)
```

### Schema

```sql
CREATE TABLE videos (
  id                 BIGSERIAL PRIMARY KEY,
  ulid               TEXT        NOT NULL UNIQUE,
  status             TEXT        NOT NULL DEFAULT 'pending_upload',
  raw_key            TEXT        NOT NULL,
  raw_expires_at     TIMESTAMPTZ,
  raw_archived_at    TIMESTAMPTZ,
  transmux_key       TEXT,
  transmux_expires_at TIMESTAMPTZ,
  manifest_key       TEXT,
  browser_compatible BOOLEAN     NOT NULL DEFAULT FALSE,
  transmux_required  BOOLEAN     NOT NULL DEFAULT FALSE,
  content_type       TEXT        NOT NULL,
  size_bytes         BIGINT      NOT NULL,
  updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

---

## Data flow

### Worker (async)

**Phase 1b — transmux (if `transmux_required`):**
1. `status = 'transmuxing'`; download raw to `$WORKER_TEMP_DIR`
2. `ffmpeg -i input -c copy output.mp4`
3. Upload to R2 `transmux/<ulid>/`; set `transmux_key`, `transmux_expires_at = now() + 1hr`

**Phase 2 — HLS transcode (always):**
1. `status = 'transcoding'`; use transmux output if available, else raw
2. `ffmpeg -i input -vf scale=-2:720 -b:v 1500k -codec:a aac -b:a 128k -f hls -hls_time 6 output.m3u8`; bump `updated_at` periodically
3. Upload segments + manifest to R2 `hls/<ulid>/`
4. `status = 'ready'`; set `manifest_key`; clean up local temp files

### Playback state machine

| Status | Flags | Player action |
|---|---|---|
| `ready` | — | `hls.js` on HLS manifest |
| `uploaded` | `browser_compatible` | native `<video>` on raw; poll for upgrade |
| `uploaded` | `transmux_required` | native `<video>` on raw (codec ok); poll for transmux |
| `uploaded` | neither | "processing" UI; poll until ready |
| `transmuxing` | — | native `<video>` on raw; poll |
| `transcoding` | `transmux_key` set | native `<video>` on transmux URL; poll for HLS |
| `failed` | — | error UI |

---

## Cost efficiency

### Cost drivers

- **Storage** — raw file + optional transmux (short TTL) + HLS segments (~800 MB per 1 GB upload at 720p)
- **Egress** — $0 via Cloudflare proxy. Without it: ~$0.09/GB on AWS S3
- **Compute** — bursty CPU for transcoding; right-size the machine, don't over-provision

### Raw retention

30-day R2 lifecycle TTL by default. Upgrade path: S3 Glacier Instant Retrieval for long-term archival at ~$0.004/GB.

### Rough estimate

100 uploads/day, ~750 MB average raw, ~500 MB average HLS output, 10 views/video, 30-day retention:

| Item | Calculation | Monthly |
|---|---|---|
| HLS storage | 100/day × 500 MB × 30 days = 1,464 GB × $0.015 | ~$22 |
| Raw storage | 100/day × 750 MB × 30 days = 2,197 GB × $0.015 | ~$33 |
| Egress | Cloudflare proxy | $0 |
| Compute | Hetzner CCX23 | ~$15 |
| **Total** | | **~$70/month** |

R2 free tier (10 GB storage, 10M read ops) reduces this to near-zero at very low volumes. Equivalent on AWS S3 + EC2: $300–500/month, primarily egress.

---

## Volume-based evolution

Each stage is a localised change, not a rewrite. Move when a bottleneck is measured, not anticipated.

### Stage 1 — single VPS (< 200 uploads/day)

```
browser → Cloudflare → R2
browser → [Axum + Worker] → Postgres / R2
```

Hetzner CCX23 (4 vCPU, 8 GB, ~€14/month). `MAX_CONCURRENT_TRANSCODES=1`.

**Signal to move:** CPU >70% sustained, jobs queuing, API latency degrading.

### Stage 2 — extracted worker (~200–1,000 uploads/day)

Replace `mpsc` with NATS JetStream. Workers on spot instances, scaled to queue depth. API servers stateless behind a load balancer.

```
browser  → Cloudflare → R2
browser  → [Axum ×N] → Postgres
[Axum]   → NATS → [Worker ×N] → R2 / Postgres
```

Spot economics: ~13 CPU-hours/day at 1,000 uploads → ~$0.65/day on spot vs ~$4/day always-on.

**Signal to move:** Postgres connection saturation, metadata read latency climbing.

### Stage 3 — full horizontal (1,000+ uploads/day)

Postgres read replicas + short-TTL Redis cache on hot metadata. Managed Postgres (Supabase, Neon).

```
browser  → Cloudflare (~90% cache hit) → R2
browser  → [Axum ×N] → Redis → Postgres primary/replicas
[Axum]   → NATS → [Worker ×N] → R2 / Postgres
```

### Summary

| Stage | Volume | Change | Est. cost |
|---|---|---|---|
| 1 | < 200/day | baseline | ~$70/month |
| 2 | 200–1,000/day | Extract worker; NATS; spot instances | ~$120–220/month |
| 3 | 1,000+/day | Postgres replicas; Redis cache | ~$350–650/month |

Egress $0 at all stages.

---

