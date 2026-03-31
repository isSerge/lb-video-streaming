use std::{path::PathBuf, sync::Arc};

use ulid::Ulid;

use super::WorkerError;
use crate::{
    domain::{TransmuxKey, UploadContentType, VideoStatus},
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
        self.repository
            .update_status(ulid, VideoStatus::Transmuxing)
            .await?;

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

        // Set the new transmux key and update status to "transcoding" after successful upload
        self.repository
            .set_transmux_key(ulid, &transmux_key)
            .await?;
        self.repository
            .update_status(ulid, VideoStatus::Transcoding)
            .await?;

        tracing::info!(%ulid, "transmux phase completed, starting transcoding");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{AudioCodec, ContainerFormat, MediaMetadata, RawUploadKey, VideoCodec},
        file_transfer::port::MockFileTransfer,
        media_probe::port::MockMediaProbe,
        media_transcoder::port::MockMediaTranscoder,
        repository::port::MockVideoRepository,
        storage::port::MockStorage,
    };
    use mockall::predicate::*;
    use std::path::Path;
    use url::Url;

    // TODO: consider using builder pattern and move to common test utils
    fn mock_video_record(ulid: Ulid, transmux_required: bool) -> VideoRecord {
        VideoRecord {
            ulid,
            status: "uploaded".to_string(),
            raw_key: RawUploadKey::from(ulid),
            transmux_key: None,
            manifest_key: None,
            browser_compatible: false,
            transmux_required,
            transcode_required: true,
        }
    }

    fn dummy_url() -> Url {
        Url::parse("https://example.com/dummy").unwrap()
    }

    #[tokio::test]
    async fn process_returns_error_if_video_not_found() {
        let ulid = Ulid::new();
        let mut repo = MockVideoRepository::new();

        // Return None to simulate video not found
        repo.expect_find_video_by_ulid()
            .with(eq(ulid))
            .once()
            .returning(|_| Ok(None));

        let processor = VideoProcessor::new(
            Arc::new(repo),
            Arc::new(MockStorage::new()),
            Arc::new(MockMediaProbe::new()),
            Arc::new(MockMediaTranscoder::new()),
            Arc::new(MockFileTransfer::new()),
            std::env::temp_dir(),
        );

        let result = processor.process(ulid).await;
        assert!(matches!(result, Err(WorkerError::NotFound(id)) if id == ulid));
    }

    #[tokio::test]
    async fn process_skips_transmux_when_not_required() {
        let ulid = Ulid::new();
        let mut repo = MockVideoRepository::new();

        // Return a record where transmux_required = false
        repo.expect_find_video_by_ulid()
            .with(eq(ulid))
            .once()
            .returning(move |_| Ok(Some(mock_video_record(ulid, false))));

        // NO other mocks should be called.

        let processor = VideoProcessor::new(
            Arc::new(repo),
            Arc::new(MockStorage::new()),
            Arc::new(MockMediaProbe::new()),
            Arc::new(MockMediaTranscoder::new()),
            Arc::new(MockFileTransfer::new()),
            std::env::temp_dir(),
        );

        let result = processor.process(ulid).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_transmux_orchestrates_full_pipeline_successfully() {
        let ulid = Ulid::new();
        let temp_root = tempfile::tempdir().unwrap().keep();

        let mut repo = MockVideoRepository::new();

        // Return a record where transmux_required = true
        repo.expect_find_video_by_ulid()
            .with(eq(ulid))
            .once()
            .returning(move |_| Ok(Some(mock_video_record(ulid, true))));

        // Expect status updates to "transmuxing" and then "transcoding"
        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Transmuxing))
            .once()
            .returning(|_, _| Ok(()));

        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Transcoding))
            .once()
            .returning(|_, _| Ok(()));

        // Expect the transmux key to be set with the correct format
        repo.expect_set_transmux_key()
            .withf(move |u, key| *u == ulid && key.ends_with(".mp4"))
            .once()
            .returning(|_, _| Ok(()));

        let mut storage = MockStorage::new();

        // Expect a download URL to be created for the raw key
        storage
            .expect_create_download_url()
            .once()
            .returning(|_| Ok(dummy_url()));

        // Expect a transmux upload URL to be created with the correct key and content type
        storage
            .expect_create_transmux_upload_url()
            .withf(|key, ct| key.ends_with(".mp4") && &**ct == "video/mp4")
            .once()
            .returning(|_, _| Ok(dummy_url()));

        let mut file_transfer = MockFileTransfer::new();

        // Expect the raw file to be downloaded to the correct path
        file_transfer
            .expect_download()
            .withf(|url: &Url, path: &Path| {
                url.as_str() == dummy_url().as_str() && path.ends_with("input")
            })
            .once()
            .returning(|_, _| Ok(()));

        // Expect the transmuxed file to be uploaded from the correct path with the correct content type
        file_transfer
            .expect_upload()
            .withf(|path: &Path, url: &Url, ct: &UploadContentType| {
                path.ends_with("output.mp4")
                    && url.as_str() == dummy_url().as_str()
                    && &**ct == "video/mp4"
            })
            .once()
            .returning(|_, _, _| Ok(()));

        let mut probe = MockMediaProbe::new();

        // Expect the raw file to be probed and return metadata indicating it needs to be transmuxed to MP4
        probe
            .expect_probe_file()
            .withf(|path: &Path| path.ends_with("input"))
            .once()
            .returning(|_| {
                Ok(MediaMetadata {
                    container_format: Some(ContainerFormat::Matroska), // MKV
                    video_codec: Some(VideoCodec::H264),               // Maps to MP4 target
                    audio_codec: Some(AudioCodec::Aac),
                })
            });

        let mut transcoder = MockMediaTranscoder::new();

        // Expect the transcoder to be called to transmux the file to MP4 format with the correct input and output paths
        transcoder
            .expect_transmux()
            .withf(
                |in_path: &Path, target: &ContainerFormat, out_path: &Path| {
                    in_path.ends_with("input")
                        && *target == ContainerFormat::Mp4
                        && out_path.ends_with("output.mp4")
                },
            )
            .once()
            .returning(|_, _, _| Ok(()));

        let processor = VideoProcessor::new(
            Arc::new(repo),
            Arc::new(storage),
            Arc::new(probe),
            Arc::new(transcoder),
            Arc::new(file_transfer),
            temp_root,
        );

        let result = processor.process(ulid).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_transmux_fails_if_no_target_container_identified() {
        let ulid = Ulid::new();
        let mut repo = MockVideoRepository::new();

        // Return a record where transmux_required = true
        repo.expect_find_video_by_ulid()
            .once()
            .returning(move |_| Ok(Some(mock_video_record(ulid, true))));

        // Expect status update to "transmuxing" before probing
        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Transmuxing))
            .once()
            .returning(|_, _| Ok(()));

        let mut storage = MockStorage::new();

        // Expect a download URL to be created for the raw key
        storage
            .expect_create_download_url()
            .once()
            .returning(|_| Ok(dummy_url()));

        let mut file_transfer = MockFileTransfer::new();

        // Expect the raw file to be downloaded to the correct path
        file_transfer
            .expect_download()
            .once()
            .returning(|_, _| Ok(()));

        let mut probe = MockMediaProbe::new();

        // Expect the raw file to be probed and return metadata that does not map to any target container
        probe.expect_probe_file().once().returning(|_| {
            Ok(MediaMetadata {
                container_format: Some(ContainerFormat::Flv),
                video_codec: Some(VideoCodec::Unknown), // No target container for Unknown
                audio_codec: None,
            })
        });

        // Transcoder and Upload should NOT be called

        let processor = VideoProcessor::new(
            Arc::new(repo),
            Arc::new(storage),
            Arc::new(probe),
            Arc::new(MockMediaTranscoder::new()),
            Arc::new(file_transfer),
            tempfile::tempdir().unwrap().keep(),
        );

        let result = processor.process(ulid).await;
        assert!(matches!(result, Err(WorkerError::NoTargetContainer)));
    }
}
