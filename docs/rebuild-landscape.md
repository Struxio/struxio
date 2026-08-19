# Landscape: other platforms, our differentiator, performance

Companion to [rebuild-rfc.md](./rebuild-rfc.md) and [rebuild-reducto-map.md](./rebuild-reducto-map.md).

Reducto is the **platform shape** (parse IR, verbs, Studio, MCP). It is not the ceiling. Struxio already has something most parse-first products bury: **you name the JSON you want, reuse it, batch against it.** Keep that as the product. Steal the best ideas from everyone else. Never ship a slow default.

---

## 1. Honesty about “structured as you wish”

Reducto, Extend, LlamaExtract, Landing ADE, and Chunkr **all** take a JSON Schema and return matching JSON. Schema extract is not a secret.

What they usually do **not** lead with:

| They | We already / should |
|---|---|
| Parse is the homepage. Extract is step 2. ADE’s extract even wants parse markdown as input. | **Extract is the homepage.** Parse is infrastructure. An agent says “use the invoice template,” not “parse then extract.” |
| Schema is an argument you paste every call. | **Templates are first-class objects**: named, reusable, system seeds (Invoice, Receipt already in the DB), CRUD, batch `document_ids + template_id`. |
| Configs live in their cloud Studio. | Same templates work in OSS, MCP, REST, and cloud. |
| Switching schemas often means re-sending the whole file through a VLM. | Parse once → run **many templates** on the same IR (Landing ADE got this right). |

So the differential is not “we have JSON Schema.” It is:

1. **Schema-first product** — the contract you care about is *your* JSON, not our chunk tree.
2. **Named templates + batch** — already in this repo; make them versioned and slug-addressable (`invoice`, not a UUID).
3. **Parse is optional at the API edge** — `extract` may parse internally; you are not forced into a two-step dance.
4. **Open source** — LlamaExtract configs and Reducto Studio are closed. Ours are not.

LlamaExtract’s “extraction configurations” are the closest cousin. Copy their **extraction target** (`per_doc` / `per_page` / `per_entity`) onto our templates. That is a feature Reducto’s public MCP does not emphasize and that invoice/line-item work desperately needs.

---

## 2. Platform-by-platform (steal / skip / beat)

### Reducto — reference platform

**Good:** parse IR (chunks/blocks/bboxes), parse-once handles, citations, agentic review, MCP + CLI + SKILL.md, Studio overlay, 30+ file types.  
**Bad:** expensive; closed; hosted MCP is API-key-first; `upload_file` hop; parse-first mental model; agentic latency (public benches ~9–13s/page).  
**We:** same verbs + IR; extract-first UX; OAuth MCP; cheaper; OSS; **fast path as default**.

### Extend.ai — production IDP

**Good:** extract infers a schema if you omit one; `extend:type` (currency, date, signature); versioned schemas; Review Agent / HITL; evals before promoting a processor; split + classify in one pipeline. Parse 2.0 accuracy is high.  
**Bad:** ~**20s/page** on their own RealDoc-Bench for the accurate mode. HITL and evals are a year of UI. Closed.  
**Steal:** schema inference; typed semantic fields; template **versioning**; “parse quality vs extract quality” debugging (if extract is wrong, look at parse first).  
**Skip for v1:** full HITL product, $500/mo review queues.  
**Beat:** latency. Never make 20s/page the default.

### LlamaParse / LlamaExtract — developer extract

**Good:** reusable **extraction configurations** (schema + tier); Pydantic/Zod; **per_doc / per_page / per_entity**; citations; hosted MCP with OAuth (the connect UX to copy); 10k free credits.  
**Bad:** parse and extract feel like two products; LlamaIndex-centric; agentic parse is **~30s/page** in Extend’s bench; markdown-first (weaker bboxes than ADE).  
**Steal:** extraction targets; SDK-native schema (Pydantic/JSON Schema); OAuth MCP; config as a named object (our templates).  
**Skip:** tying the product to one agent framework.

### Landing ADE — visual grounding

**Good:** parse → hierarchical JSON with coordinates; extract schema with `x-alternativeNames`; **parse once, extract many times**; schema-from-NL / playground; form_field, checkbox, signature, barcode chunk types; auto-split huge PDFs and parse chunks in parallel.  
**Bad:** extract often documented as “pass parse markdown in”; closed; not MCP-native in the Reducto sense.  
**Steal:** parse-once / extract-many; alt names on fields; form-native block types; **page-parallel parse**.  
**Skip:** requiring the user to glue parse→extract.

### Unstructured — RAG ingest

**Good:** 60+ types, connectors, chunk/embed, OSS library + closed platform, MCP for workflows. Cheap at volume.  
**Bad:** user JSON Schema extract is not the product (elements JSON). Workflow CRUD MCP is hostile to indie builders.  
**Steal:** connectors later; don’t confuse RAG partition with extract.  
**Skip:** making chunk/embed a v1 tool.

### Amazon Textract / Google Document AI / Azure DI — cloud IDPs

**Good:** **fast** (Textract median ~2s on some benches; Azure DI ~5s/page in Extend’s parse bench). IAM, regions, prebuilts (invoice, ID, expense). Real bboxes.  
**Bad:** not arbitrary nested JSON Schema (query fields ~20, or train a custom model). Page-scoped (Textract). Ugly block graphs you must stitch. Console-first, not agent-first.  
**Steal:** a **fast structured path** for invoices/receipts that does not call a frontier VLM; confidence on every value.  
**Skip:** training custom processors; AWS-only lock-in.  
**Beat:** nested schema + one MCP URL.

### Sensible — hybrid rules + LLM

**Good:** SenseML layout queries are **faster and deterministic** when the form is stable; LLM for messy docs; mix both; 150+ **open** prebuilt configs (tax, insurance); validations; human review that highlights the source field.  
**Bad:** SenseML is a new language (nerds only); not a general agent MCP play; closed platform.  
**Steal:** **validations** on extracted JSON (required, regex, “total ≈ sum(line_items)”); prebuilt template library as OSS; layout-fast path for known forms later.  
**Skip:** inventing a second query language in v1. JSON Schema + field descriptions stay the authoring UX.

### Chunkr — parse + schema extract

**Good:** schema extract with citations that **mirror the schema via field paths**; word-level and segment citations; throughput vs thinking parse models.  
**Bad:** smaller ecosystem; not the Langfuse analog.  
**Steal:** citation object that mirrors the result tree (cleaner than Reducto’s wrap-every-leaf, or offer both).  
**Skip:** training our own parse VLM.

### Mistral OCR / Document AI — OCR API

**Good:** cheap (~$4/1k pages), blocks + bboxes + confidence, 170 langs, self-host container, optional “document AI” structured layer on the same call. Fast-ish OCR.  
**Bad:** not a template/batch/MCP product.  
**Steal:** **Mistral OCR as a ParseBackend** next to Docling/PaddleOCR; same IR. Another way to stay cheap and fast.  
**Skip:** making Mistral the only path (vendor lock).

### Nanonets / Rossum / Mindee / Veryfi — AP automation

**Good:** pre-trained invoice/receipt models, ERP write-back, three-way match, learning from corrections. Out-of-the-box accuracy on *those* docs.  
**Bad:** not “any JSON you want”; enterprise AP, not agent builders; closed.  
**Steal:** excellent **system templates** for invoice/receipt/PO (we already started); optional learning-from-edits later.  
**Skip:** SAP connectors in year one.

### Docling / MinerU / Marker / Kreuzberg — OSS engines

**Good:** actual parse quality, bboxes, many formats. Kreuzberg is Rust.  
**Bad:** libraries, not products (no named templates, billing, hosted MCP, Studio as a company).  
**Steal:** they **are** our backends.  
**Beat:** productize them.

---

## 3. Features we add *beyond* a Reducto clone

Priority is “schema-first + speed.” Do not turn this into a kitchen sink.

| Feature | From | Why it is ours |
|---|---|---|
| Named, slugged, versioned templates | Struxio today + LlamaExtract configs + Extend versions | The differential. `template: invoice@v3` |
| Extract-first API/MCP (`extract` parses if needed) | ADE inverse | Agents never learn parse |
| Extraction target: `document` \| `page` \| `entity` | LlamaExtract | Line items / one-record-per-page without ugly array schemas |
| Schema inference if schema omitted | Extend | First-run in chat: “extract this” still returns JSON |
| `suggest_schema` + field descriptions as the prompt | ADE + LlamaExtract | Schema literacy is the #1 drop-off |
| Parse once, run N templates | ADE | Folder of mixed docs; cheap |
| JSON Schema extensions: `x-description`, `x-altNames`, `x-format: money\|date` | ADE + Extend | Better extract without SenseML |
| Validations on result | Sensible | `total == sum(lines)` → confidence down / 422-able |
| Fast / accurate / agentic **explicit modes** | everyone, we default **fast** | See §4 |
| System template library (OSS) | Sensible library + our Invoice/Receipt seeds | Invoice, receipt, PO, ID, contract |
| Citations mirroring schema paths | Chunkr | Studio click-to-bbox |
| MCP + SKILL as equal to REST | Reducto / LlamaParse | Builder path |
| CLI `struxio extract ./invoices --template invoice` | Reducto CLI | Nerd path, still schema-first |
| Page-parallel parse | ADE | Performance |

Explicitly **later** (do not block): HITL queues, ERP match, SenseML-like layout DSL, generate/redact/translate, custom model training.

---

## 4. Performance doctrine (non-negotiable)

Public benches (Extend RealDoc-Bench, Aug 2026-ish):

| Mode | Approx. latency / page | Accuracy (their Q&A) |
|---|---|---|
| Azure DI | ~5.5s | 88.8% |
| Textract | ~6.6s | 70.5% |
| Reducto (non-agentic) | ~9.6s | 88.5% |
| Reducto agentic | ~13s | 91.1% |
| Extend Parse 2.0 | ~**19.7s** | 95.7% |
| LlamaParse agentic | ~**30s** | 92.1% |

Accuracy-at-all-costs is how incumbents get slow and expensive. Struxio defaults the other way. **Fast is the product.** Accurate is a flag.

### Rules

1. **Default path must feel instant on a digital invoice** — target **p50 < 3s** end-to-end extract for a 1–2 page text-layer PDF, schema already known. If we miss this, we have failed even if parse looks like Reducto.
2. **Never default to agentic.** `mode=fast | accurate | agentic`. Fast = digital pdfium or Docling CPU, Flash extract on markdown. Agentic = review loop, billed, slow, opt-in.
3. **Short-circuit.** Text-layer PDF → no OCR, no VLM parse. Scans → OCR. Only low-confidence blocks go to a VLM.
4. **Parse once.** Extract, split, classify, and a second template reuse `parse_job_id`. Re-OCR is a bug.
5. **Parallelism.** Pages parse concurrently. Batch worker is **not** `XREAD COUNT 1` forever (today’s worker is serial — that is the first performance fix in *this* repo). Configurable concurrency with backpressure.
6. **Rust stays on the hot path.** Sidecars for Docling/Paddle/Mistral OCR; no Python in the API process. Zero extra copies of file bytes where we can. Connection pools to PG/Redis/S3 always on.
7. **Sync vs async is a size switch**, not a religion. Small extract = one HTTP round trip (what we have). Big / batch = queue. Timeouts advertised (`wait` + `job_id` fallback).
8. **Budgets in the API.** `max_pages`, `timeout_ms`, `mode`. Exceed → partial result or 400, not a 60s hang.
9. **Measure it.** CI fixture: 1-page digital invoice extract latency + a scan path. Cloud: p50/p95 parse and extract on the dashboard. No “it’s Rust so it’s fast” without numbers.
10. **Cost follows speed.** Flash, parse-once, OCR-skip is how we undercut $0.03/page. Pro/agentic is a multiplier the user chooses.

### Fast path vs accurate path (product)

```
extract(mode=fast)     → digital_pdf or docling-lite → Flash JSON     [default]
extract(mode=accurate) → docling + table model → Flash/Pro
extract(mode=agentic)  → accurate + VLM review on low-confidence
```

MCP `extract` uses `fast` unless the user/agent says “be thorough” or confidence comes back low (then `next_steps` can suggest `accurate`).

### What “super performant” is not

- Not “always call Gemini Pro on the raw PDF” (today’s code, large files).
- Not “agentic OCR on every page like Extend 2.0.”
- Not “serial worker, unbounded lists, double S3 download” (today’s bugs).
- Not GPU-required for the default self-host. Compose `core` stays CPU. `parse` / `gpu` profiles are opt-in quality.

---

## 5. What this does to the rebuild

- Keep Reducto as the **IR and verb set**.
- Keep Langfuse as the **OSS+cloud motion**.
- **Lead with templates** in README, MCP instructions, Studio, and homepage — that is the wedge vs parse-first SaaS.
- Add LlamaExtract targets, Extend schema inference, Sensible validations, ADE parse-once, Mistral/Docling backends — without delaying P0 (`workspace_id` + one-shot extract).
- Treat the serial worker, whole-file Gemini base64, and missing parse short-circuit as **performance bugs**, same severity as missing features.
