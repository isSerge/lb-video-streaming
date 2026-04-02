CREATE TABLE videos (
    id                  BIGSERIAL PRIMARY KEY,
    ulid                TEXT NOT NULL UNIQUE,
    status              TEXT NOT NULL DEFAULT 'pending_upload',
    raw_key             TEXT NOT NULL,
    raw_expires_at      TIMESTAMPTZ,
    raw_archived_at     TIMESTAMPTZ,
    transmux_key        TEXT,
    manifest_key        TEXT,
    browser_compatible  BOOLEAN NOT NULL DEFAULT FALSE,
    transmux_required   BOOLEAN NOT NULL DEFAULT FALSE,
    transcode_required  BOOLEAN NOT NULL DEFAULT TRUE,
    content_type        TEXT NOT NULL,
    size_bytes          BIGINT NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 1. Index for the Worker queue puller (only active uploads)
CREATE INDEX idx_videos_queue ON videos (created_at) 
WHERE status = 'uploaded';

-- 2. Index for the Zombie Sweeper (only active transcodes)
CREATE INDEX idx_videos_zombie_sweep ON videos (updated_at) 
WHERE status IN ('transmuxing', 'transcoding');

-- 3. Index for the Stale Upload Sweeper (only pending uploads)
CREATE INDEX idx_videos_stale_uploads ON videos (created_at) 
WHERE status = 'pending_upload';

-- Note: We do NOT index 'ready' or 'failed' statuses. 
-- The `UNIQUE (ulid)` constraint automatically creates the B-Tree index needed for API lookups.
