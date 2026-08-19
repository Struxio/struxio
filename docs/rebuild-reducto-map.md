# Reducto as the product spec

**Status:** proposed companion to [rebuild-rfc.md](./rebuild-rfc.md)  
**Positioning:** Struxio is the open-source Reducto, the way Langfuse is the open-source LangSmith. Same platform shape, same agent/MCP surface, cheaper, self-hostable, no custom-VLM lab.

Reducto’s models are closed. Their **product** is not magic — it is a pipeline of layout, OCR, VLM review, a canonical parse tree, then extract/split/classify/edit on top, plus Studio and MCP. Most of those stages already exist as open-source libraries. We orchestrate them in Rust, own the IR and the APIs, and keep every capability in OSS. Cloud is the same software with GPUs, hosted keys, and a credit card.

We do **not** clone their weights. We clone the **platform**.

---

## 1. Langfuse → LangSmith, applied here

| Langfuse move | Struxio move |
|---|---|
| Same category as LangSmith (LLM engineering), not a “lite tracing SDK” | Same category as Reducto (agentic document platform), not a “thin Gemini wrapper” |
| Full product in open source (tracing, evals, prompts, playground, UI) | Full product in open source (parse, extract, split, classify, MCP, Studio) |
| Cloud = same codebase, zero ops, usage pricing | Cloud = same engine, hosted parse workers + VLMs, cheaper credits |
| Self-host is first-class, not an enterprise add-on | Compose/Helm is first-class; VPC/SSO later |
| Cheaper than the closed leader | Price under Reducto (~$0.03/page extract) by using OSS parse + Flash-tier VLMs |
| Switching is easy because the *job* is the same | Mirror Reducto’s verbs so an agent or a customer can switch with a URL change |

Langfuse did **not** win by being a subset. They won by being the same product you can run yourself. That is the bar: a self-hoster can parse, extract with citations, split a packet, classify a pile, and inspect bboxes in Studio. The nerd still will not pay. The builder will, because he does not want to run Docling + PaddleOCR + Redis + GPUs.

What stays out of OSS is only the control plane extras: Stripe, hosted model keys, multi-tenant metering, SSO. Not parse quality. Not Studio. Not MCP.

---

## 2. What Reducto actually is

Public architecture (their own write-up): **12+ models**, three stages.

```
file → layout CV (regions + bboxes)
     → VLM contextual review (tables, figures, forms, reading order)
     → agentic OCR loop (re-check low confidence, merged cells, handwriting)
     → canonical parse tree (chunks + blocks + bboxes + confidence)
           │
           ├─ Extract (schema JSON + citations pointing at blocks)
           ├─ Split (section / sub-document boundaries)
           ├─ Classify (plain-language taxonomy)
           ├─ Edit (fill PDF/DOCX)
           ├─ RAG chunking (embed vs display markdown)
           └─ Studio (overlay bboxes on the page)
```

Core HTTP verbs they ship (and we should):

| Verb | Job | OSS now? |
|---|---|---|
| **Upload** | ingest file, return a handle (`reducto://` / `struxio://`) | we have S3 check/confirm; agents need one-shot ingest |
| **Parse** | layout + OCR + chunks/blocks/bboxes | **missing — this is the rebuild’s spine** |
| **Extract** | schema JSON, optional citations | we have Gemini-on-raw-bytes; no parse IR, no citations |
| **Split** | one file → many logical docs | missing |
| **Classify** | route by NL categories | missing |
| **Edit** | fill forms / patch DOCX | missing (later) |
| **Pipeline** | chain the above, Studio-deployed | later |
| **Jobs** | async, poll, cancel, delete, webhooks | we have a weak batch worker |
| **MCP / CLI / SDK / SKILL.md** | agent distribution | missing |

They also market generate / redact / translate. Treat those as **v2**. Do not block the platform on them.

Supporting product (copy the shape, not the brand):

- Studio: upload, parse viewer, bbox overlay, extract playground, pipeline builder, API keys
- Async + webhooks (they use Svix)
- Batch queue (cheaper, slower)
- Credits per page with per-feature multipliers (agentic, latency)
- Handles so parse is paid once and extract/split reuse `jobid://`
- Hosted MCP + local stdio MCP that can read disk
- `SKILL.md`, `llms.txt`, `.well-known/ai-catalog.json`

### Parse IR to copy (this is the contract)

Own a **Struxio document IR** that looks like Reducto’s on purpose. Switching and Studio overlays depend on it.

- `chunks[]`: `content` (markdown), `embed` (RAG-optimized), `blocks[]`
- `blocks[]`: `type`, `content`, `bbox`, `confidence`, `image_url?`
- Block types: `Title`, `Section Header`, `Header`, `Footer`, `Text`, `Table`, `Figure`, `Key Value`, `List Item`, `Checkbox`
- `bbox`: normalized `[0,1]` `{ left, top, width, height, page, original_page }`
- Confidence: `"high" | "low"` plus optional `parse_confidence` 0–1
- Large results: `result.type = "full" | "url"` (same trap they solved)

Extract with citations wraps each field:

```json
{ "total": { "value": 1575.0, "citations": [{ "type": "Table", "content": "Total Due: $1,575.00", "bbox": { "...": "..." } }] } }
```

Citations are cheap **if extract reads the parse tree** instead of a raw PDF. That is why parse-then-extract is not optional.

### MCP tools they expose (mirror, then slim)

Reducto: `get_documentation`, `upload_file`, `parse_document`, `extract_data`, `split_document`, `classify_document`, `edit_document`, `get_job`, `list_jobs`.

Struxio v1 (OSS + cloud, same names in both):

| Tool | Notes |
|---|---|
| `parse` | Default first step for RAG / inspection. Returns `job_id` + chunks (truncated) |
| `extract` | Schema or `template_id`. If no parse job given, parse internally and reuse |
| `extract_batch` | Folder / glob / urls |
| `split` | NL section descriptions |
| `classify` | NL taxonomy |
| `suggest_schema` | From description or a sample parse |
| templates + `get_job` | as in the RFC |
| `get_account` | cloud only |

Do **not** require the agent to call `upload_file` then parse (Reducto’s own footgun). `parse` / `extract` accept `path` | `url` | `file_base64` | `document_id` | `job_id`. Internally we ingest. Edit stays off MCP until the edit engine exists.

---

## 3. OSS library map (do not train a VLM lab)

Reducto’s moat is **orchestration + evals + enterprise packaging**, not a secret file format. Rebuild each stage from libraries. Keep a **provider trait** so we can swap backends without changing the API.

### 3.1 Ingest and rasterize

| Need | Libraries | License | Notes |
|---|---|---|---|
| Digital PDF text + page raster | `pdfium` / `pypdfium2`, `poppler`, `pdfium-render` (Rust) | Apache-2 / GPL (poppler) | Skip OCR when a text layer exists |
| Office → PDF | LibreOffice headless, or `docx-rs` / `calamine` for native xlsx | MPL / MIT / Apache | Reducto supports 30+ types; we start with PDF, images, DOCX, XLSX, PPTX |
| Images | `image` crate, `pdfium` for TIFF multipage | MIT | |
| MIME / sniff | `infer`, `tree_magic` | MIT | Normalize `pdf` vs `application/pdf` |

Prefer **pdfium** over Poppler in the default path (license + embedding). LibreOffice as an optional sidecar for Office files.

### 3.2 Layout (the CV stage)

| Need | Libraries | License | Notes |
|---|---|---|---|
| **Default structured parser** | [Docling](https://github.com/docling-project/docling) (IBM) — Heron layout, TableFormer, `DoclingDocument` with provenance | MIT | Best OSS “blocks + bboxes + types” today. Python sidecar. |
| Fast layout / OCR toolkit | [Surya](https://github.com/VikParuchuri/surya) (via Marker) | GPL-3.0 | 90+ langs, layout labels, reading order, tables. GPL is OK **inside our AGPL server**; do not ship Surya inside an Apache MCP proxy. |
| Scientific / formulas | [MinerU](https://github.com/opendatalab/MinerU) — DocLayout-YOLO, UniMERNet, PP-OCR | Apache-2 (weights may need HF click-through) | Optional backend, not default |
| Rust-native extract | [Kreuzberg](https://github.com/kreuzberg-dev/kreuzberg) | Apache-2 | Closest in-language twin; use as a backend, we still own the product IR |
| Layout YOLO family | DocLayout-YOLO, RT-DETR (inside Docling) | various | Do not call these from API code; wrap via a parser backend |

**Decision:** Docling is the default `ParseBackend`. Map `DoclingDocument` → Struxio IR (never expose Docling types on `/v1/parse`). Marker/Surya and MinerU are `--parser marker|mineru` later.

### 3.3 OCR (scans, photos, handwriting)

| Need | Libraries | License | Notes |
|---|---|---|---|
| Default OCR | [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) PP-OCRv6 | Apache-2 | Strong multilingual, used inside MinerU |
| Fallback CPU | Tesseract (`leptess` / `tesseract-rs`) | Apache-2 | Always available, worse on tables |
| Line OCR + layout | Surya | GPL-3 | Good when we already opted into Marker |
| Small VLM OCR | Granite-Docling 258M, PaddleOCR-VL, olmOCR, GOT-OCR 2.0 | Apache / research | Optional quality tier; GPU |
| EasyOCR | EasyOCR | Apache-2 | Docling extra; fine as optional |

Detect text-layer PDFs first. OCR is the expensive path.

### 3.4 Tables and figures

| Need | Libraries | License |
|---|---|---|
| Table structure | Docling TableFormer; Surya table rec; StructEqTable (MinerU) | MIT / GPL / Apache |
| Spreadsheets | `calamine` (Rust), or parse XLSX as cells not pages | MIT |
| Chart → data | VLM pass on figure crops (Gemini / local VLM) | n/a — our agentic loop |
| Figure captions | same VLM pass | |

### 3.5 Agentic / multi-pass (this is *our* code)

Reducto’s “Agentic OCR” is a **review loop**, not a unique model:

1. Parse once.
2. Flag `confidence = low` blocks (and always tables / checkboxes / handwriting).
3. Crop the bbox from the page image.
4. Ask a VLM: “correct this block; return type + text.”
5. Write back into the IR. Repeat until clean or budget exhausted.

Implement this in `struxio-core` as `ReviewPass`. Model is a provider: Gemini, OpenAI, Anthropic, Ollama, vLLM. OSS users BYO. Cloud uses our key. This is the Langfuse move: **the loop is the product; the model is a plugin.**

Deep Extract (their extract-time agentic loop) is the same pattern on the schema: extract → verify fields against cited blocks → retry misses. Ship as `extract.settings.deep = true` after vanilla extract works.

### 3.6 Extract, split, classify (LLM layer)

| Need | Approach | Keep cheap |
|---|---|---|
| Extract | Structured output (Gemini `responseSchema`, or any JSON-mode LLM) over **parse markdown + block id map**, not raw PDF bytes | Reuse parse job; Flash default; send page crops only for cited fields |
| Citations | LLM returns `block_id` or we fuzzy-match value → block.content | IR makes this honest; do not invent bboxes |
| Split | LLM over chunk headings / first-N tokens per page | Cheap; no OCR |
| Classify | LLM over first pages + title blocks | Cheap |
| Suggest schema | LLM over one parse | already planned |

Current Struxio (whole file as Gemini inline base64) stays as `parser = "vlm_direct"` for tiny happy-path invoices. Default product path becomes `parse` then `extract`.

### 3.7 Edit / generate / redact (later)

| Need | Libraries |
|---|---|
| PDF form fill | `lopdf`, `pdf-rs`, pypdf, pdfrw |
| DOCX patch | `python-docx`, `docx-rs` |
| Redact | overlay black rects from bboxes (once we have IR) — surprisingly small |
| Translate + layout | hard; wrap a VLM per block, rewrite PDF later |

Do not start here.

### 3.8 Studio / agent distribution

| Need | Libraries |
|---|---|
| PDF.js bbox overlay | PDF.js + a canvas/SVG layer from normalized bboxes |
| Spreadsheet overlay | they open-sourced a SpreadsheetViewer idea; we can do similar on `calamine` cells |
| MCP | `rmcp` (Rust), same tools OSS and cloud |
| CLI | `struxio parse ./file.pdf` — clap in this repo |
| SKILL.md / llms.txt | copy Reducto’s agent onboarding, point at our URLs |

---

## 4. Target architecture (engine)

Rust stays the product process. Model zoos stay in **sidecars**. Do not rewrite Docling in Rust.

```
                    ┌─────────────┐
   files            │ struxio-api │  REST + /mcp
                    │ struxio-mcp │
                    └──────┬──────┘
                           │ core
                    ┌──────▼──────┐
                    │  ingest     │  pdfium, office convert, S3
                    │  parse      │  ParseBackend trait
                    │  review     │  VLM loop (optional)
                    │  extract    │  LLM + schema + citations
                    │  split/clf  │  LLM over IR
                    │  jobs       │  Redis + worker
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        parse-worker   vlm-worker    llm-worker
        (Docling /     (Ollama /     (Gemini /
         PaddleOCR)     vLLM /        OpenAI /
                        Gemini)       local)
```

`ParseBackend` implementations:

| Id | When | Cost |
|---|---|---|
| `docling` | default self-host and cloud | CPU/GPU, $0 model |
| `digital_pdf` | text-layer PDFs, invoices from QuickBooks | tiny |
| `vlm_direct` | current behavior: whole file → Gemini | easy, expensive, no honest bboxes |
| `marker` / `mineru` | opt-in quality | GPU |

Cloud default: `docling` + `review` on low-confidence + Gemini Flash extract.  
OSS default: `docling` + BYO LLM for extract. `vlm_direct` remains for people who only have a Gemini key and no Python sidecar.

Compose profiles:

- `core` — api, worker, postgres, redis, minio (today)
- `parse` — adds `struxio-parse` (Docling + PaddleOCR)  
- `gpu` — vLLM / Ollama for local VLM review

The nerd can still run `vlm_direct` only. The product demo and cloud always parse.

---

## 5. Feature parity plan (OSS includes it)

| Reducto capability | OSS Struxio | Cloud extra | Phase |
|---|---|---|---|
| One-shot ingest + handle | yes | hosted storage | P0 |
| Parse → chunks/blocks/bboxes | yes (Docling) | GPU workers, faster queue | P1 |
| Extract + schema | yes | hosted LLM | P0 (today’s path) then P1 (from IR) |
| Citations + confidence | yes (from IR) | — | P1 |
| Reuse parse job for extract | yes | — | P1 |
| Split | yes | — | P2 |
| Classify | yes | — | P2 |
| Agentic review pass | yes (BYO VLM) | hosted VLM, billed | P2 |
| Deep extract | yes | billed multiplier | P2 |
| Batch + webhooks | yes | Svix-class delivery | P1 worker, P2 webhooks |
| MCP parse/extract/batch | yes | OAuth hosted URL | P1 |
| CLI | yes | — | P1 |
| Studio parse viewer + bbox | **yes, OSS** | login/billing chrome | P2 |
| Template studio | yes | — | P2 |
| Pipelines | yes (YAML + API) | Studio deploy | P3 |
| Edit / redact | yes | — | P3 |
| Generate / translate | maybe | — | later |
| SSO, ZDR, BAA, air-gap | no | enterprise | later |
| Custom fine-tunes | no | enterprise | later |

Langfuse lesson: **do not put parse or Studio behind a paywall.** That would kill the “open source alternative” sentence. Cloud sells GPUs, uptime, and “I didn’t docker-compose Docling.”

---

## 6. Cheaper than Reducto (on purpose)

Rough public numbers: Reducto extract ~2 credits/page at $0.015/credit ≈ **$0.03/page** plus parse. Extend is higher. Our COGS if we parse with Docling on our GPU/CPU and extract with Gemini Flash on markdown is often **$0.001–0.01/page**.

Pricing sketch (tune later, keep the story):

| | Self-host | Struxio Cloud | Reducto |
|---|---|---|---|
| Parse | your hardware | ~$0.005–0.01 / page | parse credits (often 1–4 / page) |
| Extract (after parse) | your LLM key | ~$0.01 / page | ~$0.03 / page |
| Agentic review | your VLM | extra credits | extra credits |
| Product | full | full + no ops | full + no ops + custom models |

Rules that keep us cheaper:

1. **Parse once.** Extract/split/classify take a `job_id`. Never re-OCR.
2. **Digital PDF short-circuit.** No PaddleOCR on a native-text invoice.
3. **Flash default.** Pro / agentic is a flag.
4. **Review only low-confidence blocks**, not every page.
5. **No custom-model R&D** to amortize. We ride Docling + frontier APIs.

Accuracy will lag Reducto on handwriting and nightmarish tables until the review pass is good. That is acceptable if we are honest, cheaper, and open. Improve backends without changing `/v1/parse`.

---

## 7. What we copy vs what we refuse

**Copy**

- Verb set: parse / extract / split / classify / jobs
- IR: chunks, blocks, normalized bboxes, confidence
- Parse-once handles (`struxio://job/{id}`)
- Citations as first-class extract output
- Studio overlay (the screenshot that sells enterprise later)
- MCP + CLI + SKILL.md as the on-ramp
- Agentic loop as optional multiplier
- `next_steps` on MCP results; schema as object or string

**Refuse**

- Their upload-then-parse MCP dance as the only path
- API-key-only hosted MCP (OAuth Connect for the builder)
- Training a 12-model zoo before we have parse IR
- Paywalling OSS parse quality
- Fake `confidenceScore` on the homepage until it comes from a block
- Claiming “we out-parse Reducto” until we publish a bake-off (RD-TableBench is public; we can run it)

---

## 8. Implementation slice that makes this real

After `workspace_id` + `POST /v1/extract` (RFC P0):

1. Define `struxio_common::parse::{Chunk, Block, BBox, ParseResult}` to match §2.
2. `ParseBackend` trait + `digital_pdf` (pdfium text + naive paragraphs) so `/v1/parse` exists **without** Python.
3. `docling` sidecar (`struxio-parse` Docker image) mapping Docling → IR.
4. Change extract to prefer `parse_job_id`; attach citations from block ids.
5. MCP tools `parse` + `extract` sharing that path.
6. Only then: split, classify, review pass, Studio overlay.

`digital_pdf` is a week. Docling sidecar is the quality jump. Review pass is the “agentic” headline. None of that requires raising a Series B.
