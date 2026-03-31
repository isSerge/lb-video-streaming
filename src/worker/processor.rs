use std::{path::PathBuf, sync::Arc};

use ulid::Ulid;

use super::WorkerError;
use crate::{
    domain::{TransmuxKey, UploadContentType},
    file_transfer::FileTransfer,
    media_probe::MediaProbe,
    media_transcoder::MediaTranscoder,
    repository::{VideoRecord, VideoRepository},
    storage::Storage,
};

/// VideoProcessor handles the core logic of processing uploaded videos, including probing media info, determining if transmuxing is needed, and performing transcoding if necessary.
#[derive(Clone)]
pub struct VideoProcessor {
    repository: Arc<dyn VideoRepository>,
    storage: Arc<dyn Storage>,
    media_probe: Arc<dyn MediaProbe>,
    transcoder: Arc<dyn MediaTranscoder>,
    file_transfer: Arc<dyn FileTransfer>,
    temp_root: PathBuf,
}

impl VideoProcessor {
    pub fn new(
        repository: Arc<dyn VideoRepository>,
        storage: Arc<dyn Storage>,
        media_probe: Arc<dyn MediaProbe>,
        transcoder: Arc<dyn MediaTranscoder>,
        file_transfer: Arc<dyn FileTransfer>,
        temp_root: PathBuf,
    ) -> Self {
        Self {
            file_transfer,
            repository,
            storage,
            media_probe,
            transcoder,
            temp_root,
        }
    }

    /// Process a video by its ULID, performing necessary steps like transmuxing and transcoding.
    pub async fn process(&self, ulid: Ulid) -> Result<(), WorkerError> {
        let record = self
            .repository
            .find_video_by_ulid(ulid)
            .await?
            .ok_or(WorkerError::NotFound(ulid))?;

        if record.transmux_required {
            self.run_transmux(ulid, &record).await?;
        }

        // TODO: add other processing steps like transcoding here

        Ok(())
    }

    /// Run the transmuxing step for a video, downloading the raw file, probing it, and performing transmuxing if needed.
    async fn run_transmux(&self, ulid: Ulid, record: &VideoRecord) -> Result<(), WorkerError> {
        tracing::info!(%ulid, "starting transmux");

        // Update status to "transmuxing" before starting the operation
        self.repository.update_status(ulid, "transmuxing").await?;

        let temp_dir = tempfile::tempdir_in(&self.temp_root)?;
        let raw_path = temp_dir.path().join("input");

        // Download raw file from storage to local temp path for processing
        let raw_url = self.storage.create_download_url(&record.raw_key).await?;
        tracing::info!(url = %raw_url, "downloading raw file for transmuxing");
        self.file_transfer.download(raw_url, &raw_path).await?;

        // Probe media info to determine target container format
        let metadata = self.media_probe.probe_file(&raw_path).await?;
        let target_container = metadata
            .transmux_target_container()
            .ok_or(WorkerError::NoTargetContainer)?;

        // Build output path with correct extension based on target container
        let output_ext = target_container.extension();
        let output_path = temp_dir.path().join(format!("output.{}", output_ext));

        // Perform transmuxing using the media transcoder
        self.transcoder
            .transmux(&raw_path, target_container, &output_path)
            .await?;

        // Upload transmuxed file back to storage and update video record with new key and status
        let content_type = target_container
            .mime_type_str()
            .parse::<UploadContentType>()?;
        let transmux_key = TransmuxKey::new(ulid, target_container);
        let upload_url = self
            .storage
            .create_transmux_upload_url(&transmux_key, &content_type)
            .await?;

        tracing::info!(%ulid, url = %upload_url, "uploading transmuxed file");
        self.file_transfer
            .upload(&output_path, upload_url, &content_type)
            .await?;

        // Set the new transmux key and update status to "transmuxed"
        self.repository
            .set_transmux_key(ulid, &transmux_key)
            .await?;
        self.repository.update_status(ulid, "transmuxed").await?;

        tracing::info!(%ulid, "transmux phase completed");

        Ok(())
    }
}
