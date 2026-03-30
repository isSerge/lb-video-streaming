//! Health endpoint handlers.

use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: &'static str,
    checks: HealthChecks,
}

#[derive(Debug, Serialize)]
struct HealthChecks {
    // TODO: wire real DB ping and R2 check once state is injected into router.
    database: &'static str,
    r2_storage: &'static str,
}

/// Lightweight process health endpoint.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        checks: HealthChecks {
            database: "todo",
            r2_storage: "todo",
        },
    })
}
