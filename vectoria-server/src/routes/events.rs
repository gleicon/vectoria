use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use vectoria_core::model::{Event, EventType};
use serde::Deserialize;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct EventRequest {
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub product_id: String,
    pub user_id: Option<String>,
    pub query: Option<String>,
    pub session_id: Option<String>,
}

/// Fire-and-forget event ingestion. Returns 202 immediately.
pub async fn record_event(
    State(state): State<AppState>,
    Json(req): Json<EventRequest>,
) -> impl IntoResponse {
    let mut event = Event::new(req.event_type, req.product_id);
    event.user_id = req.user_id;
    event.query = req.query;
    event.session_id = req.session_id;

    // Spawn async to keep response fast.
    let engine = state.engine.clone();
    tokio::spawn(async move {
        if let Err(e) = engine.record_event(event).await {
            tracing::warn!("failed to record event: {}", e);
        }
    });

    StatusCode::ACCEPTED
}
