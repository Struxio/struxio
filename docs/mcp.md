# Struxio MCP adapter

Minimal **stdio** MCP server for agents. It is an adapter over the existing `core` services (`TemplateService`, `ExtractionService`, `BatchService`) and the same `PrincipalContext` / scope checks used by the HTTP API. It does not call repositories, parse providers, evidence, or generic jobs.

## Run

The process uses the same environment as `struxio-api` (`DATABASE_URL`, Redis, S3, Gemini, `STRUXIO_API_KEY`). Logs go to **stderr**. **stdout** is JSON-RPC only.

```bash
cargo run -p struxio-mcp
```

Cursor / Claude Code example (use a built binary so cargo compile output cannot touch stdout):

```json
{
  "mcpServers": {
    "struxio": {
      "command": "/absolute/path/to/struxio-mcp",
      "env": {
        "DATABASE_URL": "postgres://struxio:struxio@localhost:5432/struxio",
        "REDIS_URL": "redis://127.0.0.1:6379",
        "S3_ENDPOINT": "http://localhost:9000",
        "S3_ACCESS_KEY_ID": "minioadmin",
        "S3_SECRET_ACCESS_KEY": "minioadmin",
        "S3_BUCKET": "struxio",
        "GEMINI_API_KEY": "your-gemini-api-key",
        "STRUXIO_API_KEY": "your-local-key"
      }
    }
  }
}
```

OSS stdio auth is the process environment: `STRUXIO_API_KEY` must be set, and every tool call uses the seeded local workspace principal loaded from Postgres. Callers cannot pass `workspace_id`. Hosted OAuth MCP is out of scope for this crate.

## Protocol

| Decision | Choice |
|---|---|
| Transport | stdio, one JSON-RPC 2.0 object per line, no embedded newlines |
| Framing | newline-delimited (MCP stdio spec). LSP `Content-Length` is not supported |
| Batches | top-level JSON-RPC arrays are rejected (`-32600` / `batches_not_supported`) |
| Handshake | `initialize` then `notifications/initialized`. `tools/list` and `tools/call` require `initialize`. `ping` does not |
| Versions | accept `2024-11-05`, `2025-03-26`, `2025-06-18`; unknown client versions fall back to `2025-03-26` |
| Logging | stderr only |
| Tool errors | MCP `result.isError` + `{ "error": { "code", "message" } }`. Protocol failures stay JSON-RPC errors |
| Success body | JSON text in `content[0].text` plus `structuredContent` |

Rebuild RFC verbs this crate implements: `list_templates`, `extract`, `extract_batch`, plus `get_template` / `get_extraction` / `get_batch` instead of a generic `get_job`. Not implemented: `parse`, `split`, `classify`, `suggest_schema`, `upload_file`, `get_account`.

## Tools

| Tool | Scope | Maps to |
|---|---|---|
| `list_templates` | `templates:read` | `TemplateService::list` (id/name/description/`is_system` only) |
| `get_template` | `templates:read` | `TemplateService::get` |
| `extract` | `extractions:create` | `create_inline` (`file_base64`) or `create_sync` (`document_id`) |
| `get_extraction` | `extractions:read` | `ExtractionService::get` |
| `extract_batch` | `batches:create` | `BatchService::create` |
| `get_batch` | `batches:read` | `BatchService::get` |

`extract` requires `template_id` and **exactly one** of `document_id` or `file_name` + `file_type` + `file_base64`.

## Limits

| Limit | Value |
|---|---|
| JSON-RPC line | 12 MiB |
| Decoded inline file | 8 MiB |
| `extract_batch.document_ids` | 1–100, unique, non-nil |
| `file_name` / `file_type` | 1–255 / 1–64 bytes |

Unknown argument fields are rejected. Nil UUIDs are rejected.

## Errors

Stable `error.code` values: `invalid_arguments`, `input_too_large`, `unauthorized`, `forbidden`, `not_found`, `rate_limited`, `conflict`, `upstream_error`, `internal_error`, `tool_not_found`.

`forbidden` is always `insufficient scope`. `not_found` is always `resource not found`. Database strings, workspace ids, and row identities are not placed on the wire.

## Limitations

- No Streamable HTTP, OAuth, or `/mcp` route (cloud control-plane work).
- No parse / split / classify / evidence / provider tools.
- No path or URL ingest; agents must send `file_base64` or a workspace `document_id`.
- Inline and single-document `extract` are synchronous (same as REST). `extract_batch` is async via the existing worker.
- One principal per process (local OSS operator). Scope restriction for hosted callers is not wired here.
- JSON-RPC batches and Content-Length framing are not supported.
