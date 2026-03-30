use sqlx::PgPool;
use ulid::Ulid;

use crate::{
    config::Config,
    domain::{ManifestKey, RawUploadKey, TransmuxKey, UploadContentType},
};

#[derive(Debug)]
pub struct VideoRecord {
    pub ulid: Ulid,
    pub status: String,
    pub raw_key: RawUploadKey,
    pub transmux_key: Option<TransmuxKey>,
    pub manifest_key: Option<ManifestKey>,
    pub browser_compatible: bool,
    pub transmux_required: bool,
    pub transcode_required: bool,
}

/// Holds database resources and startup routines.
pub struct VideoRepository {
    pool: PgPool,
}

impl VideoRepository {
    /// Connect to Postgres and run any pending migrations.
    pub async fn new(config: &Config) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(&config.database_url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Insert a new video row in `pending_upload` state before direct-to-R2 upload starts.
    pub async fn create_pending_video(
        &self,
        ulid: Ulid,
        raw_key: &RawUploadKey,
        content_type: &UploadContentType,
        size_bytes: i64,
    ) -> Result<(), sqlx::Error> {
        let ulid = ulid.to_string();

        sqlx::query!(
            "INSERT INTO videos (ulid, status, raw_key, content_type, size_bytes) VALUES ($1, 'pending_upload', $2, $3, $4)",
            &ulid,
            &**raw_key,
            &**content_type,
            size_bytes,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Mark an existing video as `uploaded` once the client confirms upload completion.
    ///
    /// Returns `true` when a row was updated and `false` when no video matched the ULID.
    pub async fn mark_uploaded(&self, ulid: Ulid) -> Result<bool, sqlx::Error> {
        let ulid = ulid.to_string();
        let result = sqlx::query!("UPDATE videos SET status = 'uploaded' WHERE ulid = $1", &ulid)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Fetch a video by ULID for API responses.
    ///
    /// Returns `Ok(None)` when the ULID does not exist.
    pub async fn find_video_by_ulid(&self, ulid: Ulid) -> Result<Option<VideoRecord>, sqlx::Error> {
        let ulid = ulid.to_string();
        let row = sqlx::query!(
            "SELECT ulid, status, raw_key, transmux_key, manifest_key, browser_compatible, transmux_required, transcode_required FROM videos WHERE ulid = $1",
            &ulid,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let ulid = r
                .ulid
                .parse::<Ulid>()
                .expect("invalid ULID stored in videos.ulid");

            VideoRecord {
                ulid,
            status: r.status,
            raw_key: r.raw_key.into(),
            transmux_key: r.transmux_key.map(Into::into),
            manifest_key: r.manifest_key.map(Into::into),
            browser_compatible: r.browser_compatible,
            transmux_required: r.transmux_required,
            transcode_required: r.transcode_required,
            }
        }))
    }
}

#[cfg(test)]
impl VideoRepository {
    /// Build a repository from an existing pool for integration-style unit tests.
    pub fn with_pool(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(test)]
mod tests {
    use super::VideoRepository;
    use sqlx::PgPool;
    use std::str::FromStr;
    use ulid::Ulid;

    use crate::domain::{RawUploadKey, UploadContentType};

    fn ulid(value: &str) -> Ulid {
        value.parse::<Ulid>().expect("valid ulid literal")
    }

    fn content_type(value: &str) -> UploadContentType {
        UploadContentType::from_str(value).expect("valid content type literal")
    }

    fn raw_key(ulid: Ulid) -> RawUploadKey {
        RawUploadKey::from(ulid)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_pending_video_and_find_by_ulid(pool: PgPool) {
        let repository = VideoRepository::with_pool(pool);
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let raw_key = raw_key(ulid);

        repository
            .create_pending_video(ulid, &raw_key, &content_type("video/mp4"), 123)
            .await
            .unwrap();

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(found.ulid, ulid);
        assert_eq!(found.status, "pending_upload");
        assert_eq!(&*found.raw_key, &*raw_key);
        assert!(!found.browser_compatible);
        assert!(!found.transmux_required);
        assert!(found.transcode_required);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_video_by_ulid_returns_none_when_missing(pool: PgPool) {
        let repository = VideoRepository::with_pool(pool);

        let found = repository
            .find_video_by_ulid(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAA"))
            .await
            .unwrap();

        assert!(found.is_none());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_pending_video_duplicate_ulid_returns_error(pool: PgPool) {
        let repository = VideoRepository::with_pool(pool);
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAB");

        repository
            .create_pending_video(
                ulid,
                &raw_key(ulid),
                &content_type("video/mp4"),
                123,
            )
            .await
            .unwrap();

        let duplicate = repository
            .create_pending_video(
                ulid,
                &raw_key(ulid),
                &content_type("video/mp4"),
                456,
            )
            .await;

        assert!(duplicate.is_err());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_uploaded_updates_status(pool: PgPool) {
        let repository = VideoRepository::with_pool(pool);
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");

        repository
            .create_pending_video(
                ulid,
                &raw_key(ulid),
                &content_type("video/mp4"),
                321,
            )
            .await
            .unwrap();

        let updated = repository.mark_uploaded(ulid).await.unwrap();
        assert!(updated);

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(found.status, "uploaded");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_uploaded_returns_false_when_missing(pool: PgPool) {
        let repository = VideoRepository::with_pool(pool);
        let updated = repository
            .mark_uploaded(ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"))
            .await
            .unwrap();

        assert!(!updated);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_video_by_ulid_maps_optional_fields(pool: PgPool) {
        let repository = VideoRepository::with_pool(pool.clone());
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2");

        repository
            .create_pending_video(
                ulid,
                &raw_key(ulid),
                &content_type("video/mp4"),
                789,
            )
            .await
            .unwrap();

        sqlx::query(
            "UPDATE videos SET transmux_key = $1, manifest_key = $2, browser_compatible = $3, transmux_required = $4, transcode_required = $5 WHERE ulid = $6",
        )
        .bind("transmux/01ARZ3NDEKTSV4RRFFQ69G5FB2/output.mp4")
        .bind("hls/01ARZ3NDEKTSV4RRFFQ69G5FB2/manifest.m3u8")
        .bind(true)
        .bind(true)
        .bind(false)
        .bind(ulid.to_string())
        .execute(&pool)
        .await
        .unwrap();

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(
            found.transmux_key.as_ref().map(|k| &**k),
            Some("transmux/01ARZ3NDEKTSV4RRFFQ69G5FB2/output.mp4")
        );
        assert_eq!(
            found.manifest_key.as_ref().map(|k| &**k),
            Some("hls/01ARZ3NDEKTSV4RRFFQ69G5FB2/manifest.m3u8")
        );
        assert!(found.browser_compatible);
        assert!(found.transmux_required);
        assert!(!found.transcode_required);
    }
}
