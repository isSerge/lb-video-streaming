//! Shared API state passed to handlers.

use std::sync::Arc;

use crate::{
    config::Config, media_probe::MediaProbe, repository::VideoRepository, storage::Storage,
};

/// Immutable application state required by API handlers.
#[derive(Clone)]
pub struct AppState {
    pub video_repository: Arc<dyn VideoRepository>,
    pub storage: Arc<dyn Storage>,
    pub media_probe: Arc<dyn MediaProbe>,
    pub config: Arc<Config>,
}

impl AppState {
    /// Build shared API state from initialized runtime services.
    pub fn new(
        video_repository: Arc<dyn VideoRepository>,
        storage: Arc<dyn Storage>,
        media_probe: Arc<dyn MediaProbe>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            video_repository,
            storage,
            media_probe,
            config,
        }
    }
}
