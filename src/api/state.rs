//! Shared API state passed to handlers.

use std::sync::Arc;

use crate::{config::Config, r2_storage::R2Storage, video_repository::VideoRepository};

/// Immutable application state required by API handlers.
#[derive(Clone)]
pub struct AppState {
    pub video_repository: Arc<VideoRepository>,
    pub r2_storage: Arc<R2Storage>,
    pub config: Arc<Config>,
}

impl AppState {
    /// Build shared API state from initialized runtime services.
    pub fn new(
        video_repository: Arc<VideoRepository>,
        r2_storage: Arc<R2Storage>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            video_repository,
            r2_storage,
            config,
        }
    }
}
