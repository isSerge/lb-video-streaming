use std::{path::Path, str::FromStr, sync::Arc, time::Duration};

use tokio::{sync::Semaphore, task::JoinSet};
use ulid::Ulid;

use super::WorkerError;
use crate::{
    config::WorkerConfig,
    domain::{HLSKey, ManifestKey, TransmuxKey, UploadContentType, VideoStatus},
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
    config: WorkerConfig,
}

impl VideoProcessor {
    pub fn new(
        repository: Arc<dyn VideoRepository>,
        storage: Arc<dyn Storage>,
        media_probe: Arc<dyn MediaProbe>,
        transcoder: Arc<dyn MediaTranscoder>,
        file_transfer: Arc<dyn FileTransfer>,
        config: WorkerConfig,
    ) -> Self {
        Self {
            file_transfer,
            repository,
            storage,
            media_probe,
            transcoder,
            config,
        }
    }

    /// Process a video by its ULID, performing necessary steps like transmuxing and transcoding.
    #[tracing::instrument(skip(self))]
    pub async fn process(&self, ulid: Ulid) -> Result<(), WorkerError> {
        let record = self
            .repository
            .find_video_by_ulid(ulid)
            .await?
            .ok_or(WorkerError::NotFound(ulid))?;

        // If the video requires transmuxing, run the transmux step first and get the TransmuxKey
        let maybe_transmux_key = if record.transmux_required {
            Some(self.run_transmux(ulid, &record).await?)
        } else {
            None
        };

        // Run HLS transcoding step, passing the TransmuxKey if available
        self.run_hls_transcode(ulid, &record, maybe_transmux_key)
            .await?;

        Ok(())
    }

    /// Run the transmuxing step for a video, downloading the raw file, probing it, and performing transmuxing.
    /// Returns the new TransmuxKey if successful, which can be used for subsequent transcoding steps.
    #[tracing::instrument(skip(self, record))]
    async fn run_transmux(
        &self,
        ulid: Ulid,
        record: &VideoRecord,
    ) -> Result<TransmuxKey, WorkerError> {
        // Update status to "transmuxing" before starting the operation
        self.repository
            .update_status(ulid, VideoStatus::Transmuxing)
            .await?;

        let temp_dir = tempfile::Builder::new()
            .prefix(&format!("transmux-{}", ulid))
            .tempdir_in(&self.config.temp_dir)?;
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
            .transmux(
                &raw_path,
                target_container,
                &output_path,
                Duration::from_secs(self.config.transmux_timeout_secs),
            )
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

        tracing::info!(url = %upload_url, "uploading transmuxed file");
        self.file_transfer
            .upload(&output_path, upload_url, &content_type)
            .await?;

        // Set the new transmux key
        self.repository
            .set_transmux_key(ulid, &transmux_key)
            .await?;

        Ok(transmux_key)
    }

    /// Run the HLS transcoding step for a video, which includes generating HLS segments and manifest, uploading them to storage, and updating the video record with the manifest key.
    #[tracing::instrument(skip(self, record))]
    async fn run_hls_transcode(
        &self,
        ulid: Ulid,
        record: &VideoRecord,
        transmux_key: Option<TransmuxKey>,
    ) -> Result<(), WorkerError> {
        // Update status to "transcoding" before starting the operation
        self.repository
            .update_status(ulid, VideoStatus::Transcoding)
            .await?;

        // Create a temp directory
        let temp_dir = tempfile::Builder::new()
            .prefix(&format!("hls-{}", ulid))
            .tempdir_in(&self.config.temp_dir)?;

        // Download the source file (either raw or transmuxed) to the temp directory for processing
        let download_url = match &transmux_key {
            Some(transmux_key) => {
                self.storage
                    .create_transmux_download_url(transmux_key)
                    .await
            }
            None => self.storage.create_download_url(&record.raw_key).await,
        }?;

        let input_path = temp_dir.path().join("input");
        tracing::info!(url = %download_url, "downloading file for HLS transcoding");
        self.file_transfer
            .download(download_url, &input_path)
            .await?;

        // Define output directory for HLS segments and manifest within the temp directory
        let output_dir = temp_dir.path().join("hls");
        tokio::fs::create_dir_all(&output_dir).await?;

        // Spawn the keep-alive ticker to update the video's updated_at timestamp periodically while transcoding is in progress
        let repo_clone = self.repository.clone();
        let interval_secs = self.config.transcode_heartbeat_interval_secs;

        let keep_alive = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                tracing::debug!("transcode heartbeat \u{2014} ffmpeg still running");
                if let Err(e) = repo_clone.update_updated_at(ulid).await {
                    tracing::error!(error = %e, "failed to update updated_at");
                }
            }
        });

        // Run the HLS transcoding process, which generates segments and manifest in the output directory.
        let transcode_result = self
            .transcoder
            .hls_transcode(
                &input_path,
                &output_dir,
                Duration::from_secs(self.config.transcode_timeout_secs),
            )
            .await;

        // Kill the keep-alive loop immediately after transcoding finishes, whether it succeeded or failed, to avoid unnecessary database updates.
        keep_alive.abort();

        // If transcoding failed, return the error without proceeding to uploads or database updates.
        let manifest_path = transcode_result?;

        // Upload manifest
        let manifest_key = ManifestKey::new(ulid);
        let upload_content_type = UploadContentType::from_str("application/vnd.apple.mpegurl")?;
        let manifest_url = self
            .storage
            .create_manifest_upload_url(&manifest_key, &upload_content_type)
            .await?;

        tracing::info!(url = %manifest_url, "uploading HLS manifest");
        self.file_transfer
            .upload(&manifest_path, manifest_url, &upload_content_type)
            .await?;

        // Upload segments
        self.upload_segments(ulid, &output_dir).await?;

        // Update timestamp after all uploads
        self.repository.update_updated_at(ulid).await?;

        // Set the manifest key and update status to "ready" after successful uploads
        self.repository
            .set_manifest_key(ulid, &manifest_key)
            .await?;

        self.repository
            .update_status(ulid, VideoStatus::Ready)
            .await?;

        // Clean up intermediate transmux file from storage
        if let Some(transmux_key) = &record.transmux_key {
            tracing::info!("cleaning up intermediate transmux file from storage");
            if let Err(e) = self.storage.delete_object(transmux_key).await {
                tracing::warn!(error = %e, "failed to delete transmux file from R2, leaving stranded bytes");
            }

            // Always clear the key so the API stops serving the transmux URL.
            if let Err(e) = self.repository.clear_transmux_key(ulid).await {
                tracing::error!(error = %e, "failed to clear transmux key in database");
                // We don't return an error because the HLS phase already succeeded.
            }
        }

        // Note: We do NOT delete the raw file here.
        // As per the architecture doc, raw file cleanup is handled seamlessly
        // by a Cloudflare R2 Object Lifecycle rule (e.g., 30-day expiration).

        Ok(())
    }

    /// Upload HLS segments in parallel
    #[tracing::instrument(skip(self))]
    async fn upload_segments(&self, ulid: Ulid, output_dir: &Path) -> Result<(), WorkerError> {
        let mut segment_paths = Vec::new();
        let mut read_dir = tokio::fs::read_dir(output_dir).await?;

        // Collect all segment paths (assuming they have .ts extension)
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ts") {
                segment_paths.push(path);
            }
        }

        tracing::info!(count = segment_paths.len(), "uploading HLS segments");

        // Use a semaphore to limit concurrency of segment uploads and a JoinSet to manage the upload tasks
        let semaphore = Arc::new(Semaphore::new(self.config.segment_upload_concurrency));
        let mut join_set = JoinSet::new();

        for path in segment_paths {
            let filename = path.file_name().unwrap().to_string_lossy().into_owned();
            let segment_key = HLSKey::new(ulid, &filename);
            let storage = self.storage.clone();
            let file_transfer = self.file_transfer.clone();

            // Acquire a permit from the semaphore before spawning the upload task
            let permit = semaphore.clone().acquire_owned().await?;

            // Spawn a task for each segment upload, acquiring a permit from the semaphore to limit concurrency
            join_set.spawn(async move {
                let _permit = permit;
                tracing::debug!(segment = %filename, "uploading HLS segment");
                let ts_content_type = UploadContentType::from_str("video/MP2T")?;
                let segment_url = storage
                    .create_hls_segment_upload_url(&segment_key, &ts_content_type)
                    .await?;
                file_transfer
                    .upload(&path, segment_url, &ts_content_type)
                    .await?;
                Ok::<_, WorkerError>(())
            });
        }

        // Wait for all segment upload tasks to complete, returning an error if any task fails
        while let Some(res) = join_set.join_next().await {
            res??; // First ? is for JoinSet error, second ? is for the upload task error
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{AudioCodec, ContainerFormat, MediaMetadata, RawUploadKey, VideoCodec},
        file_transfer::{FileTransferError, port::MockFileTransfer},
        media_probe::port::MockMediaProbe,
        media_transcoder::{TranscoderError, port::MockMediaTranscoder},
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
            status: VideoStatus::PendingUpload,
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

    fn dummy_worker_config() -> WorkerConfig {
        WorkerConfig {
            max_concurrent_transcodes: 1.try_into().unwrap(),
            temp_dir: std::env::temp_dir(),
            segment_upload_concurrency: 4,
            transcode_heartbeat_interval_secs: 30.try_into().unwrap(),
            zombie_timeout_secs: 7200.try_into().unwrap(),
            zombie_sweep_interval_secs: 3600.try_into().unwrap(),
            worker_channel_buffer_size: 100,
            http_connect_timeout_secs: 10,
            http_read_timeout_secs: 30,
            transmux_timeout_secs: 300,
            transcode_timeout_secs: 1800,
            file_transfer_retry_min_delay_ms: 50,
            file_transfer_retry_max_delay_ms: 100,
            file_transfer_retry_max_times: 3,
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_min_recovery_secs: 10,
            circuit_breaker_max_recovery_secs: 60,
            job_requeue_delay_secs: 5,
        }
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
            dummy_worker_config(),
        );

        let result = processor.process(ulid).await;
        assert!(matches!(result, Err(WorkerError::NotFound(id)) if id == ulid));
    }

    #[tokio::test]
    async fn run_transmux_orchestrates_full_pipeline_successfully() {
        let ulid = Ulid::new();
        let _temp_dir = tempfile::tempdir().unwrap();

        let mut repo = MockVideoRepository::new();

        // Return a record where transmux_required = true
        let record = mock_video_record(ulid, true);

        // Expect status update to "transmuxing"
        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Transmuxing))
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
                |in_path: &Path, target: &ContainerFormat, out_path: &Path, timeout: &Duration| {
                    in_path.ends_with("input")
                        && *target == ContainerFormat::Mp4
                        && out_path.ends_with("output.mp4")
                        && timeout.as_secs() == 300
                },
            )
            .once()
            .returning(|_, _, _, _| Ok(()));

        let processor = VideoProcessor::new(
            Arc::new(repo),
            Arc::new(storage),
            Arc::new(probe),
            Arc::new(transcoder),
            Arc::new(file_transfer),
            dummy_worker_config(),
        );

        let result = processor.run_transmux(ulid, &record).await;
        assert!(
            result.is_ok(),
            "expected run_transmux to succeed but got error: {:?}",
            result.err()
        );
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
            dummy_worker_config(),
        );

        let result = processor.process(ulid).await;
        assert!(matches!(result, Err(WorkerError::NoTargetContainer)));
    }

    #[tokio::test]
    async fn run_hls_transcode_orchestrates_successfully() {
        let ulid = Ulid::new();
        let _temp_dir = tempfile::tempdir().unwrap();
        let manifest_key = ManifestKey::new(ulid);
        let manifest_key_clone = manifest_key.clone();

        let mut repo = MockVideoRepository::new();
        let mut record = mock_video_record(ulid, false);
        // Simulate a transmuxed file exists to trigger cleanup
        let transmux_key = TransmuxKey::new(ulid, ContainerFormat::Mp4);
        record.transmux_key = Some(transmux_key.clone());

        // Expect status update to "transcoding" before starting
        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Transcoding))
            .once()
            .returning(|_, _| Ok(()));

        // Expect update updated_at to be called during transcoding
        repo.expect_update_updated_at()
            .with(eq(ulid))
            .once()
            .returning(|_| Ok(()));

        // Expect the manifest key to be set with the correct value after transcoding
        repo.expect_set_manifest_key()
            .withf(move |u, key| *u == ulid && *key == manifest_key)
            .once()
            .returning(|_, _| Ok(()));

        // Update status to "ready" after transcoding
        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Ready))
            .once()
            .returning(|_, _| Ok(()));

        // Expect clear_transmux_key to be called during cleanup
        repo.expect_clear_transmux_key()
            .with(eq(ulid))
            .once()
            .returning(|_| Ok(()));

        let mut storage = MockStorage::new();

        // Expect a download URL to be created for the transmux key (since it is Some)
        storage
            .expect_create_transmux_download_url()
            .with(eq(transmux_key.clone()))
            .once()
            .returning(|_| Ok(dummy_url()));

        // Expect a manifest upload URL to be created with the correct key and content type
        storage
            .expect_create_manifest_upload_url()
            .withf(move |key, ct| {
                *key == manifest_key_clone && &**ct == "application/vnd.apple.mpegurl"
            })
            .once()
            .returning(|_, _| Ok(dummy_url()));

        // Expect the transmuxed object to be deleted during cleanup
        let transmux_key_str = transmux_key.to_string();
        storage
            .expect_delete_object()
            .with(eq(transmux_key_str))
            .once()
            .returning(|_| Ok(()));

        let mut file_transfer = MockFileTransfer::new();

        // Expect the file to be downloaded to the correct path
        file_transfer
            .expect_download()
            .withf(|url: &Url, path: &Path| {
                url.as_str() == dummy_url().as_str() && path.ends_with("input")
            })
            .once()
            .returning(|_, _| Ok(()));

        // Expect the manifest file to be uploaded from the correct path with the correct content type
        file_transfer
            .expect_upload()
            .withf(|path: &Path, url: &Url, ct: &UploadContentType| {
                path.ends_with("manifest.m3u8")
                    && url.as_str() == dummy_url().as_str()
                    && &**ct == "application/vnd.apple.mpegurl"
            })
            .once()
            .returning(|_, _, _| Ok(()));

        let mut transcoder = MockMediaTranscoder::new();

        // HLS transcode should be called
        transcoder
            .expect_hls_transcode()
            .withf(|in_path: &Path, out_dir: &Path, timeout: &Duration| {
                in_path.ends_with("input") && out_dir.ends_with("hls") && timeout.as_secs() == 1800
            })
            .once()
            .returning(|_, out_dir, _| {
                let manifest = out_dir.join("manifest.m3u8");
                std::fs::create_dir_all(out_dir).unwrap();
                std::fs::write(&manifest, "dummy manifest").unwrap();
                Ok(manifest)
            });

        let processor = VideoProcessor::new(
            Arc::new(repo),
            Arc::new(storage),
            Arc::new(MockMediaProbe::new()),
            Arc::new(transcoder),
            Arc::new(file_transfer),
            dummy_worker_config(),
        );

        let result = processor
            .run_hls_transcode(ulid, &record, Some(transmux_key))
            .await;
        assert!(
            result.is_ok(),
            "expected run_hls_transcode to succeed but got error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn run_hls_transcode_bubbles_error_on_ffmpeg_failure() {
        let ulid = Ulid::new();
        let _temp_dir = tempfile::tempdir().unwrap();
        let mut repo = MockVideoRepository::new();

        // It should still mark as transcoding and update updated_at initially
        repo.expect_update_status()
            .with(eq(ulid), eq(VideoStatus::Transcoding))
            .once()
            .returning(|_, _| Ok(()));

        // The keep-alive ticker should call update_updated_at at least once during transcoding
        repo.expect_update_updated_at().returning(|_| Ok(()));

        let mut storage = MockStorage::new();
        storage
            .expect_create_download_url()
            .once()
            .returning(|_| Ok(dummy_url()));

        let mut file_transfer = MockFileTransfer::new();
        file_transfer
            .expect_download()
            .once()
            .returning(|_, _| Ok(()));

        let mut transcoder = MockMediaTranscoder::new();

        // Simulate an FFmpeg failure during HLS transcoding by returning an error from the mock transcoder.
        transcoder
            .expect_hls_transcode()
            .once()
            .returning(|_, _, _| {
                Err(TranscoderError::TranscodeFailed {
                    stderr: "boom".into(),
                })
            });

        // The pipeline MUST halt here. Storage upload and DB finalize should NOT be called.

        let processor = VideoProcessor::new(
            Arc::new(repo),
            Arc::new(storage),
            Arc::new(MockMediaProbe::new()),
            Arc::new(transcoder),
            Arc::new(file_transfer),
            dummy_worker_config(),
        );

        let record = mock_video_record(ulid, false);
        let result = processor.run_hls_transcode(ulid, &record, None).await;

        assert!(matches!(
            result,
            Err(WorkerError::Transcoder(
                TranscoderError::TranscodeFailed { .. }
            ))
        ));
    }

    #[tokio::test]
    async fn upload_segments_bubbles_error_if_any_transfer_fails() {
        let ulid = Ulid::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let hls_dir = temp_dir.path().join("hls");
        std::fs::create_dir_all(&hls_dir).unwrap();

        // Create 3 dummy .ts files
        std::fs::write(hls_dir.join("segment_001.ts"), "data").unwrap();
        std::fs::write(hls_dir.join("segment_002.ts"), "data").unwrap();
        std::fs::write(hls_dir.join("segment_003.ts"), "data").unwrap();

        let mut storage = MockStorage::new();
        storage
            .expect_create_hls_segment_upload_url()
            .returning(|_, _| Ok(dummy_url()));

        let mut file_transfer = MockFileTransfer::new();
        // Simulate a failure on the second file, success on others
        file_transfer.expect_upload().returning(|path, _, _| {
            if path.ends_with("segment_002.ts") {
                Err(FileTransferError::Io(std::io::Error::other(
                    "simulated network error",
                )))
            } else {
                Ok(())
            }
        });

        let processor = VideoProcessor::new(
            Arc::new(MockVideoRepository::new()),
            Arc::new(storage),
            Arc::new(MockMediaProbe::new()),
            Arc::new(MockMediaTranscoder::new()),
            Arc::new(file_transfer),
            dummy_worker_config(),
        );

        let result = processor.upload_segments(ulid, &hls_dir).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("simulated network error"));
    }
}
