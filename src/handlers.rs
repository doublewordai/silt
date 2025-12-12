use crate::models::{CompletionRequest, RequestStatus};
use crate::state::StateManager;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::stream::StreamExt;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub state_manager: StateManager,
}

pub async fn health_check() -> &'static str {
    "OK"
}

pub async fn create_chat_completion(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CompletionRequest>,
) -> Result<Response, ApiError> {
    // Extract or generate idempotency key
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let generated_key = Uuid::new_v4().to_string();
            info!("No idempotency key provided, generated: {}", generated_key);
            generated_key
        });

    // Extract API key from Authorization header (required)
    let api_key = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::MissingApiKey)?
        .to_string();

    info!("Received request with idempotency key: {}", idempotency_key);

    // Check if request already exists
    let existing_state = app_state.state_manager.get_request(&idempotency_key).await
        .map_err(|e| ApiError::InternalError(e.to_string()))?;

    match existing_state {
        Some(state) if state.status == RequestStatus::Complete => {
            // Already completed - return cached result
            info!("Returning cached result for: {}", idempotency_key);
            if let Some(result) = state.result {
                return Ok(Json(result).into_response());
            } else {
                return Err(ApiError::InternalError("No result found for completed request".to_string()));
            }
        }
        Some(state) if state.status == RequestStatus::Failed => {
            // Previously failed
            let error_msg = state.error.unwrap_or_else(|| "Unknown error".to_string());
            error!("Request failed previously: {}", error_msg);
            return Err(ApiError::BatchFailed(error_msg));
        }
        Some(_) => {
            // In progress - wait for completion
            info!("Request already in progress, waiting: {}", idempotency_key);
        }
        None => {
            // New request - create it
            info!("Creating new request: {}", idempotency_key);
            app_state.state_manager
                .create_request(&idempotency_key, request, api_key)
                .await
                .map_err(|e| ApiError::InternalError(e.to_string()))?;
        }
    }

    // Wait for completion
    wait_for_completion(&app_state.state_manager, &idempotency_key).await
}

async fn wait_for_completion(
    state_manager: &StateManager,
    request_id: &str,
) -> Result<Response, ApiError> {
    // Subscribe to completion events
    let mut pubsub = state_manager
        .subscribe_to_completion(request_id)
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?;

    // Wait for completion with periodic checks
    loop {
        // Try to get message with timeout
        let result = timeout(Duration::from_secs(30), async {
            let mut stream = pubsub.on_message();
            stream.next().await
        })
        .await;

        match result {
            Ok(Some(_msg)) => {
                // Completion event received, fetch the result
                if let Some(state) = state_manager.get_request(request_id).await
                    .map_err(|e| ApiError::InternalError(e.to_string()))? {
                    match state.status {
                        RequestStatus::Complete => {
                            if let Some(result) = state.result {
                                info!("Request completed: {}", request_id);
                                return Ok(Json(result).into_response());
                            }
                        }
                        RequestStatus::Failed => {
                            let error_msg = state.error.unwrap_or_else(|| "Unknown error".to_string());
                            error!("Request failed: {}", error_msg);
                            return Err(ApiError::BatchFailed(error_msg));
                        }
                        _ => {
                            // Still processing, continue waiting
                            continue;
                        }
                    }
                }
            }
            Ok(None) => {
                warn!("PubSub stream ended unexpectedly");
                // Reconnect and continue
                pubsub = state_manager
                    .subscribe_to_completion(request_id)
                    .await
                    .map_err(|e| ApiError::InternalError(e.to_string()))?;
            }
            Err(_) => {
                // Timeout - check status directly
                if let Some(state) = state_manager.get_request(request_id).await
                    .map_err(|e| ApiError::InternalError(e.to_string()))? {
                    match state.status {
                        RequestStatus::Complete => {
                            if let Some(result) = state.result {
                                info!("Request completed (via poll): {}", request_id);
                                return Ok(Json(result).into_response());
                            }
                        }
                        RequestStatus::Failed => {
                            let error_msg = state.error.unwrap_or_else(|| "Unknown error".to_string());
                            error!("Request failed (via poll): {}", error_msg);
                            return Err(ApiError::BatchFailed(error_msg));
                        }
                        _ => {
                            // Still processing, continue waiting
                            continue;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum ApiError {
    MissingApiKey,
    InternalError(String),
    BatchFailed(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::MissingApiKey => (
                StatusCode::UNAUTHORIZED,
                "Authorization header with Bearer token is required".to_string(),
            ),
            ApiError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::BatchFailed(msg) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Batch processing failed: {}", msg)),
        };

        let body = serde_json::json!({
            "error": {
                "message": message,
                "type": "api_error",
            }
        });

        (status, Json(body)).into_response()
    }
}
