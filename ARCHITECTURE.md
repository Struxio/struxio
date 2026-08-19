# Struxio Architecture

A self-hostable document extraction API. You upload documents, define JSON schemas for what you want to extract, and Gemini AI does the rest — synchronously or asynchronously via a job queue.

---

## Crate Structure

```
struxio/
├── crates/
│   ├── common/     ← shared models, AppError, Config
│   ├── db/         ← SQLx repository layer (raw DB queries)
│   ├── core/       ← business logic, services, queue traits, storage, Gemini
│   ├── api/        ← Axum HTTP server, routes, auth middleware
│   └── worker/     ← background job processor binary
├── migrations/     ← Postgres schema
└── docker-compose.yml
```

### Dependency flow

```
common → db → core → api
                   ↘ worker
```

`api` and `worker` are the only binaries. Everything else is a library crate.

---

## Schema

| Table | Purpose |
|---|---|
| `documents` | Uploaded files (stored in S3/MinIO) |
| `extraction_templates` | Reusable JSON schemas + Gemini prompt templates |
| `extractions` | Individual extraction jobs with status and result |
| `batch_jobs` | A batch run that fans out to many extractions |
| `ai_models` | AI model registry with pricing metadata |

---

## Full Request Flow by Endpoint

### 1 — Document Upload (two-step)

Documents are never uploaded through the API server. Instead, the client gets a pre-signed S3 URL and uploads directly to storage.

```
POST /v1/documents/check
POST /v1/documents/confirm
```

```mermaid
sequenceDiagram
    participant C as Client
    participant API as struxio-api
    participant DB as Postgres
    participant S3 as S3/MinIO

    C->>API: POST /v1/documents/check { md5_hash, file_name, file_type, size_bytes }
    API->>DB: DocumentRepo::find_by_hash(md5_hash)
    alt document already exists
        DB-->>API: Document
        API-->>C: 200 { exists: true, document: {...} }
    else new document
        API->>S3: generate_presigned_upload_url(key, content_type, 1h)
        S3-->>API: presigned URL
        API-->>C: 200 { exists: false, upload_url: "https://...", s3_key: "uuid/filename.pdf" }
        C->>S3: PUT <upload_url>  (client uploads directly)
        C->>API: POST /v1/documents/confirm { s3_key, md5_hash, file_name, file_type, size_bytes }
        API->>DB: DocumentRepo::create(...)
        DB-->>API: Document
        API-->>C: 200 Document
    end
```

**How to call:**
```bash
# Step 1 — check / get upload URL
curl -X POST http://localhost:8080/v1/documents/check \
  -H "Authorization: Bearer $STRUXIO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"md5_hash":"abc123","file_name":"invoice.pdf","file_type":"pdf","size_bytes":204800}'

# Step 2 — upload directly to S3 using the returned upload_url
curl -X PUT "<upload_url>" \
  -H "Content-Type: application/pdf" \
  --data-binary @invoice.pdf

# Step 3 — confirm the upload
curl -X POST http://localhost:8080/v1/documents/confirm \
  -H "Authorization: Bearer $STRUXIO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"s3_key":"<s3_key>","md5_hash":"abc123","file_name":"invoice.pdf","file_type":"pdf","size_bytes":204800}'
```

---

### 2 — Templates

Templates define **what** to extract: a Gemini prompt + a JSON schema for the response shape.

```
GET    /v1/templates
POST   /v1/templates
GET    /v1/templates/{id}
PUT    /v1/templates/{id}
DELETE /v1/templates/{id}
```

```mermaid
sequenceDiagram
    participant C as Client
    participant API as struxio-api
    participant DB as Postgres

    C->>API: POST /v1/templates { name, json_schema, prompt_template }
    API->>API: validate json_schema has "type" + "properties"
    API->>DB: TemplateRepo::create(name, json_schema, prompt_template, is_system=false)
    DB-->>API: ExtractionTemplate
    API-->>C: 201 ExtractionTemplate
```

**How to call:**
```bash
# Create a template
curl -X POST http://localhost:8080/v1/templates \
  -H "Authorization: Bearer $STRUXIO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Invoice",
    "description": "Extract invoice data",
    "prompt_template": "Extract all invoice data from this document.",
    "json_schema": {
      "type": "object",
      "properties": {
        "vendor_name": { "type": "string" },
        "total": { "type": "number" },
        "invoice_number": { "type": "string" }
      }
    }
  }'

# List all templates (includes system templates)
curl http://localhost:8080/v1/templates \
  -H "Authorization: Bearer $STRUXIO_API_KEY"

# Get / update / delete by ID
curl http://localhost:8080/v1/templates/<id> \
  -H "Authorization: Bearer $STRUXIO_API_KEY"
```

---

### 3 — Single Extraction (synchronous)

Runs extraction immediately, calls Gemini, and returns the result in the response.

```
POST /v1/extractions
GET  /v1/extractions
GET  /v1/extractions/{id}
```

```mermaid
sequenceDiagram
    participant C as Client
    participant API as struxio-api
    participant DB as Postgres
    participant S3 as S3/MinIO
    participant G as Gemini API

    C->>API: POST /v1/extractions { document_id, template_id }
    API->>DB: DocumentRepo::find_by_id(document_id)
    API->>DB: TemplateRepo::find_by_id(template_id)
    API->>DB: ExtractionRepo::create_with_status(status="processing")
    API->>S3: StorageClient::download(s3_key)
    S3-->>API: file bytes
    API->>G: GeminiClient::extract(bytes, mime_type, prompt, schema)
    G-->>API: { result, input_tokens, output_tokens }
    API->>DB: ExtractionRepo::update_completed(result, tokens, model_id)
    DB-->>API: Extraction
    API-->>C: 200 Extraction { status: "completed", result: {...} }
```

**How to call:**
```bash
# Run a synchronous extraction
curl -X POST http://localhost:8080/v1/extractions \
  -H "Authorization: Bearer $STRUXIO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"document_id":"<doc-uuid>","template_id":"<template-uuid>"}'

# List all extractions
curl http://localhost:8080/v1/extractions \
  -H "Authorization: Bearer $STRUXIO_API_KEY"

# Get a specific extraction
curl http://localhost:8080/v1/extractions/<id> \
  -H "Authorization: Bearer $STRUXIO_API_KEY"
```

---

### 3b — Inline Extraction (one-shot, base64 body)

Send a file directly in the request body — no upload step required. The server decodes the base64, deduplicates by MD5, stores the file in S3 if it hasn't been seen before, and returns the extraction result synchronously.

```
POST /v1/extractions/inline
```

```mermaid
sequenceDiagram
    participant C as Client
    participant API as struxio-api
    participant DB as Postgres
    participant S3 as S3/MinIO
    participant G as Gemini API

    C->>API: POST /v1/extractions/inline { file_name, file_type, file_base64, template_id }
    API->>API: base64_decode(file_base64) → bytes
    API->>API: md5_hash = MD5(bytes)
    API->>DB: DocumentRepo::find_by_hash(md5_hash)
    alt document already exists
        DB-->>API: Document (reuse)
    else new file
        API->>S3: StorageClient::upload(key, bytes, content_type)
        API->>DB: DocumentRepo::create(md5_hash, file_name, s3_key, ...)
        DB-->>API: Document
    end
    API->>DB: TemplateRepo::find_by_id(template_id)
    API->>DB: ExtractionRepo::create_with_status(status="processing")
    API->>S3: StorageClient::download(s3_key)
    S3-->>API: file bytes
    API->>G: GeminiClient::extract(bytes, mime_type, prompt, schema)
    G-->>API: { result, input_tokens, output_tokens }
    API->>DB: ExtractionRepo::update_completed(result, tokens, model_id)
    DB-->>API: Extraction
    API-->>C: 200 Extraction { status: "completed", result: {...} }
```

**How to call:**
```bash
# Encode file to base64
B64=$(base64 -i invoice.pdf)   # macOS / Linux

# One-shot extraction — no upload step needed
curl -X POST http://localhost:8080/v1/extractions/inline \
  -H "Authorization: Bearer $STRUXIO_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"file_name\": \"invoice.pdf\",
    \"file_type\": \"pdf\",
    \"file_base64\": \"$B64\",
    \"template_id\": \"<template-uuid>\"
  }"
```

> **Inline vs. two-step upload:** use inline for quick scripts or one-off calls; use the two-step flow for large files or when you need the document ID before extraction.

---

### 4 — Batch Extraction (asynchronous)

Runs many documents through a template in parallel via a Redis Streams job queue. The API returns immediately; the worker processes jobs in the background.

```
POST /v1/batches
GET  /v1/batches
GET  /v1/batches/{id}
GET  /v1/batches/{id}/extractions
```

```mermaid
sequenceDiagram
    participant C as Client
    participant API as struxio-api
    participant Q as Redis Stream
    participant W as struxio-worker
    participant S3 as S3/MinIO
    participant G as Gemini API
    participant DB as Postgres

    C->>API: POST /v1/batches { document_ids: [...], template_id }
    API->>DB: TemplateRepo::find_by_id(template_id)
    API->>DB: BatchJobRepo::create(template_id, total_documents)
    loop for each document_id
        API->>DB: ExtractionRepo::create(doc_id, template_id, batch_job_id, status="pending")
        API->>Q: XADD extractions:queue * extraction_id doc_id template_id workspace_id batch_job_id
    end
    API-->>C: 200 BatchJob { status: "pending", total_documents: N }

    Note over W,G: Worker processes concurrently with bounded permits
    loop for each job in queue
        W->>Q: XAUTOCLAIM stale PEL + XREADGROUP COUNT N
        Q-->>W: ExtractionJob
        W->>DB: claim_for_processing (attempt++, skip if terminal)
        W->>DB: DocumentRepo::find_by_id
        W->>DB: TemplateRepo::find_by_id
        W->>S3: download(s3_key)
        W->>G: GeminiClient::extract(...)
        alt success
            W->>DB: apply_completed + recompute batch counters in one txn
            W->>Q: XACK
        else retryable failure, attempts remaining
            W->>DB: mark_retrying
            W->>Q: ZADD extractions:delayed then XACK
        else permanent or exhausted
            W->>DB: apply_failed + recompute batch counters in one txn
            W->>Q: XADD extractions:dlq then XACK
        end
    end
```

**How to call:**
```bash
# Start a batch
curl -X POST http://localhost:8080/v1/batches \
  -H "Authorization: Bearer $STRUXIO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"document_ids":["<uuid1>","<uuid2>","<uuid3>"],"template_id":"<template-uuid>"}'

# Poll batch status
curl http://localhost:8080/v1/batches/<batch-id> \
  -H "Authorization: Bearer $STRUXIO_API_KEY"

# Get all extractions for a batch
curl http://localhost:8080/v1/batches/<batch-id>/extractions \
  -H "Authorization: Bearer $STRUXIO_API_KEY"
```

---

## Queue Architecture

The extraction job queue is abstracted behind a thin producer/consumer pair in `struxio-core::queue`. Delivery policy (retryability, backoff, batch terminal states) lives in `struxio-core::jobs` and is unit-tested without Redis or Postgres.

```rust
trait QueueProducer { async fn enqueue_extraction(...) }
trait QueueConsumer  { async fn next_job(...) → Option<ExtractionJob> }
```

`RedisProducer` / `RedisConsumer` use Redis Streams with a consumer group. Guarantees:

- **At-least-once delivery.** ACK happens only after a durable success, a durable retry (`retrying` + delayed ZSET), or a durable dead-letter (`failed` + `extractions:dlq`).
- **Concurrent consumers.** `XREADGROUP COUNT N` plus a bounded semaphore; stale PEL entries are reclaimed with `XAUTOCLAIM`.
- **Idempotent terminal state.** Completing or failing an already-terminal extraction is a no-op and does not bump batch counters.
- **Honest batch status.** Counters are recomputed from child rows inside the same transaction as the terminal write: `completed`, `partially_completed`, or `failed` only when no child is pending, processing, or retrying.

Workspace identity is a required stream field. Malformed or nil-workspace entries are dead-lettered and ACKed, never executed.

---

## Authentication

Every endpoint requires `Authorization: Bearer <STRUXIO_API_KEY>`.

The key is validated in the `static_auth` Axum middleware via `FromRequestParts`. On success, it injects an `AuthUser` marker into the request — a unit struct that proves authentication with no identity fields.

```bash
export STRUXIO_API_KEY=your-api-key

curl http://localhost:8080/v1/documents \
  -H "Authorization: Bearer $STRUXIO_API_KEY"
```

---

## Running Locally

```bash
# Start infrastructure
docker compose up -d

# Copy and fill in env vars
cp .env.example .env

# Apply migrations
sqlx migrate run

# Start the API server (port 8080)
cargo run -p struxio-api

# Start the worker (separate terminal — needed for batch jobs)
cargo run -p struxio-worker
```

| Service | Port | Purpose |
|---|---|---|
| struxio-api | 8080 | HTTP API |
| Postgres | 5432 | Primary datastore |
| Redis | 6379 | Job queue (Redis Streams) |
| MinIO | 9000 | S3-compatible document store |
| MinIO console | 9001 | Browser UI for MinIO |
