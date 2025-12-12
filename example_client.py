#!/usr/bin/env python3
"""
Example client for the OpenAI Batch Proxy.

This demonstrates how to use the batch proxy with automatic retries
and long timeouts to transparently handle batch processing.
"""

import uuid
import time
from openai import OpenAI, APIError, APITimeoutError
import sys


def batched_completion(
    messages,
    model="gpt-4",
    base_url="http://localhost:8080/v1",
    max_wait_hours=24,
    timeout_per_attempt=3600,  # 1 hour per attempt
):
    """
    Make a completion request through the batch proxy.

    Args:
        messages: List of message dicts with 'role' and 'content'
        model: Model name to use
        base_url: Base URL of the batch proxy
        max_wait_hours: Maximum time to wait for completion (hours)
        timeout_per_attempt: Timeout for each HTTP attempt (seconds)

    Returns:
        OpenAI completion response
    """
    client = OpenAI(
        base_url=base_url,
        api_key="dummy",  # Not needed for the proxy, but required by the SDK
        timeout=timeout_per_attempt,
        max_retries=0,  # We handle retries manually
    )

    # Generate a unique request ID
    request_id = str(uuid.uuid4())
    print(f"Request ID: {request_id}")

    start_time = time.time()
    retry_delay = 30  # Start with 30 seconds
    attempt = 0

    while time.time() - start_time < max_wait_hours * 3600:
        attempt += 1
        elapsed = time.time() - start_time
        print(f"\nAttempt {attempt} (elapsed: {elapsed:.1f}s)")

        try:
            response = client.chat.completions.create(
                model=model,
                messages=messages,
                extra_headers={
                    "Idempotency-Key": request_id
                }
            )

            print("✓ Request completed successfully!")
            return response

        except APITimeoutError as e:
            print(f"⏱ Connection timeout, retrying in {retry_delay}s...")
            time.sleep(retry_delay)
            retry_delay = min(retry_delay * 1.5, 300)  # Max 5 minutes

        except APIError as e:
            if e.status_code >= 500:
                # Server error, retry
                print(f"⚠ Server error ({e.status_code}), retrying in {retry_delay}s...")
                time.sleep(retry_delay)
                retry_delay = min(retry_delay * 1.5, 300)
            else:
                # Client error, don't retry
                print(f"✗ Client error: {e}")
                raise

        except Exception as e:
            print(f"⚠ Unexpected error: {e}, retrying in {retry_delay}s...")
            time.sleep(retry_delay)
            retry_delay = min(retry_delay * 1.5, 300)

    raise TimeoutError(f"Batch did not complete within {max_wait_hours} hours")


def main():
    """Example usage"""
    print("OpenAI Batch Proxy - Example Client")
    print("=" * 50)

    messages = [
        {
            "role": "user",
            "content": "Write a short poem about batch processing."
        }
    ]

    print("\nSending request to batch proxy...")
    print("This may take a while (potentially hours) depending on batch processing.")
    print("The connection will automatically retry if it drops.\n")

    try:
        response = batched_completion(
            messages=messages,
            model="gpt-4",
            max_wait_hours=24,
        )

        print("\n" + "=" * 50)
        print("Response:")
        print("=" * 50)
        print(response.choices[0].message.content)
        print("\n" + "=" * 50)
        print(f"Model: {response.model}")
        print(f"Tokens: {response.usage.total_tokens}")

    except KeyboardInterrupt:
        print("\n\n⚠ Interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n\n✗ Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
