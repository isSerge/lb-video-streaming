//! Domain value objects and validation types.

mod api_path;
mod media_keys;
mod media_metadata;
mod raw_upload_key;
mod upload_content_type;
mod upload_size;
mod video_status;

pub use api_path::{UploadCompletePath, VideoMetadataPath};
pub use media_keys::{HLSKey, ManifestKey, TransmuxKey};
pub use media_metadata::{
    AudioCodec, ContainerFormat, FormatCompatibility, MediaMetadata, VideoCodec,
};
pub use raw_upload_key::RawUploadKey;
pub use upload_content_type::{UploadContentType, UploadContentTypeError};
pub use upload_size::{MaxUploadBytes, UploadSizeBytes, UploadSizeError};
pub use video_status::{VideoStatus, VideoStatusError};
