use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::store::{Store, StoredNarrative};

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
}

pub async fn serve(store: Arc<Store>, port: u16) -> Result<()> {
    let state = AppState { store };

    let app = Router::new()
        .route("/", get(dashboard))
        .route("/api/recent", get(api_recent))
        .route("/api/stats", get(api_stats))
        .route("/api/narratives", get(api_narratives))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Web dashboard at http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

#[derive(Deserialize)]
struct RecentParams {
    limit: Option<i64>,
}

async fn api_recent(
    State(state): State<AppState>,
    Query(params): Query<RecentParams>,
) -> Response {
    let limit = params.limit.unwrap_or(100).min(500);
    match state.store.recent(limit).await {
        Ok(results) => Json(results).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("DB error: {}", e),
        )
            .into_response(),
    }
}

async fn api_stats(State(state): State<AppState>) -> Response {
    match state.store.stats().await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("DB error: {}", e),
        )
            .into_response(),
    }
}

async fn api_narratives(
    State(state): State<AppState>,
    Query(params): Query<RecentParams>,
) -> Response {
    let limit = params.limit.unwrap_or(50).min(200);
    match state.store.all_recent_narratives(limit).await {
        Ok(results) => Json::<Vec<StoredNarrative>>(results).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)).into_response(),
    }
}
