use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub frequency_penalty: Option<f32>,
    #[serde(default)]
    pub presence_penalty: Option<f32>,
    #[serde(default)]
    pub stop: Option<Vec<String>>,
    #[serde(default)]
    pub n: Option<u32>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Queued,
    Batching,
    Processing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestState {
    pub request_id: String,
    pub status: RequestStatus,
    pub batch_id: Option<String>,
    pub request: CompletionRequest,
    pub api_key: String,
    pub result: Option<CompletionResponse>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RequestState {
    pub fn new(request_id: String, request: CompletionRequest, api_key: String) -> Self {
        let now = Utc::now();
        Self {
            request_id,
            status: RequestStatus::Queued,
            batch_id: None,
            request,
            api_key,
            result: None,
            error: None,
            created_at: now,
            updated_at: now,
        }
    }
}

// OpenAI Batch API structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    pub input_file_id: String,
    pub endpoint: String,
    pub completion_window: String,
    #[serde(default)]
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    pub id: String,
    pub object: String,
    pub endpoint: String,
    pub input_file_id: String,
    pub output_file_id: Option<String>,
    pub error_file_id: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchLine {
    pub custom_id: String,
    pub method: String,
    pub url: String,
    pub body: CompletionRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResultLine {
    pub id: String,
    pub custom_id: String,
    pub response: BatchResultResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResultResponse {
    pub status_code: u16,
    pub body: CompletionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUploadResponse {
    pub id: String,
    pub object: String,
    pub bytes: u64,
    pub created_at: i64,
    pub filename: String,
    pub purpose: String,
}
