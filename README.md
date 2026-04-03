# Minimal Video Streaming Service

A private, horizontally scalable video streaming service built with Rust, Svelte, Postgres, and Cloudflare R2.

> **Architecture Note:** See the [`ARCHITECTURE.md`](./ARCHITECTURE.md) document for a detailed breakdown of the two-phase streaming design, cost-efficiency strategy (zero-egress), and scaling evolution.

## Prerequisites

- **Rust** (stable)
- **Node.js / npm** (for the Svelte frontend)
- **Docker** to run Postgres container
- **SQLX CLI** to run migrations
- **FFmpeg & FFprobe** installed and available in `$PATH`
- **Cloudflare R2** account

## Local Development Setup

1. **Database**

	Start Docker container
   ```bash
   docker run --name lab-postgres \
  	-e POSTGRES_PASSWORD=postgres \
  	-e POSTGRES_DB=video_streaming \
  	-p 5432:5432 \
  	-d postgres:16
   ```

	Run migrations using SQLX CLI
	 ```bash
	 cargo sqlx migrate run
	 ```

2. **Environment Variables**
   Copy `.env.example` to `.env` and fill in your R2 credentials and DB URL:
   ```env
   DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres
   R2_ACCOUNT_ID=...
   R2_ACCESS_KEY_ID=...
   R2_SECRET_ACCESS_KEY=...
   R2_BUCKET_NAME=...
   PUBLIC_CDN_DOMAIN=https://pub-your-bucket-id.r2.dev
   ```
   *Note: Ensure your R2 bucket has a CORS policy allowing `PUT` and `GET` from `http://localhost:5173`.*

3. **Run the Backend (API + Worker)**
   ```bash
   cargo run
   ```

4. **Run the Frontend**
   In a new terminal window:
   ```bash
   cd ui
   npm install
   VITE_API_BASE_URL=http://127.0.0.1:3000 npm run dev
   ```
   Navigate to `http://localhost:5173` to upload and stream videos.

## Generate Dummy Test Videos (FFmpeg)

Use FFmpeg's built-in `lavfi` (libavfilter) to generate 10-second test patterns with a synthesized audio beep. 

Run these commands in your terminal to generate one of each compatibility type:

**1. MP4 (H.264 / AAC) — Phase 1a (Instant Raw Fast-Path)**
```bash
ffmpeg -f lavfi -i testsrc=duration=10:size=1280x720:rate=30 -f lavfi -i sine=frequency=440:duration=10 -c:v libx264 -c:a aac test_fastpath.mp4
```

**2. WebM (VP9 / Opus) — Phase 1a (Instant Raw Fast-Path)**
```bash
ffmpeg -f lavfi -i testsrc=duration=10:size=1280x720:rate=30 -f lavfi -i sine=frequency=440:duration=10 -c:v libvpx-vp9 -c:a libopus test_fastpath.webm
```

**3. MKV (H.264 / AAC) — Phase 1b (Transmux Required)**
```bash
ffmpeg -f lavfi -i testsrc=duration=10:size=1280x720:rate=30 -f lavfi -i sine=frequency=440:duration=10 -c:v libx264 -c:a aac test_transmux.mkv
```

**4. MP4 (HEVC/H.265 / AAC) — Phase 2 (Transcode Required, no fast path)**
```bash
ffmpeg -f lavfi -i testsrc=duration=10:size=1280x720:rate=30 -f lavfi -i sine=frequency=440:duration=10 -c:v libx265 -c:a aac test_transcode.mp4
```
