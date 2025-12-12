use crate::models::{
    BatchLine, BatchRequest, BatchResponse, BatchResultLine, CompletionRequest,
    CompletionResponse, FileUploadResponse,
};
use anyhow::{anyhow, Result};
use reqwest::Client;
use std::collections::HashMap;

pub struct OpenAIClient {
    client: Client,
    base_url: String,
}

impl OpenAIClient {
    pub fn new(base_url: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        Self {
            client,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        }
    }

    pub async fn upload_batch_file(
        &self,
        api_key: &str,
        requests: Vec<(String, CompletionRequest)>,
    ) -> Result<String> {
        let num_requests = requests.len();

        // Create JSONL content
        let mut lines = Vec::new();
        for (request_id, request) in requests {
            let batch_line = BatchLine {
                custom_id: request_id,
                method: "POST".to_string(),
                url: "/v1/chat/completions".to_string(),
                body: request,
            };
            lines.push(serde_json::to_string(&batch_line)?);
        }
        let content = lines.join("\n");

        tracing::info!("Uploading batch file with {} requests ({} bytes)", num_requests, content.len());

        // Generate unique filename
        let filename = format!("batch_{}.jsonl", uuid::Uuid::new_v4());

        // Upload file
        let form = reqwest::multipart::Form::new()
            .text("purpose", "batch")
            .part(
                "file",
                reqwest::multipart::Part::bytes(content.into_bytes())
                    .file_name(filename)
                    .mime_str("application/jsonl")?,
            );

        let url = format!("{}/files", self.base_url);
        tracing::debug!("POST {}", url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send file upload request: {}", e))?;

        let status = response.status();
        tracing::debug!("Upload response status: {}", status);

        if !status.is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to upload file ({}): {}", status, error_text));
        }

        let upload_response: FileUploadResponse = response.json().await?;
        tracing::info!("File uploaded: {}", upload_response.id);
        Ok(upload_response.id)
    }

    pub async fn create_batch(&self, api_key: &str, input_file_id: String) -> Result<BatchResponse> {
        let batch_request = BatchRequest {
            input_file_id: input_file_id.clone(),
            endpoint: "/v1/chat/completions".to_string(),
            completion_window: "24h".to_string(),
            metadata: None,
        };

        tracing::info!("Creating batch for file: {}", input_file_id);

        let url = format!("{}/batches", self.base_url);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&batch_request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send batch creation request: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to create batch ({}): {}", status, error_text));
        }

        let batch_response: BatchResponse = response.json().await?;
        tracing::info!("Batch created: {} (status: {})", batch_response.id, batch_response.status);
        Ok(batch_response)
    }

    pub async fn get_batch_status(&self, api_key: &str, batch_id: &str) -> Result<BatchResponse> {
        let response = self
            .client
            .get(&format!("{}/batches/{}", self.base_url, batch_id))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to get batch status: {}", error_text));
        }

        let batch_response: BatchResponse = response.json().await?;
        Ok(batch_response)
    }

    pub async fn retrieve_batch_results(
        &self,
        api_key: &str,
        output_file_id: &str,
    ) -> Result<HashMap<String, CompletionResponse>> {
        let response = self
            .client
            .get(&format!("{}/files/{}/content", self.base_url, output_file_id))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to retrieve results: {}", error_text));
        }

        let content = response.text().await?;
        let mut results = HashMap::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let result_line: BatchResultLine = serde_json::from_str(line)?;
            results.insert(result_line.custom_id, result_line.response.body);
        }

        Ok(results)
    }
}
