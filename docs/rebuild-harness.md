# Extraction kernel + harness

The rebuild in one sentence: **keep `extract(document, schema) → JSON` as a tiny kernel, and build a production harness around it.**

That is what this repo almost is today (Gemini + a template). The rebuild is not a new extractor. It is everything you need so that kernel is fast, cited, batchable, agent-native, and trustworthy — without making parse the product.

Langfuse is a harness around an LLM call. Pytest is a harness around `assert`. Struxio is a harness around structured extract.

```
     MCP · REST · CLI · Studio · webhooks     ← adapters (how you hold it)
  ─────────────────────────────────────────
     ingest   parse IR   templates   targets
     validate cite       modes       batch
     jobs     retries    evals       metrics   ← harness (what makes it real)
  ─────────────────────────────────────────
     extract(bytes | ir, schema) → JSON        ← kernel (the only magic)
```

Users (and agents) talk to adapters. The harness never asks them to think about OCR. The kernel does not know about Stripe, S3 check/confirm, or Cursor.

---

## Kernel (stay small)

One function, stable forever:

```text
extract(
  source:   parsed IR | raw bytes,   # IR preferred
  schema:   JSON Schema,             # from template or inline
  prompt:   str,                     # from template
  mode:     fast | accurate | agentic,
  target:   document | page | entity,
) -> { value: Json, citations?, confidence?, usage }
```

Rules:

- No HTTP, no Postgres, no MCP types inside the kernel.
- Swappable LLM (`GeminiClient` today). Swappable parse IR input.
- `vlm_direct` (today’s whole-file Gemini) is a **kernel backend**, not the product.
- If the kernel grows “S3” or “org_id”, we have failed the split.

Today: `GeminiClient::extract` + `ExtractionService::create_sync`. Keep that shape; feed it IR instead of raw PDFs when the harness has parsed.

---

## Harness (this is the product)

Each piece exists only to make `extract` better in production. If it does not help extract, it does not ship.

| Layer | Job | Without it, extract is… |
|---|---|---|
| **Ingest** | path / url / bytes / folder → bytes + mime | a curl toy |
| **Parse IR** | layout, OCR short-circuit, bboxes, parse-once | blind, slow on the 2nd schema, no honest citations |
| **Templates** | named, slugged, versioned schema+prompt | paste JSON Schema every call |
| **Targets** | per document / page / entity | ugly array schemas for line items |
| **Infer / suggest** | schema from NL or a sample file | dead if the user cannot write JSON Schema |
| **Cite** | field → block bbox | untrustworthy JSON |
| **Validate** | `total ≈ sum(lines)`, required, types | silent garbage |
| **Modes** | fast default, accurate, agentic opt-in | either slow or dumb, never both available |
| **Batch / jobs** | queue, concurrency, retries, webhooks | one file in a blocking HTTP request |
| **Evals** | golden docs + expected JSON | we cannot say we improved |
| **Observe** | p50/p95, tokens, queue lag, 402 | “it’s Rust” with no numbers |
| **Tenancy / credits** | cloud only | cannot sell |

Parse, split, and classify live **in the harness**, not as sibling products:

- **Parse** = cache + grounding for extract (and a RAG escape hatch).
- **Split** = “run extract on each sub-document.”
- **Classify** = “pick a template, then extract.”

Studio is the harness **debugger**: click a field, see the bbox, rerun with `accurate`, diff template versions. Not a second app.

MCP/CLI/REST are **adapters** over the same harness calls. Same `extract` tool, same templates, same modes.

---

## Why this is decent

Parse-first platforms (Reducto, ADE) make you learn their tree, then maybe extract. AP platforms (Rossum) hide the schema inside invoice-only models. We do the Langfuse thing: **the unit of value is one function**, and the company is the harness that makes that function survivable.

It also matches the code we already have: templates, batch jobs, `credits_charged`, a worker. The harness was sketched; it was never finished. The rebuild finishes it — and puts parse *under* extract instead of beside it.

---

## Performance lives in the harness

The kernel should be as dumb-fast as the backend allows. The harness is where we refuse to be slow:

- Choose `digital_pdf` vs OCR vs VLM **before** calling the kernel.
- Pass IR into the kernel so we do not base64 a 40-page PDF twice.
- Parallelize pages and batch jobs **outside** the kernel.
- Honor `timeout_ms` / `max_pages` / `mode=fast` **outside** the kernel.
- Default MCP/REST to `mode=fast`.

---

## Implementation mapping

| Kernel | `core::extract` (today: gemini + extraction_service sync path) |
|---|---|
| Harness | `core::ingest`, `parse`, `templates`, `validate`, `jobs`; worker concurrency |
| Adapters | `api` (`POST /v1/extract`), `mcp` (`extract`, `extract_batch`), CLI, Studio |

Do not put harness policy (credits, workspace, mode routing) in `GeminiClient`. Do not put kernel JSON-schema filling in Axum handlers.
