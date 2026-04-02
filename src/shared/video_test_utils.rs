use crate::domain::{ManifestKey, RawUploadKey, TransmuxKey, VideoStatus};
use crate::repository::VideoRecord;
use ulid::Ulid;

/// Builder pattern for constructing VideoRecord instances in tests
pub struct VideoRecordBuilder {
    ulid: Ulid,
    status: VideoStatus,
    raw_key: RawUploadKey,
    transmux_key: Option<TransmuxKey>,
    manifest_key: Option<ManifestKey>,
    browser_compatible: bool,
    transmux_required: bool,
    transcode_required: bool,
}

#[allow(dead_code)]
impl VideoRecordBuilder {
    pub fn new(ulid: Ulid) -> Self {
        Self {
            ulid,
            status: VideoStatus::PendingUpload,
            raw_key: RawUploadKey::from(ulid),
            transmux_key: None,
            manifest_key: None,
            browser_compatible: false,
            transmux_required: false,
            transcode_required: true,
        }
    }

    pub fn status(mut self, status: VideoStatus) -> Self {
        self.status = status;
        self
    }

    pub fn raw_key(mut self, raw_key: RawUploadKey) -> Self {
        self.raw_key = raw_key;
        self
    }

    pub fn transmux_key(mut self, transmux_key: Option<TransmuxKey>) -> Self {
        self.transmux_key = transmux_key;
        self
    }

    pub fn manifest_key(mut self, manifest_key: Option<ManifestKey>) -> Self {
        self.manifest_key = manifest_key;
        self
    }

    pub fn browser_compatible(mut self, compatible: bool) -> Self {
        self.browser_compatible = compatible;
        self
    }

    pub fn transmux_required(mut self, required: bool) -> Self {
        self.transmux_required = required;
        self
    }

    pub fn transcode_required(mut self, required: bool) -> Self {
        self.transcode_required = required;
        self
    }

    pub fn build(self) -> VideoRecord {
        VideoRecord {
            ulid: self.ulid,
            status: self.status,
            raw_key: self.raw_key,
            transmux_key: self.transmux_key,
            manifest_key: self.manifest_key,
            browser_compatible: self.browser_compatible,
            transmux_required: self.transmux_required,
            transcode_required: self.transcode_required,
        }
    }
}
