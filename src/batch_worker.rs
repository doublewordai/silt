use crate::config::Config;
use crate::models::{CompletionRequest, RequestStatus};
use crate::openai_client::OpenAIClient;
use crate::state::StateManager;
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

pub struct BatchWorker {
    config: Arc<Config>,
    state: StateManager,
    openai_client: OpenAIClient,
}

impl BatchWorker {
    pub fn new(config: Arc<Config>, state: StateManager) -> Self {
        let openai_client = OpenAIClient::new(config.upstream_base_url.clone());
        Self {
            config,
            state,
            openai_client,
        }
    }

    pub async fn start_dispatcher(&self) {
        let mut ticker = interval(Duration::from_secs(self.config.batch_window_secs));

        loop {
            ticker.tick().await;

            if let Err(e) = self.dispatch_batch().await {
                error!("Error dispatching batch: {}", e);
            }
        }
    }

    async fn dispatch_batch(&self) -> Result<()> {
        // Get all queued requests
        let request_ids = self.state.get_queued_requests().await?;

        if request_ids.is_empty() {
            info!("No requests queued for batching");
            return Ok(());
        }

        info!("Dispatching batches for {} queued requests", request_ids.len());

        // Gather requests and group by API key
        let mut requests_by_key: std::collections::HashMap<String, Vec<(String, CompletionRequest)>> =
            std::collections::HashMap::new();
        let mut request_id_to_key: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for request_id in &request_ids {
            if let Some(state) = self.state.get_request(request_id).await? {
                let api_key = state.api_key.clone();
                requests_by_key
                    .entry(api_key.clone())
                    .or_insert_with(Vec::new)
                    .push((request_id.clone(), state.request));
                request_id_to_key.insert(request_id.clone(), api_key);
            }
        }

        if requests_by_key.is_empty() {
            warn!("No valid requests found in queue");
            return Ok(());
        }

        info!("Creating {} batch(es) grouped by API key", requests_by_key.len());

        // Process each API key's batch
        for (api_key, requests) in requests_by_key {
            let batch_request_ids: Vec<String> = requests.iter().map(|(id, _)| id.clone()).collect();
            self.dispatch_batch_for_key(api_key, requests, batch_request_ids).await?;
        }

        Ok(())
    }

    async fn dispatch_batch_for_key(
        &self,
        api_key: String,
        requests: Vec<(String, CompletionRequest)>,
        request_ids: Vec<String>,
    ) -> Result<()> {
        info!("Dispatching batch with {} requests for API key", requests.len());

        // Upload batch file - don't fail requests on transient errors, let them retry
        let file_id = match self.openai_client.upload_batch_file(&api_key, requests).await {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to upload batch file (will retry next window): {}", e);
                // Leave requests in queue for retry
                return Ok(());
            }
        };

        info!("Uploaded batch file: {}", file_id);

        // Create batch - don't fail requests on transient errors, let them retry
        let batch = match self.openai_client.create_batch(&api_key, file_id).await {
            Ok(batch) => batch,
            Err(e) => {
                error!("Failed to create batch (will retry next window): {}", e);
                // Leave requests in queue for retry
                return Ok(());
            }
        };

        info!("Created batch: {}", batch.id);

        // Update state
        self.state
            .move_to_batching(&request_ids, &batch.id, &api_key)
            .await?;

        // Start polling for this batch
        let worker = self.clone();
        let batch_id = batch.id.clone();
        tokio::spawn(async move {
            if let Err(e) = worker.poll_batch(&batch_id).await {
                error!("Error polling batch {}: {}", batch_id, e);
            }
        });

        Ok(())
    }

    async fn poll_batch(&self, batch_id: &str) -> Result<()> {
        info!("Starting to poll batch: {}", batch_id);

        // Get API key for this batch
        let api_key = match self.state.get_batch_api_key(batch_id).await? {
            Some(key) => key,
            None => {
                error!("No API key found for batch {}", batch_id);
                return Err(anyhow::anyhow!("No API key found for batch"));
            }
        };

        let mut ticker = interval(Duration::from_secs(self.config.batch_poll_interval_secs));

        loop {
            ticker.tick().await;

            // Try to get batch status, but don't fail the whole polling loop on transient errors
            let batch = match self.openai_client.get_batch_status(&api_key, batch_id).await {
                Ok(b) => b,
                Err(e) => {
                    warn!("Failed to get batch status for {}, will retry: {}", batch_id, e);
                    continue;  // Retry on next tick
                }
            };

            info!("Batch {} status: {}", batch_id, batch.status);

            // Update request statuses to processing
            let request_ids = self.state.get_batch_requests(batch_id).await?;
            for request_id in &request_ids {
                if let Some(state) = self.state.get_request(request_id).await? {
                    if state.status == RequestStatus::Batching {
                        self.state
                            .update_status(request_id, RequestStatus::Processing, Some(batch_id.to_string()))
                            .await?;
                    }
                }
            }

            match batch.status.as_str() {
                "completed" => {
                    info!("Batch {} completed!", batch_id);
                    if let Some(output_file_id) = batch.output_file_id {
                        self.process_batch_results(&api_key, batch_id, &output_file_id).await?;
                    } else {
                        warn!("Batch completed but no output file");
                    }
                    self.state.remove_processing_batch(batch_id).await?;
                    break;
                }
                "failed" | "expired" | "cancelled" => {
                    error!("Batch {} failed with status: {}", batch_id, batch.status);
                    // Mark all requests as failed
                    let request_ids = self.state.get_batch_requests(batch_id).await?;
                    for request_id in request_ids {
                        self.state
                            .fail_request(&request_id, format!("Batch {}", batch.status))
                            .await?;
                    }
                    self.state.remove_processing_batch(batch_id).await?;
                    break;
                }
                _ => {
                    // Still processing
                    continue;
                }
            }
        }

        Ok(())
    }

    async fn process_batch_results(&self, api_key: &str, batch_id: &str, output_file_id: &str) -> Result<()> {
        info!("Processing results for batch: {}", batch_id);

        let results = self
            .openai_client
            .retrieve_batch_results(api_key, output_file_id)
            .await?;

        info!("Retrieved {} results", results.len());

        for (request_id, response) in results {
            self.state.complete_request(&request_id, response).await?;
        }

        Ok(())
    }

    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            state: self.state.clone(),
            openai_client: OpenAIClient::new(self.config.upstream_base_url.clone()),
        }
    }

    pub async fn start_poller(&self) {
        // Poll existing batches on startup
        if let Ok(batch_ids) = self.state.get_processing_batches().await {
            for batch_id in batch_ids {
                let worker = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = worker.poll_batch(&batch_id).await {
                        error!("Error polling batch {}: {}", batch_id, e);
                    }
                });
            }
        }
    }
}
