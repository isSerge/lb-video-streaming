CREATE TABLE videos (
    id                  BIGSERIAL PRIMARY KEY,
    ulid                TEXT NOT NULL UNIQUE,
    status              TEXT NOT NULL DEFAULT 'pending_upload',
                        -- pending_upload | uploaded | transmuxing | transcoding | ready | failed
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

CREATE INDEX videos_status_idx ON videos (status);
