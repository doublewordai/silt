use crate::models::{CompletionRequest, CompletionResponse, RequestState, RequestStatus};
use anyhow::Result;
use chrono::Utc;
use redis::AsyncCommands;

#[derive(Clone)]
pub struct StateManager {
    redis: redis::aio::ConnectionManager,
    client: redis::Client,
}

impl StateManager {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let redis = redis::aio::ConnectionManager::new(client.clone()).await?;
        Ok(Self { redis, client })
    }

    pub async fn get_request(&self, request_id: &str) -> Result<Option<RequestState>> {
        let mut conn = self.redis.clone();
        let key = format!("request:{}", request_id);
        let data: Option<String> = conn.get(&key).await?;

        match data {
            Some(json) => {
                let state: RequestState = serde_json::from_str(&json)?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    pub async fn create_request(
        &self,
        request_id: &str,
        request: CompletionRequest,
        api_key: String,
    ) -> Result<RequestState> {
        let mut conn = self.redis.clone();
        let state = RequestState::new(request_id.to_string(), request, api_key);

        let key = format!("request:{}", request_id);
        let json = serde_json::to_string(&state)?;

        // Set with 48 hour expiry
        conn.set_ex::<_, _, ()>(&key, json, 48 * 3600).await?;

        // Add to queued set
        conn.sadd::<_, _, ()>("queued_requests", request_id).await?;

        Ok(state)
    }

    pub async fn update_status(
        &self,
        request_id: &str,
        status: RequestStatus,
        batch_id: Option<String>,
    ) -> Result<()> {
        let mut conn = self.redis.clone();

        if let Some(mut state) = self.get_request(request_id).await? {
            state.status = status;
            state.batch_id = batch_id;
            state.updated_at = Utc::now();

            let key = format!("request:{}", request_id);
            let json = serde_json::to_string(&state)?;
            conn.set_ex::<_, _, ()>(&key, json, 48 * 3600).await?;
        }

        Ok(())
    }

    pub async fn complete_request(
        &self,
        request_id: &str,
        result: CompletionResponse,
    ) -> Result<()> {
        let mut conn = self.redis.clone();

        if let Some(mut state) = self.get_request(request_id).await? {
            state.status = RequestStatus::Complete;
            state.result = Some(result);
            state.updated_at = Utc::now();

            let key = format!("request:{}", request_id);
            let json = serde_json::to_string(&state)?;
            // Keep completed requests for 48 hours
            conn.set_ex::<_, _, ()>(&key, json, 48 * 3600).await?;

            // Publish completion event
            let channel = format!("completion:{}", request_id);
            conn.publish::<_, _, ()>(&channel, "complete").await?;
        }

        Ok(())
    }

    pub async fn fail_request(
        &self,
        request_id: &str,
        error: String,
    ) -> Result<()> {
        let mut conn = self.redis.clone();

        if let Some(mut state) = self.get_request(request_id).await? {
            state.status = RequestStatus::Failed;
            state.error = Some(error.clone());
            state.updated_at = Utc::now();

            let key = format!("request:{}", request_id);
            let json = serde_json::to_string(&state)?;
            conn.set_ex::<_, _, ()>(&key, json, 48 * 3600).await?;

            // Publish completion event (even for failures)
            let channel = format!("completion:{}", request_id);
            conn.publish::<_, _, ()>(&channel, &error).await?;
        }

        Ok(())
    }

    pub async fn get_queued_requests(&self) -> Result<Vec<String>> {
        let mut conn = self.redis.clone();
        let request_ids: Vec<String> = conn.smembers("queued_requests").await?;
        Ok(request_ids)
    }

    pub async fn move_to_batching(
        &self,
        request_ids: &[String],
        batch_id: &str,
        api_key: &str,
    ) -> Result<()> {
        let mut conn = self.redis.clone();

        // Remove from queued set
        for request_id in request_ids {
            conn.srem::<_, _, ()>("queued_requests", request_id).await?;
            self.update_status(
                request_id,
                RequestStatus::Batching,
                Some(batch_id.to_string()),
            ).await?;
        }

        // Store batch -> request mapping
        let batch_key = format!("batch:{}", batch_id);
        let request_ids_json = serde_json::to_string(request_ids)?;
        conn.set_ex::<_, _, ()>(&batch_key, request_ids_json, 48 * 3600).await?;

        // Store batch -> API key mapping
        let batch_api_key = format!("batch_api_key:{}", batch_id);
        conn.set_ex::<_, _, ()>(&batch_api_key, api_key, 48 * 3600).await?;

        // Add to processing batches set
        conn.sadd::<_, _, ()>("processing_batches", batch_id).await?;

        Ok(())
    }

    pub async fn get_batch_api_key(&self, batch_id: &str) -> Result<Option<String>> {
        let mut conn = self.redis.clone();
        let key = format!("batch_api_key:{}", batch_id);
        let api_key: Option<String> = conn.get(&key).await?;
        Ok(api_key)
    }

    pub async fn get_batch_requests(&self, batch_id: &str) -> Result<Vec<String>> {
        let mut conn = self.redis.clone();
        let batch_key = format!("batch:{}", batch_id);
        let data: Option<String> = conn.get(&batch_key).await?;

        match data {
            Some(json) => {
                let request_ids: Vec<String> = serde_json::from_str(&json)?;
                Ok(request_ids)
            }
            None => Ok(vec![]),
        }
    }

    pub async fn get_processing_batches(&self) -> Result<Vec<String>> {
        let mut conn = self.redis.clone();
        let batch_ids: Vec<String> = conn.smembers("processing_batches").await?;
        Ok(batch_ids)
    }

    pub async fn remove_processing_batch(&self, batch_id: &str) -> Result<()> {
        let mut conn = self.redis.clone();
        conn.srem::<_, _, ()>("processing_batches", batch_id).await?;
        Ok(())
    }

    pub async fn subscribe_to_completion(&self, request_id: &str) -> Result<redis::aio::PubSub> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        let channel = format!("completion:{}", request_id);
        pubsub.subscribe(&channel).await?;
        Ok(pubsub)
    }
}
