//! API module composition and route wiring.

mod errors;
mod health;
mod state;
mod upload;
mod video;

#[cfg(test)]
mod tests;

use axum::{
    Router,
    http::{HeaderValue, Method, header},
    routing::{get, post},
};
use tower_http::cors::CorsLayer;

pub use state::AppState;

/// Build the top-level API router.
pub fn router(state: AppState) -> Router {
    let ui_origin = HeaderValue::from_str(&state.config.ui_origin)
        .expect("UI_ORIGIN must be a valid HTTP header value");

    let cors = CorsLayer::new()
        .allow_origin(ui_origin)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE]);

    Router::new()
        .route("/health", get(health::health))
        .route("/api/upload-url", post(upload::create_upload_url))
        .route(
            "/api/upload-complete/{ulid}",
            post(upload::mark_upload_complete),
        )
        .route("/api/video/{ulid}", get(video::get_video_metadata))
        .layer(cors)
        .with_state(state)
}
