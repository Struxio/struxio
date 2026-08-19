<div align="center">

# Struxio

**Open-source document data extraction API**

[![License: AGPL-3.0-only](https://img.shields.io/badge/License-AGPL--3.0--only-blue.svg)](./LICENSING.md)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Issues Welcome](https://img.shields.io/badge/Issues-welcome-brightgreen.svg)](https://github.com/Struxio/struxio/issues)

</div>

Struxio is a self-hostable REST API that extracts structured data from documents (PDFs, images) using AI models. Define a JSON schema template once, point it at a document, and get back structured JSON.

---

## Features

- 📄 **Document extraction** — upload PDFs/images and extract structured data via JSON schema templates
- 🔁 **Batch processing** — submit hundreds of documents as a single batch job
- 🧩 **Custom templates** — define reusable extraction schemas with prompt templates
- 🔑 **API key auth** — static bearer token auth for self-hosted deployments
- 🐳 **Docker-ready** — ships with a `docker-compose.yml` for local dev

## Architecture

```
┌─────────────────────────────────────────────┐
│                  struxio                    │
│                                             │
│  crates/api     — Axum HTTP server          │
│  crates/core    — business logic, services  │
│  crates/db      — SQLx repositories         │
│  crates/common  — shared models & config    │
│  crates/worker  — background job worker     │
└─────────────────────────────────────────────┘
        ↕ PostgreSQL   ↕ Redis   ↕ S3
```

## Tech Stack

| Layer | Technology |
|---|---|
| Language | Rust (2021 edition) |
| HTTP framework | Axum 0.8 |
| Database | PostgreSQL via SQLx 0.8 |
| Queue | Redis Streams (pluggable — Kafka-ready) |
| Storage | S3-compatible (MinIO for local dev) |
| AI model | Google Gemini |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) ≥ 1.75
- [Docker](https://docs.docker.com/get-docker/) & Docker Compose
- A Gemini API key ([get one free](https://aistudio.google.com/))

### 1. Clone and configure

```bash
git clone https://github.com/Struxio/struxio.git
cd struxio
cp .env.example .env
# Edit .env — at minimum set GEMINI_API_KEY and STRUXIO_API_KEY
```

### 2. Start infrastructure

```bash
docker compose up -d   # starts PostgreSQL, Redis, MinIO
```

### 3. Run the API server

```bash
cargo run -p struxio-api
```

The API is now running at `http://localhost:8080`.

### 4. Make your first extraction

```bash
# Authenticate with your API key
export API_KEY="your-key-from-.env"

# Upload a document
curl -X POST http://localhost:8080/v1/documents/check \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"md5_hash":"abc123","file_name":"invoice.pdf","file_type":"application/pdf","size_bytes":102400}'

# Create an extraction template
curl -X POST http://localhost:8080/v1/templates \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Invoice",
    "json_schema": {"total": "number", "vendor": "string", "date": "string"},
    "prompt_template": "Extract the invoice total, vendor name, and date."
  }'
```

## API Reference

| Method | Endpoint | Description |
|---|---|---|
| GET | `/v1/health` | Health check |
| POST | `/v1/documents/check` | Check if document exists / get upload URL |
| POST | `/v1/documents/confirm` | Confirm S3 upload |
| GET | `/v1/documents` | List all documents |
| GET | `/v1/templates` | List extraction templates |
| POST | `/v1/templates` | Create template |
| POST | `/v1/extractions` | Start extraction |
| POST | `/v1/extractions/inline` | Start extraction directly from base64 document |
| GET | `/v1/extractions/{id}` | Poll extraction status |
| POST | `/v1/batches` | Submit batch job |
| GET | `/v1/batches/{id}` | Poll batch status |

### Document Uploading Pattern (Direct-to-S3)
To ensure scalability and prevent our API servers from becoming bottlenecks with large files, Struxio uses a 3-step "Pre-signed URL" pattern for uploading documents:

1. **Check & Request URL**: Send `POST /v1/documents/check` with the file metadata (`md5_hash`, `size_bytes`, `file_name`, `file_type`).
    - If the API returns `exists: true`, the document can be used immediately (saving bandwidth).
    - If it's new, the API returns an `upload_url` (a secure, temporary S3 pre-signed URL) and an `s3_key`.
2. **Direct Upload**: Your client performs a standard HTTP `PUT` request with the raw file payload directly to the provided `upload_url`.
3. **Confirm**: Once the upload to S3 completes, send `POST /v1/documents/confirm` with the file metadata and the `s3_key` to finalize the document record in the database.

*Alternative: Inline Extractions*
For smaller files where persistence isn't required, you can use `POST /v1/extractions/inline`. This endpoint accepts a `file_base64` string in the JSON payload, temporarily storing and extracting data in one request, bypassing the 3-step upload.

## Configuration

All config is via environment variables. Copy `.env.example` to `.env`.

| Variable | Required | Description |
|---|---|---|
| `DATABASE_URL` | ✅ | PostgreSQL connection string |
| `REDIS_URL` | ✅ | Redis URL |
| `S3_ENDPOINT` | ✅ | S3/MinIO endpoint |
| `S3_BUCKET` | ✅ | S3 bucket name |
| `S3_ACCESS_KEY_ID` | ✅ | S3 access key |
| `S3_SECRET_ACCESS_KEY` | ✅ | S3 secret key |
| `GEMINI_API_KEY` | ✅ | Google Gemini API key |
| `STRUXIO_API_KEY` | ✅ | Bearer token for self-hosted auth |
| `SERVER_PORT` | ❌ | Port (default: 8080) |

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License
New revisions are offered under [AGPL-3.0-only](./LICENSE). Alternative commercial terms are available separately through a written agreement. Revisions through and including [`0f4bd171e08e586000c6f5668b3bebf9e20dcfa9`](https://github.com/Struxio/struxio/commit/0f4bd171e08e586000c6f5668b3bebf9e20dcfa9) remain under Apache-2.0. See [LICENSING.md](./LICENSING.md) for the licensing history.
