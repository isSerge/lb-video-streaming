use axum::{extract::Json, routing::get, Router};
use serde::Serialize;

// TODO: add comment on necessary production routes (metrics, liveness/readiness, etc.)

pub fn router() -> Router {
    Router::new().route("/health", get(health))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    checks: HealthChecks,
}

#[derive(Debug, Serialize)]
struct HealthChecks {
    // TODO: wire real DB ping and R2 check once state is injected into router.
    database: &'static str,
    storage: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        checks: HealthChecks {
            database: "todo",
            storage: "todo",
        },
    })
}
