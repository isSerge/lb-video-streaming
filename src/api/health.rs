//! Health endpoint handlers.

use axum::{Json, extract::State};
use serde::Serialize;

use super::state::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: &'static str,
    checks: HealthChecks,
}

#[derive(Debug, Serialize)]
struct HealthChecks {
    database: &'static str,
    r2_storage: &'static str,
}

/// Lightweight process health endpoint.
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let (db_res, r2_res) = tokio::join!(state.video_repository.ping(), state.storage.ping());

    let database = db_res.map_or("error", |_| "ok");
    let r2_storage = r2_res.map_or("error", |_| "ok");
    let status = if database == "ok" && r2_storage == "ok" {
        "ok"
    } else {
        "error"
    };

    Json(HealthResponse {
        status,
        checks: HealthChecks {
            database,
            r2_storage,
        },
    })
}
