pub mod port;

use std::{num::NonZeroU64, time::Duration};

pub use port::VideoRepository;

use sqlx::PgPool;
use ulid::Ulid;

use crate::{
    config::Config,
    domain::{
        FormatCompatibility, ManifestKey, RawUploadKey, TransmuxKey, UploadContentType, VideoStatus,
    },
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
pub struct PgVideoRepository {
    pool: PgPool,
}

impl PgVideoRepository {
    /// Connect to Postgres and run any pending migrations.
    pub async fn new(config: &Config) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(&config.database_url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl VideoRepository for PgVideoRepository {
    async fn create_pending_video(
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

    async fn mark_uploaded_with_compatibility(
        &self,
        ulid: Ulid,
        compatibility: FormatCompatibility,
    ) -> Result<bool, sqlx::Error> {
        let ulid = ulid.to_string();
        let result = sqlx::query!(
            "UPDATE videos SET status = 'uploaded', browser_compatible = $2, transmux_required = $3, transcode_required = $4 WHERE ulid = $1",
            &ulid,
            compatibility.browser_compatible(),
            compatibility.transmux_required(),
            compatibility.transcode_required(),
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn find_video_by_ulid(&self, ulid: Ulid) -> Result<Option<VideoRecord>, sqlx::Error> {
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

    async fn recover_pending_jobs(&self) -> Result<Vec<Ulid>, sqlx::Error> {
        // Reset any jobs that were in a pending state but never completed (e.g. due to a crash)
        sqlx::query!(
            "UPDATE videos SET status = 'uploaded' WHERE status IN ('transmuxing', 'transcoding')"
        )
        .execute(&self.pool)
        .await?;

        // Fetch all jobs that need processing
        let rows = sqlx::query!("SELECT ulid FROM videos WHERE status = 'uploaded'")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|r| r.ulid.parse().unwrap()).collect())
    }

    async fn mark_zombie_jobs_failed(&self, timeout: NonZeroU64) -> Result<u64, sqlx::Error> {
        // Safe cast: 2 hours is 7,200 seconds, well within i32 limits.
        let timeout_secs = timeout.get() as i32;

        let result = sqlx::query!(
            "UPDATE videos SET status = 'failed' 
             WHERE status IN ('transmuxing', 'transcoding') 
             AND updated_at < NOW() - ($1::int * INTERVAL '1 second')",
            timeout_secs
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    async fn delete_stale_pending_uploads(&self, older_than: Duration) -> Result<u64, sqlx::Error> {
        let older_than_secs = older_than.as_secs() as f64;
        let result = sqlx::query!(
            "DELETE FROM videos WHERE status = 'pending_upload' AND created_at < NOW() - ($1 * INTERVAL '1 second')",
            older_than_secs
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    async fn update_status(&self, ulid: Ulid, status: VideoStatus) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE videos SET status = $1, updated_at = NOW() WHERE ulid = $2",
            status.as_ref(),
            ulid.to_string()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_transmux_key(&self, ulid: Ulid, key: &TransmuxKey) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE videos SET transmux_key = $1, updated_at = NOW() WHERE ulid = $2",
            &**key,
            ulid.to_string()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_manifest_key(&self, ulid: Ulid, key: &ManifestKey) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE videos SET manifest_key = $1, updated_at = NOW() WHERE ulid = $2",
            &**key,
            ulid.to_string()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_updated_at(&self, ulid: Ulid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE videos SET updated_at = NOW() WHERE ulid = $1",
            ulid.to_string()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn clear_transmux_key(&self, ulid: Ulid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE videos SET transmux_key = NULL, updated_at = NOW() WHERE ulid = $1",
            ulid.to_string()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
impl PgVideoRepository {
    /// Build a repository from an existing pool for integration-style unit tests.
    pub fn with_pool(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use std::str::FromStr;
    use ulid::Ulid;

    use crate::domain::{ContainerFormat, FormatCompatibility, RawUploadKey, UploadContentType};

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
        let repository = PgVideoRepository::with_pool(pool);
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let raw_key = raw_key(ulid);

        repository
            .create_pending_video(ulid, &raw_key, &content_type("video/mp4"), 123)
            .await
            .unwrap();

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(found.ulid, ulid);
        assert_eq!(found.status, VideoStatus::PendingUpload.as_ref());
        assert_eq!(&*found.raw_key, &*raw_key);
        assert!(!found.browser_compatible);
        assert!(!found.transmux_required);
        assert!(found.transcode_required);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_video_by_ulid_returns_none_when_missing(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool);

        let found = repository
            .find_video_by_ulid(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAA"))
            .await
            .unwrap();

        assert!(found.is_none());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_pending_video_duplicate_ulid_returns_error(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool);
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAB");

        repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 123)
            .await
            .unwrap();

        let duplicate = repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 456)
            .await;

        assert!(duplicate.is_err());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_uploaded_with_compatibility_updates_status(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool);
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");

        repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 321)
            .await
            .unwrap();

        let updated = repository
            .mark_uploaded_with_compatibility(ulid, FormatCompatibility::TranscodeRequired)
            .await
            .unwrap();
        assert!(updated);

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(found.status, VideoStatus::Uploaded.as_ref());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_uploaded_with_compatibility_returns_false_when_missing(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool);
        let updated = repository
            .mark_uploaded_with_compatibility(
                ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
                FormatCompatibility::TranscodeRequired,
            )
            .await
            .unwrap();

        assert!(!updated);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_uploaded_with_compatibility_updates_status_and_flags(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool);
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB3");

        repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 321)
            .await
            .unwrap();

        let updated = repository
            .mark_uploaded_with_compatibility(ulid, FormatCompatibility::BrowserCompatible)
            .await
            .unwrap();
        assert!(updated);

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(found.status, VideoStatus::Uploaded.as_ref());
        assert!(found.browser_compatible);
        assert!(!found.transmux_required);
        assert!(!found.transcode_required);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_video_by_ulid_maps_optional_fields(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2");

        repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 789)
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
        assert_eq!(found.status, VideoStatus::PendingUpload.as_ref());
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

    #[sqlx::test(migrations = "./migrations")]
    async fn recover_pending_jobs_resets_processing_states_and_returns_all_uploaded(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());

        let u_uploaded = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC1");
        let u_transmuxing = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC2");
        let u_transcoding = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC3");
        let u_ready = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC4");
        let u_pending = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC5");

        // Insert rows with specific statuses
        for (u, status) in [
            (u_uploaded, VideoStatus::Uploaded.as_ref()),
            (u_transmuxing, VideoStatus::Transmuxing.as_ref()),
            (u_transcoding, VideoStatus::Transcoding.as_ref()),
            (u_ready, VideoStatus::Ready.as_ref()),
            (u_pending, VideoStatus::PendingUpload.as_ref()),
        ] {
            repository
                .create_pending_video(u, &raw_key(u), &content_type("video/mp4"), 100)
                .await
                .unwrap();
            sqlx::query!(
                "UPDATE videos SET status = $1 WHERE ulid = $2",
                status,
                u.to_string()
            )
            .execute(&pool)
            .await
            .unwrap();
        }

        let recovered = repository.recover_pending_jobs().await.unwrap();

        // Should return the 3 jobs that need processing
        assert_eq!(recovered.len(), 3);
        assert!(recovered.contains(&u_uploaded));
        assert!(recovered.contains(&u_transmuxing));
        assert!(recovered.contains(&u_transcoding));

        // DB statuses should now all be 'uploaded' for those 3
        for u in [u_uploaded, u_transmuxing, u_transcoding] {
            let found = repository.find_video_by_ulid(u).await.unwrap().unwrap();
            assert_eq!(found.status, VideoStatus::Uploaded.as_ref());
        }

        // 'ready' and 'pending_upload' should remain untouched
        assert_eq!(
            repository
                .find_video_by_ulid(u_ready)
                .await
                .unwrap()
                .unwrap()
                .status,
            "ready"
        );
        assert_eq!(
            repository
                .find_video_by_ulid(u_pending)
                .await
                .unwrap()
                .unwrap()
                .status,
            "pending_upload"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_zombie_jobs_failed_updates_only_old_processing_jobs(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let timeout = NonZeroU64::new(7200).unwrap(); // 2 hours

        let z_transcoding = ulid("01ARZ3NDEKTSV4RRFFQ69G5FD1"); // Zombie transcoding
        let z_transmuxing = ulid("01ARZ3NDEKTSV4RRFFQ69G5FD2"); // Zombie transmuxing
        let a_transcoding = ulid("01ARZ3NDEKTSV4RRFFQ69G5FD3"); // Active transcoding
        let o_uploaded = ulid("01ARZ3NDEKTSV4RRFFQ69G5FD4"); // Old uploaded (not processing)

        // Setup rows
        for u in [z_transcoding, z_transmuxing, a_transcoding, o_uploaded] {
            repository
                .create_pending_video(u, &raw_key(u), &content_type("video/mp4"), 100)
                .await
                .unwrap();
        }

        // Manually backdate updated_at to simulate time passing
        sqlx::query!("UPDATE videos SET status = 'transcoding', updated_at = NOW() - INTERVAL '3 hours' WHERE ulid = $1", z_transcoding.to_string()).execute(&pool).await.unwrap();
        sqlx::query!("UPDATE videos SET status = 'transmuxing', updated_at = NOW() - INTERVAL '3 hours' WHERE ulid = $1", z_transmuxing.to_string()).execute(&pool).await.unwrap();
        sqlx::query!("UPDATE videos SET status = 'uploaded', updated_at = NOW() - INTERVAL '3 hours' WHERE ulid = $1", o_uploaded.to_string()).execute(&pool).await.unwrap();

        // Active job is only 1 hour old (under the 2 hour timeout)
        sqlx::query!("UPDATE videos SET status = 'transcoding', updated_at = NOW() - INTERVAL '1 hour' WHERE ulid = $1", a_transcoding.to_string()).execute(&pool).await.unwrap();

        let affected = repository.mark_zombie_jobs_failed(timeout).await.unwrap();

        // Only the two old processing jobs should be affected
        assert_eq!(affected, 2);

        // Verify zombies were failed
        assert_eq!(
            repository
                .find_video_by_ulid(z_transcoding)
                .await
                .unwrap()
                .unwrap()
                .status,
            "failed"
        );
        assert_eq!(
            repository
                .find_video_by_ulid(z_transmuxing)
                .await
                .unwrap()
                .unwrap()
                .status,
            "failed"
        );

        // Verify active processing and old non-processing jobs were untouched
        assert_eq!(
            repository
                .find_video_by_ulid(a_transcoding)
                .await
                .unwrap()
                .unwrap()
                .status,
            "transcoding"
        );
        assert_eq!(
            repository
                .find_video_by_ulid(o_uploaded)
                .await
                .unwrap()
                .unwrap()
                .status,
            "uploaded"
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn delete_stale_pending_uploads_removes_old_rows(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let ulid_old = ulid("01ARZ3NDEKTSV4RRFFQ69G5FE1");
        let ulid_new = ulid("01ARZ3NDEKTSV4RRFFQ69G5FE2");

        // Insert two pending_upload rows
        repository
            .create_pending_video(
                ulid_old,
                &raw_key(ulid_old),
                &content_type("video/mp4"),
                100,
            )
            .await
            .unwrap();
        repository
            .create_pending_video(
                ulid_new,
                &raw_key(ulid_new),
                &content_type("video/mp4"),
                200,
            )
            .await
            .unwrap();

        // Manually backdate one row by 2 hours
        sqlx::query!(
            "UPDATE videos SET created_at = NOW() - INTERVAL '2 hours' WHERE ulid = $1",
            ulid_old.to_string()
        )
        .execute(&pool)
        .await
        .unwrap();

        let deleted = repository
            .delete_stale_pending_uploads(Duration::from_secs(3600))
            .await
            .unwrap();

        assert_eq!(deleted, 1);

        // Old row should be gone, new row still there
        let found_old = repository.find_video_by_ulid(ulid_old).await.unwrap();
        let found_new = repository.find_video_by_ulid(ulid_new).await.unwrap();
        assert!(found_old.is_none());
        assert!(found_new.is_some());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn update_status_updates_status(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FF1");

        repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 100)
            .await
            .unwrap();

        repository
            .update_status(ulid, VideoStatus::Transmuxing)
            .await
            .unwrap();

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(found.status, VideoStatus::Transmuxing.as_ref());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn set_transmux_key_sets_key(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FF1");

        repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 100)
            .await
            .unwrap();

        repository
            .set_transmux_key(ulid, &TransmuxKey::new(ulid, ContainerFormat::Mp4))
            .await
            .unwrap();

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(
            found.transmux_key.as_ref().map(|k| &**k),
            Some("transmux/01ARZ3NDEKTSV4RRFFQ69G5FF1/output.mp4")
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn recover_pending_jobs_returns_empty_when_no_jobs_exist(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool);
        let recovered = repository.recover_pending_jobs().await.unwrap();

        assert!(recovered.is_empty());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn recover_pending_jobs_ignores_terminal_and_pending_states(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let u_ready = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC4");
        let u_failed = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC5");
        let u_pending = ulid("01ARZ3NDEKTSV4RRFFQ69G5FC6");

        for (u, status) in [
            (u_ready, "ready"),
            (u_failed, "failed"),
            (u_pending, "pending_upload"),
        ] {
            repository
                .create_pending_video(u, &raw_key(u), &content_type("video/mp4"), 100)
                .await
                .unwrap();
            sqlx::query!(
                "UPDATE videos SET status = $1 WHERE ulid = $2",
                status,
                u.to_string()
            )
            .execute(&pool)
            .await
            .unwrap();
        }

        let recovered = repository.recover_pending_jobs().await.unwrap();

        // It must strictly ignore jobs that are complete, permanently failed, or haven't finished uploading
        assert!(recovered.is_empty());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_zombie_jobs_failed_returns_zero_when_no_jobs_exist(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool);
        let timeout = NonZeroU64::new(7200).unwrap();

        let affected = repository.mark_zombie_jobs_failed(timeout).await.unwrap();
        assert_eq!(affected, 0);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn mark_zombie_jobs_failed_ignores_old_jobs_in_terminal_states(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let timeout = NonZeroU64::new(7200).unwrap();

        let u_ready = ulid("01ARZ3NDEKTSV4RRFFQ69G5FD1");
        let u_failed = ulid("01ARZ3NDEKTSV4RRFFQ69G5FD2");
        let u_uploaded = ulid("01ARZ3NDEKTSV4RRFFQ69G5FD3"); // Uploaded, but hasn't started processing

        for u in [u_ready, u_failed, u_uploaded] {
            repository
                .create_pending_video(u, &raw_key(u), &content_type("video/mp4"), 100)
                .await
                .unwrap();
        }

        // Backdate them all past the timeout (e.g., jobs completed 5 hours ago)
        for (u, status) in [
            (u_ready, "ready"),
            (u_failed, "failed"),
            (u_uploaded, "uploaded"),
        ] {
            sqlx::query!(
                "UPDATE videos SET status = $1, updated_at = NOW() - INTERVAL '5 hours' WHERE ulid = $2",
                status, u.to_string()
            ).execute(&pool).await.unwrap();
        }

        let affected = repository.mark_zombie_jobs_failed(timeout).await.unwrap();

        // Because they aren't in 'transcoding' or 'transmuxing', the query must leave them alone
        assert_eq!(affected, 0);

        // Sanity check they weren't altered
        assert_eq!(
            repository
                .find_video_by_ulid(u_ready)
                .await
                .unwrap()
                .unwrap()
                .status,
            VideoStatus::Ready.as_ref()
        );
        assert_eq!(
            repository
                .find_video_by_ulid(u_uploaded)
                .await
                .unwrap()
                .unwrap()
                .status,
            VideoStatus::Uploaded.as_ref()
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn set_manifest_key_updates_key(pool: PgPool) {
        let repository = PgVideoRepository::with_pool(pool.clone());
        let ulid = ulid("01ARZ3NDEKTSV4RRFFQ69G5FF2");

        repository
            .create_pending_video(ulid, &raw_key(ulid), &content_type("video/mp4"), 100)
            .await
            .unwrap();

        let manifest_key = ManifestKey::from(ulid.to_string());
        repository
            .set_manifest_key(ulid, &manifest_key)
            .await
            .unwrap();

        let found = repository.find_video_by_ulid(ulid).await.unwrap().unwrap();
        assert_eq!(
            found.manifest_key.as_ref().map(|k| &**k),
            Some(&*manifest_key)
        );
    }
}
