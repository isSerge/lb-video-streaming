//! Domain type for raw upload object keys in storage.

use std::ops::Deref;

use ulid::Ulid;

/// Object key for a raw uploaded video in storage.
#[derive(Debug)]
pub struct RawUploadKey(String);

impl Deref for RawUploadKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Ulid> for RawUploadKey {
    fn from(ulid: Ulid) -> Self {
        Self(format!("raw/{}/video", ulid))
    }
}
