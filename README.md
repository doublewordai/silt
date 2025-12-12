# Silt

A transparent batching proxy for the OpenAI API that accumulates real-time
requests and dispatches at intervals using the OpenAI Batch API to achieve ~50%
cost savings.

Includes functionality to make it easy to handle long lived 'real-time'
requests - including request resumption via idempotency keys and TCP keepalives
to avoid connection drops.

## Features

- **Transparent Batching**: Standard OpenAI API interface - no custom client
code needed
- **Automatic Retry**: Idempotent requests with client-generated IDs enable
safe retries
- **Long-Lived Connections**: TCP keepalives and connection resumption for
multi-hour waits
- **Cost Optimization**: Leverages OpenAI's Batch API for 50% cost reduction

## Architecture

```
Client → Batch Proxy → OpenAI Batch API
   ↓                         ↓
Idempotency-Key         Batch File Upload
   ↓                         ↓
Redis State ← ← ← ← ← Batch Polling
```

1. Client sends request with `Idempotency-Key` header
2. Proxy queues request and holds connection open
3. Background worker accumulates requests for N seconds
4. Worker uploads batch file to OpenAI
5. Worker polls batch status every M seconds
6. When complete, results are returned to waiting clients
7. Disconnected clients can reconnect with same key to resume

## Prerequisites

- Rust 1.70+
- Redis
- OpenAI API key with Batch API access

## Setup

1. **Clone and build**:

```bash
cd silt
cargo build --release
```

2. **Configure environment**:

```bash
cp .env.example .env
# Edit .env with your settings
```

Required configuration:

- `OPENAI_API_KEY`: Your OpenAI API key
- `REDIS_URL`: Redis connection URL (default: `redis://127.0.0.1:6379`)

Optional configuration:

- `BATCH_WINDOW_SECS`: How long to accumulate requests (default: 60)
- `BATCH_POLL_INTERVAL_SECS`: Batch status polling interval (default: 60)
- `SERVER_HOST`: Server bind address (default: `0.0.0.0`)
- `SERVER_PORT`: Server port (default: `8080`)
- `TCP_KEEPALIVE_SECS`: TCP keepalive interval (default: 60)

3. **Start Redis** (if not already running):

```bash
redis-server
```

4. **Run the proxy**:

```bash
cargo run --release
```

## Usage

### Python Client

The proxy is designed to work with the standard OpenAI Python client with minimal modifications:

```python
import uuid
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8080/v1",
    api_key="dummy",  # Not validated by proxy
    timeout=3600,     # 1 hour timeout per attempt
)

# Generate unique ID for this request
request_id = str(uuid.uuid4())

# Make request with idempotency key
response = client.chat.completions.create(
    model="gpt-4",
    messages=[{"role": "user", "content": "Hello!"}],
    extra_headers={
        "Idempotency-Key": request_id
    }
)

print(response.choices[0].message.content)
```

### With Automatic Retries

For production use, implement retry logic to handle connection drops:

```python
import time
from openai import APIError, APITimeoutError

def batched_completion(messages, request_id, max_wait_hours=24):
    retry_delay = 30
    start_time = time.time()

    while time.time() - start_time < max_wait_hours * 3600:
        try:
            return client.chat.completions.create(
                model="gpt-4",
                messages=messages,
                extra_headers={"Idempotency-Key": request_id}
            )
        except (APITimeoutError, APIError) as e:
            print(f"Retrying in {retry_delay}s...")
            time.sleep(retry_delay)
            retry_delay = min(retry_delay * 1.5, 300)

    raise TimeoutError("Batch did not complete in time")
```

See `example_client.py` for a complete working example.

### Any HTTP Client

The proxy exposes a standard OpenAI-compatible endpoint:

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: $(uuidgen)" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

**Important**: The `Idempotency-Key` header is **required**. Requests without
it will receive a 400 error.

## How It Works

### Request Lifecycle

1. **Submission**: Client sends request with unique `Idempotency-Key`
2. **Queueing**: Proxy stores request in Redis with status `queued`
3. **Batching**: After `BATCH_WINDOW_SECS`, dispatcher collects all queued requests
4. **Upload**: Requests are formatted as JSONL and uploaded to OpenAI
5. **Dispatch**: Batch is submitted to OpenAI Batch API
6. **Processing**: Status changes to `processing`, worker polls every `BATCH_POLL_INTERVAL_SECS`
7. **Completion**: When batch completes, results are fetched and stored
8. **Response**: Waiting clients receive their individual responses

### Connection Handling

- **TCP Keepalive**: Configured at socket level to prevent connection drops
- **Pub/Sub**: Redis pub/sub notifies waiting connections when results arrive
- **Idempotency**: Same `Idempotency-Key` always returns same result
- **State Recovery**: If connection drops, client reconnects with same key

### Error Handling

- **Redis Failures**: Requests fail fast if state cannot be persisted
- **Client Disconnects**: Results are cached for 48 hours for later retrieval

## Limitations

- **Latency**: Batch processing can take hours
- **Streaming**: Batch API doesn't support streaming responses
- **Request Immutability**: Once submitted, requests cannot be cancelled

## Use Cases

Perfect for:

- Overnight document processing pipelines
- Bulk data analysis jobs
- Non-interactive content generation
- Cost-sensitive workloads where latency is acceptable

Not suitable for:

- Interactive chat applications
- Real-time completions
- Streaming responses
- Latency-sensitive workloads

## Development

Run tests (requires Redis):

```bash
cargo test
```

Run with debug logging:

```bash
RUST_LOG=debug cargo run
```

## License

MIT
