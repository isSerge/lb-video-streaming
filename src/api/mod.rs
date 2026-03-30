//! API module composition and route wiring.

mod errors;
mod health;
mod state;
mod upload;
mod video;

use axum::{
    Router,
    routing::{get, post},
};

pub use state::AppState;

/// Build the top-level API router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/api/upload-url", post(upload::create_upload_url))
        .route(
            "/api/upload-complete/{ulid}",
            post(upload::mark_upload_complete),
        )
        .route("/api/video/{ulid}", get(video::get_video_metadata))
        .with_state(state)
}
