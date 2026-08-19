# Struxio rebuild RFC

**Status:** proposed  
**Audience:** maintainers deciding how to turn this engine into a product people pay for  
**Companion research:** four parallel reviews of the current code, MCP 2026, OSS+cloud comps, and the extract market; plus a feature/library map of Reducto in [rebuild-reducto-map.md](./rebuild-reducto-map.md).

This is the plan for the rebuild. Gemini-on-bytes already works as a shortcut. The product we are building is the **open-source Reducto**: parse → IR → extract/split/classify, with MCP and Studio, hosted or self-hosted.

---

## 1. Verdict

The thesis is right, and it is bigger than a hosted MCP wrapper.

**Struxio is the open-source Reducto**, the way Langfuse is the open-source LangSmith. Same platform category (parse, extract, split, classify, citations, Studio, MCP). Full product in OSS. Cloud is the same software with zero ops and a credit card. Cheaper, because we orchestrate open-source layout/OCR/VLMs instead of amortizing a custom model lab.

The person who deploys this on a VM will not pay. That is fine — and now even more important: OSS must be a **complete document platform**, not a teaser. If parse and Studio are cloud-only, the Langfuse sentence is a lie. AGPL is the lawyer that stops someone else from hosting our engine as a competing cloud. It is not a conversion funnel.

The person who pays is the product builder sitting in Cursor, Claude Code, or Codex. He wants Reducto’s outcome (JSON, citations, folders of invoices) without Reducto’s price or a 12-model zoo on his laptop. He will pay extra to skip Docling + PaddleOCR + Redis + GPUs.

Enterprise (SSO, VPC, DPA, SLA) is a later check. Do not build it first. Do not paint the schema into a corner that makes it impossible.

**Interfaces:** MCP is how agents enter. REST is how pipelines and CI enter. Studio is how humans trust the output (bbox overlay). All three sit on one parse IR.

The website already markets a one-shot extract (`documentUrl` + schema → JSON). That stays as the happy path. Underneath, the rebuild adds parse-then-extract so citations are real and large files do not go to Gemini as base64.

Feature and library map: [rebuild-reducto-map.md](./rebuild-reducto-map.md).

---

## 2. Who we are selling to

| Persona | What they do | Will they pay? | What we give them |
|---|---|---|---|
| **Nerd / self-hoster** | Docker Compose on a VM, BYO LLM, curl / MCP | No | Complete AGPL platform: parse, extract, citations, MCP, Studio. No dark patterns. |
| **Builder (primary buyer)** | Uses Cursor / Codex / Claude Code. Building a product. Hates extra ops. | Yes, gladly, for simplicity | Hosted MCP URL + credits. Sign in, click Add to Cursor, extract. |
| **Enterprise** | Security, procurement, volume | Yes, later, big check | Same cloud, plus SSO / VPC / DPA / SLA. Packaging, not a third engine. |

Do not design pricing, UX, or the first roadmap around the nerd. Do not feature-gate parse or Studio to “upsell” him — that would not be Langfuse. GitLab-style open-core (paywall the graphs) is the wrong model.

Closest analog: **Langfuse vs LangSmith** — same product, open and cheaper, self-host first-class, cloud for people who will not operate it. Plausible for the AGPL fence. Reducto for the feature list we implement with OSS libraries.

---

## 3. What we have today (honest)

Struxio is a thin, competent **Gemini structured-output wrapper** in Rust: Axum API, Postgres, Redis Streams worker, S3/MinIO, JSON Schema templates, batch jobs. About 2.8k lines. Crate split is good (`common → db → core → api/worker`). Sync extract of a small PDF with a valid schema works.

It is not a document-intelligence stack. There is no OCR pipeline, no citations, no bounding boxes, no parse-then-extract, no confidence scores in code (the homepage shows `confidenceScore` anyway), no tests, no app Dockerfile, no MCP, no web app, no multi-tenant isolation.

### Already anticipated, then stripped

`CONTRIBUTING.md` already names `struxio-cloud` (Clerk, Stripe) and `struxio-web` (dashboard). Those repos do not exist. Commit `0f4bd17` removed usage endpoints, `STRUXIO_ORG_ID`, and the Self-Hosting vs Cloud section from the README. Fossils remain:

- `oss_router()` implies a cloud router sibling
- `AuthUser` is a unit struct; comments mention Clerk injecting it via extensions
- queue jobs carry `org_id` (OSS passes `Uuid::nil()`)
- `extractions.credits_charged` and `ai_models.credit_cost_per_page` exist; charges are hardcoded to `0`
- `rate_limit.rs` is a comment that cloud will add plan limits
- `documents.md5_hash` is **globally UNIQUE** — two cloud customers cannot share a database

### Docs, license, and marketing disagree with the code

| Claim | Reality |
|---|---|
| README / CONTRIBUTING: **AGPL-3.0** | `LICENSE` is **Apache-2.0**. GitHub reports `NOASSERTION`. This is a launch blocker. |
| Homepage: `POST /api/v1/extract` with `documentUrl` + inline schema + confidence | No such route. No confidence field. |
| Homepage: “comprehensive SDKs” | None exist. |
| README: `POST /v1/extractions` then poll | Handler calls `create_sync` — the HTTP request blocks on Gemini. |
| README example schema `{"total":"number"}` | Template validation requires `type` + `properties`. The example fails. |
| `file_type: "application/pdf"` in README | Gemini mime mapper only understands `pdf`, `png`, … — `application/pdf` becomes `application/octet-stream`. |
| “Docker-ready” | Compose starts Postgres, Redis, MinIO. API and worker are `cargo run`. MinIO healthcheck (`mc ready local`) is broken. |
| Default model | Config defaults to `gemini-2.5-pro`; seed only inserts `gemini-2.5-flash`. Unset `GEMINI_MODEL` can 500 on first extract. |
| Security contact | `security@struxio.com` (CONTRIBUTING) vs `security@struxio.app` (SECURITY.md) |

### Engine bugs that will bite paying users

Worker is serial (`COUNT 1`), ACKs failures with no retry, never marks a batch `partially_completed` / `failed` (those enums exist, unused). Failed Gemini still returns HTTP 200 with `status: failed`. Confirm-upload does not check that the S3 object exists. Entire files go to Gemini as JSON base64 (large PDFs will die). Lists have no pagination. Inline extract uploads then re-downloads the same bytes. No request size limit on base64. CORS default is `*`.

**Grade:** crate layout and small-file sync extract are almost good. Queue, Gemini-at-scale, docs, license, agent UX, and cloud tenancy are not ready.

---

## 4. Competitive position (be the open Reducto)

Reducto is the **reference platform**, not a rival we politely avoid. We implement their verbs and IR with open libraries; we do not train their 12-model zoo. Accuracy will lag on handwriting and nightmare tables until the review pass is good. That is the Langfuse bargain: same job, honest, cheaper, you can run it.

| Player | What they are | Implication |
|---|---|---|
| **Reducto** | Closed agentic document platform; parse/extract/split/classify/edit; Studio; MCP; ~$0.03/page extract | **Product spec.** Copy verbs, IR, citations, MCP, Studio. Beat them on price, openness, and friction. |
| **Extend, LlamaExtract, Landing ADE** | Same closed category | Same story. We are the OSS alternative to the whole class. |
| **Textract / Document AI / Azure DI** | Cloud-console IDP | Different buyer. Nested JSON Schema + agents. |
| **Docling, MinerU, Marker/Surya, PaddleOCR** | OSS parse/OCR engines | **Backends we wrap**, not competitors. We own the IR, API, MCP, Studio, jobs. |
| **Kreuzberg** | Apache Rust library + MCP | Library threat. We are the product (hosted, templates, pipelines, credits). Can also be a `ParseBackend`. |

Gemini Flash on a typical invoice is roughly **$0.001–0.01/page**. Reducto extract is ~$0.03/page plus parse. Parse-once + Flash-on-markdown + OCR only when needed is how we stay cheaper without lying about quality.

**Promise:** the Reducto-shaped platform, open and cheaper.  
**Anti-promise:** we will not claim SOTA vs Reducto until we publish a bake-off (RD-TableBench is public). We will not paywall parse or Studio.

---

## 5. Product architecture

Four planes. We sell hosted ops. The document platform itself is OSS (Langfuse rule).

```
┌─────────────────────────────────────────────────────────────────┐
│  Agents: Cursor, Claude Code, Codex, anything MCP               │
│  Primary UX: hosted MCP URL + optional desktop bridge           │
└────────────────────────────┬────────────────────────────────────┘
                             │ Streamable HTTP MCP  or  REST
┌────────────────────────────▼────────────────────────────────────┐
│  STUDIO  — OSS UI (bbox overlay, parse viewer, extract, keys)   │
│  Cloud chrome: login, Stripe, Add to Cursor                     │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│  CONTROL PLANE  — proprietary (struxio-cloud)                   │
│  Workspaces, hashed keys, Stripe, hosted VLMs, OAuth MCP        │
└────────────────────────────┬────────────────────────────────────┘
                             │ workspace_id
┌────────────────────────────▼────────────────────────────────────┐
│  ENGINE  — AGPL (this repo)                                     │
│  ingest → parse (IR) → review → extract / split / classify      │
│  REST /v1  ·  MCP  ·  CLI  ·  worker  ·  parse sidecar          │
└─────────────────────────────────────────────────────────────────┘
```

Parse-then-extract is the spine. Current Gemini-on-bytes remains `parser = "vlm_direct"` for tiny files. Default path: Docling (or digital pdfium) → Struxio IR (chunks/blocks/bboxes) → schema extract with citations. Details and library map: [rebuild-reducto-map.md](./rebuild-reducto-map.md).

### What “Struxio Web” is

Two layers, like Langfuse’s UI vs their cloud login:

1. **OSS Studio** — upload, parse viewer, bbox overlay on the page, extract playground, job history. This is how you prove we are a Reducto alternative, not a wrapper.
2. **Cloud chrome** — GitHub/Google login, Stripe, Add to Cursor, usage. Proprietary is fine.

Week-1 cloud can ship the cash-register page first (remaining pages + Add to Cursor + one sample). Studio overlay is P2, but it is **in OSS**, not an EE teaser.

Marketing site: sell “open-source Reducto” + Cursor path. Self-host is not a footer apology; it is the Langfuse half of the story.

### What the OSS engine must stay

A **complete document platform**: parse, extract with citations, split, classify, MCP, CLI. If self-host cannot parse an invoice to blocks and extract a schema, GitHub is a lie. OSS does not need Clerk or Stripe. It does need the parse sidecar (or `vlm_direct` fallback) and Studio.

Cloud talks to the engine as a **library** (`oss_router()` + `AppState` already exist), not as a rewrite, and not as “one shared database with a static key in front.”

---

## 6. MCP is the product

Agents will not do: MD5 → presigned PUT → confirm → create template UUID → extract. That dance stays on REST for large-file power users. MCP never exposes it.

### Two deployments, same tools

| | OSS (free) | Cloud (paid) |
|---|---|---|
| How you connect | `struxio-mcp` stdio, BYO Gemini | `https://mcp.struxio.dev/mcp` |
| Files | local `path` / `glob` | `url` / small `file_base64` / desktop bridge |
| Auth | env key | OAuth (Connect button) + Bearer API key fallback |
| Billing | none | credits per page |
| Ops | optional Compose; **lite mode needs no Redis/Postgres/MinIO** | we run it |

Optional **cloud stdio bridge** (`npx @struxio/mcp`): reads the laptop disk, uploads, calls hosted extract. That is how “this folder of invoices” works without the user running our stack. `mcp.json` contains no secrets (device-code / OAuth login).

Copy Linear / LlamaParse for connect UX (one URL, Connect, done). Copy Reducto for `next_steps`, schema-as-object-or-string, truncation + `get_job`, parse/extract/split/classify verbs, and local stdio that can read disk. Copy Kreuzberg for `glob` batch. Do **not** copy Reducto’s required `upload_file` hop, API-key-first hosted MCP, Unstructured’s workflow-CRUD tools, or our own three-step S3.

### Tool surface

Match Reducto’s verbs. Keep upload implicit.

| Tool | Job |
|---|---|
| `parse` | File → chunks/blocks/bboxes. Default for RAG / inspection |
| `extract` | Schema or `template_id` → JSON (+ citations). Parses internally if no `job_id` |
| `extract_batch` | Glob / paths / urls. One call, not a loop |
| `split` | NL sections → page ranges / subdocs |
| `classify` | NL taxonomy → label |
| `suggest_schema` | Description or sample parse → JSON Schema |
| `list_templates` / `get_template` / `create_template` / `update_template` | System + user (`invoice`, `receipt` already seeded) |
| `get_job` | Poll; page large parse/extract results |
| `get_account` | Cloud only: credits remaining, top-up URL |

v1 can ship `parse` + `extract` + `extract_batch` + templates + `get_job`. Split/classify follow once IR exists. Edit stays off MCP until an edit engine exists.

**Never MCP-expose:** `documents/check`, `confirm`, `upload_url`, `s3_key`, `md5_hash`, Redis IDs. Parse is not a required *user-facing* hop — `extract` may parse internally — but `parse` is a first-class tool, not a hidden implementation detail.

System instructions for the server should be short:

> Struxio parses documents into a layout tree, then extracts JSON. For RAG or inspection call `parse`. For fields call `extract` (it will parse if needed). For a folder call `extract_batch` once. Prefer a saved template. Never ask the user for S3 URLs. If status is pending, call `get_job`.

### File inputs (MCP has no binary file type)

| Input | Where it works | Notes |
|---|---|---|
| `path` / `glob` | stdio and cloud bridge only | Hosted HTTP rejects `path` with a message that names url / base64 / bridge — not “use S3” |
| `url` | both | HTTPS only. SSRF allowlist: no RFC1918, link-local, metadata IPs |
| `file_base64` | both, cap ~8 MiB decoded | Fine for one invoice in chat; fatal for 80 PDFs |
| `document_id` / `struxio://doc/…` | after ingest | Opaque handles so the model can thread state |

Hosted MCP without the bridge cannot read Cursor’s disk. Document that. For many local files, install the bridge or pass HTTPS URLs.

### Protocol

- **stdio** for local. Logs on stderr only (stdout is the protocol).
- **Streamable HTTP** at `/mcp` for cloud. Implement MCP **2026-07-28** (stateless, `Mcp-Method` / `Mcp-Name` headers) and stay compatible with **2025-11-25** (`initialize`, optional session) because Cursor / Claude / Codex will lag.
- No sticky MCP sessions. Job state lives in Postgres (cloud) or sqlite/files (OSS lite).
- Long batches: return `job_id` immediately; use MCP Tasks when advertised; always keep `get_job` for everyone else.
- Auth for remote: OAuth 2.1 + protected-resource metadata (RFC 9728) + PKCE. Bearer `sk_live_…` on the same `/mcp` for CI and clients without OAuth.
- Rate-limit on `Mcp-Name` without parsing the body.

### Zero to first extraction (the whole funnel)

1. Sign in with GitHub (or skip ahead via Marketplace).
2. Click **Add to Cursor** → `https://mcp.struxio.dev/mcp` → Connect (browser OAuth).
3. Signup gift of free pages so the next step works without a card.
4. Drag `invoice.pdf` into chat: *“Extract vendor, invoice number, date, line items, and total.”*
5. Agent uses system template `invoice` or `suggest_schema`, then `extract`. JSON in the thread.
6. At zero pages: Stripe. Agent error includes `insufficient_credits` and `top_up_url`.

Do not lead with REST docs, Docker, or API keys. Keys are CI / fallback.

Folder of invoices:

1. `list_templates` (or `suggest_schema` on one sample + `save_as`)
2. **One** `extract_batch({ glob: "./invoices/**/*.pdf", template_id: "invoice" })`
3. `get_job` until terminal
4. Write `invoices.json` (and never dump 200 full results into the model context — page via `get_job` / `struxio://jobs/{id}/result`)

---

## 7. Engine changes this repo must make

MCP and cloud sit on top of a slightly different engine than we have now. This is still a rebuild of the product, not a greenfield Gemini client.

### Keep

- Crate boundaries: business logic in `core`, thin `api`, repos in `db`
- Queue producer/consumer traits (harden the Redis impl; do not build Kafka)
- S3 presign for **large** files (not as the agent default)
- Templates: schema + prompt + system seeds (Invoice, Receipt — add slugs)
- Gemini `responseSchema` structured output (extract stage; also `vlm_direct` fallback)
- Sync path for small interactive extracts; batch + worker for volume
- Static API key for self-host

### Throw away or quarantine

- Fake concurrency / Kafka promises until they exist
- `oss_router` naming (just `router`)
- Clerk / `_require_role` theater in OSS
- Credits columns **as cloud-only concern** — either wire them honestly behind a workspace or move them out of the OSS schema. Do not leave `credits_charged = 0` forever
- Homepage `confidenceScore` until it comes from parse blocks (see IR in the Reducto map)

### Must land before a second paying tenant

1. **`workspace_id`** on documents, templates, extractions, batch_jobs. Default nil UUID in OSS. Unique `(workspace_id, md5_hash)` instead of global unique hash. S3 keys prefixed `/{workspace_id}/`. `AuthUser` becomes `{ workspace_id }` (OSS fills default). Queue already has `org_id` — use it as workspace id. **Skipping this is the 6-month rewrite.**
2. **`POST /v1/extract`** — one-shot: `file_base64` | `file_url` + `schema` or `template_id`. Homepage + MCP wrap this. Keep `/v1/extractions/inline` as an alias if needed.
3. **Parse IR + `POST /v1/parse`** — chunks/blocks/normalized bboxes. `ParseBackend` trait; `digital_pdf` first, Docling sidecar next. Extract prefers `parse_job_id` and emits citations. See [rebuild-reducto-map.md](./rebuild-reducto-map.md).
4. **MIME normalization** — store real MIME; accept `pdf` and `application/pdf`.
5. **Page count** — stop writing `page_count = 1`. Metering depends on it.
6. **Gemini / LLM production path** — timeouts, retries, model allowlist; extract from IR rather than whole-file base64 when parse exists.
7. **Worker** — concurrency limit, retries / DLQ, PEL reclaim, stream trim, honest batch terminal states, set `model_id`.
8. **Pagination** on every list. Failed sync extract must not look like HTTP success to naive clients (explicit envelope or non-200).
9. **Confirm verifies the object.** Unique violations → 409. Transactional batch create.
10. **Packaging** — Dockerfile (api / worker / mcp / parse sidecar), Compose profiles `core` + `parse`, working MinIO healthcheck.
11. **Tests + CI** — service, mime, queue contract, parse IR fixtures, MCP JSON-RPC transcripts (no live Gemini required).
12. **Observability** — request ids, parse/LLM latency, tokens, queue lag.
13. **Security** — body size limits, CORS not `*` by default in cloud, one security mailbox.

### Suggested crate layout

```
crates/
  common/     models, config, errors, parse IR (Chunk, Block, BBox)
  db/         repositories
  core/       ingest, parse (ParseBackend), review pass, extract, split, classify, jobs, storage
  api/        REST: /v1/parse, /extract, /split, /classify, jobs
  mcp/        MCP server; calls core
  worker/     job runner
  parse-sidecar (optional image)  Docling + PaddleOCR
```

`core` needs two provider traits: **LLM** (Gemini today) and **ParseBackend** (digital pdfium → Docling → Marker). Quality improves by swapping backends, not by changing the API.

OSS **lite**: in-process `vlm_direct` extract, no sidecar. OSS **full**: Compose `parse` profile. Cloud: always full.

---

## 8. Repos, license, packaging

Keep the split `CONTRIBUTING.md` already states. MCP is a **surface**, not a fourth brand.

| Repo | Visibility | License | Contains |
|---|---|---|---|
| **`struxio`** (this) | Public | **AGPL-3.0-only** (fix the Apache file) | Engine, parse sidecar, REST, worker, MCP, CLI, Compose, `SKILL.md` |
| **`struxio-cloud`** | Private | Proprietary | Multi-tenant gateway, keys, Stripe, hosted `/mcp`, OAuth, model-key vault, rate limits |
| **`struxio-web`** | Public Studio; private billing chrome OK | AGPL or MIT for Studio | Parse viewer, bbox overlay, extract playground. Cloud repo adds login/Stripe |
| **Cursor plugin / `@struxio/mcp` proxy** | Public | **Apache-2.0 or MIT** | Thin client that talks HTTP to OSS or cloud. Corporate legal will vendor this. |

Do not put Stripe in the public repo. Do not put a private `ee/` folder in the public tree (leak + contributor confusion). Cloud path-depends on these crates (`oss_router` nested at `/v1`).

**CLA on day one** if we want a later commercial AGPL exception (SaaS embedders who cannot accept AGPL). Without a CLA, dual-license is painful.

Do **not** move to FSL/BSL yet. “Open source” is the nerd funnel and the HN sentence. Revisit only if a well-funded wrapper appears.

**Immediate legal fix (before any cloud launch):** replace `LICENSE` with AGPL-3.0 text, set `Cargo.toml` `license = "AGPL-3.0-only"`, pick one security email (`security@struxio.dev`), stop claiming SDKs and a fake extract route.

### Three-tier packaging

| | OSS | Cloud | Enterprise (later) |
|---|---|---|---|
| Price | $0 | Usage + simple monthly | Quote |
| Platform | Parse, extract, split, classify, Studio, MCP | Same engine, hosted workers | Same engine |
| Models | BYO LLM + local Docling | Hosted parse + hosted VLMs (the SKU) | Dedicated / custom |
| MCP | stdio / local HTTP | Hosted URL + OAuth | Private URL / VPC |
| Auth | Static key | GitHub/Google + hashed keys | SSO / SAML |
| Support | GitHub | Email | Slack + SLA |

Enterprise is config and contracts on the control plane, not a new extractor. Public price page: OSS (free) and Cloud. Enterprise is a sentence and `sales@`.

---

## 9. Pricing (matches the buyer)

Do not charge per seat. This buyer has one human and twenty agents.

**Unit:** page. Internal ledger: credits. `credit_cost_per_page` already exists — e.g. Flash = 1 credit, Pro = 4.

Sketch (tune with real COGS; do not race to $0.001):

| | Free | Cloud | Enterprise |
|---|---|---|---|
| Price | $0 | **~$29/mo** | Custom |
| Included | ~200 pages/mo | ~2,000 pages/mo | Committed volume |
| Overage | Hard stop (402) | ~$0.02/page | Volume rate |
| Card | Not required | Required for overage | Net-30 |

Why a monthly number plus included pages, not pure PAYG: the buyer wants something he can expense without thinking. $29 is cheaper than one hour of his time. Included pages prevent a surprise $4 bill; overage captures the 2,000-invoice folder.

Signup gift (~200 pages ≈ ~$1 Gemini) is CAC, not a cost center. Reducto/Unstructured 15k free pages are enterprise evals we cannot afford until billing works.

Every MCP / HTTP error at the cap: `insufficient_credits`, remaining, `top_up_url`. Charge on success; idempotency key `account_id + job_id` so retries do not double-bill. Do not charge failed Gemini timeouts.

---

## 10. Web structure

`struxio-web` (Next.js unless we already have a marketing stack):

```
/                 marketing — “open-source Reducto” + Add to Cursor
/docs             parse / extract / MCP; bury the S3 three-step
/login            GitHub / Google (cloud)
/app              remaining pages, Add to Cursor, try-it
/app/parse        Studio: document + bbox overlay (OSS)
/app/extract      schema playground + citations
/app/templates    templates
/app/jobs         history
/app/keys         API keys
/app/billing      Stripe (cloud)
```

Week-1 cloud may be only `/app` cash-register. Parse overlay is P2 and still lands in the OSS Studio, not an EE screenshot.

---

## 11. Sequencing (take money without a platform rewrite)

Constraint: the engine already has cloud seams. Use them. Do not rewrite Gemini into a new language.

### P0 — Fence and honesty (days, not months)

- [ ] License: Apache file vs AGPL claims — pick AGPL-3.0 and make the file match
- [ ] Homepage / README: stop claiming SDKs and `/api/v1/extract` until they exist
- [ ] `workspace_id` migration + composite unique hash
- [ ] `POST /v1/extract` (file + schema or template)
- [ ] Count pages; decide how credits are recorded
- [ ] One security mailbox
- [ ] Sketch parse IR types (`Chunk`, `Block`, `BBox`) even if `/v1/parse` is the next PR

Do not start Stripe before `workspace_id` exists.

### P1 — Thinnest chargeable cloud

- [ ] Deploy current API + worker with **our** Gemini key, Postgres, Redis, S3
- [ ] `struxio-web`: GitHub OAuth → workspace → hashed API key → Stripe ($29 or page pack)
- [ ] Meter remaining pages; 402 when empty
- [ ] Hosted MCP with **two tools first**: `extract`, `list_templates` (add `parse` in P2). Bearer key is enough for week 1; OAuth is week 2–4
- [ ] Post-login: Add to Cursor + remaining pages

**Skip in P1:** Clerk orgs, team invites, Kafka, custom models, hand-written SDKs, SSO, HITL, a new queue.

### P2 — Document platform (the Reducto shape)

- [ ] `POST /v1/parse` + `digital_pdf` backend (pdfium / text layer)
- [ ] Docling parse sidecar; map to IR
- [ ] Extract from `parse_job_id` with citations
- [ ] MCP tools `parse` + `extract` + `extract_batch`
- [ ] Cursor deeplink + `SKILL.md` + invoice-folder demo
- [ ] Flash as default hosted model
- [ ] Dockerfile + Compose `core` and `parse`
- [ ] Tests for IR fixtures + MCP transcripts

### P3 — Parity loop

- [ ] OAuth for MCP; desktop bridge (`npx @struxio/mcp`)
- [ ] Split + classify
- [ ] Agentic review pass on low-confidence blocks (BYO VLM)
- [ ] OSS Studio bbox overlay
- [ ] Template studio + webhooks
- [ ] Worker reliability (retries, concurrency, honest batch status)
- [ ] OpenAPI from Axum; CLI `struxio parse|extract`

### Explicitly later (when someone asks and will pay)

Edit / redact / generate / translate, SSO, audit export, VPC, DPA, SLA, commercial AGPL exception, HITL queues, custom fine-tunes, Kafka, claiming SOTA vs Reducto without a published bake-off.

**One-sentence definition of done for “rebuild v1”:** GitHub login → Add to Cursor → agent parses/extracts a PDF against a schema → Stripe when free pages run out.

**Definition of done for “open-source Reducto”:** self-host Compose `parse` profile can parse to bboxes, extract with citations, and inspect the result in OSS Studio — no Struxio account.

---

## 12. What we will not do

- Rewrite the **control plane** in Python. Rust orchestrates; Docling/PaddleOCR stay sidecars.
- Feature-gate parse, extract, citations, or Studio in OSS. That would not be Langfuse; it would be a demo.
- Train a custom 12-model zoo before the IR and APIs exist.
- Require agents to call check/confirm/upload before parse.
- Optimize for enterprise procurement before a builder has paid $29.
- Claim we out-parse Reducto until we publish numbers.

---

## 13. Open decisions (do not block P0)

1. **CLA vendor** (GitHub CLA bot vs EasyCLA) — needed before outside PRs if we want relicensing rights.
2. **Auth provider for cloud** — GitHub OAuth in Next.js is enough for P1; Clerk is optional later (the comment in `static_auth.rs` is not a requirement).
3. **Hosted MCP OAuth in week 1 vs bearer-only** — bearer ships faster; OAuth is the businessman UX. Prefer OAuth as soon as Cursor Connect works; do not delay Stripe for it.
4. **Where templates live in OSS lite** — sqlite vs JSON file in `~/.struxio/`.
5. **Commercial license text** — can wait until the first embedder asks.
6. **EU MCP region** — wait for a customer.
7. **Default parse backend** — Docling sidecar vs `digital_pdf` only until the sidecar ships. See the library map.

---

## 14. Why this is a rebuild, not a polish

The current app is “almost good” as a **Gemini extract skeleton**. It is not a document platform.

A polish would add docs and a Dockerfile and still be a wrapper. Reducto (and Langfuse, from the other side) show the bar: parse tree, citations, split/classify, Studio, MCP, self-host or cloud.

A rebuild keeps the Rust engine and Gemini extract, and changes the center of gravity:

- **Category:** open-source Reducto (Langfuse-style), not “thin extract API”
- **Spine:** parse IR → extract/split/classify (Gemini-on-bytes is a backend, not the product)
- **Interface:** MCP + REST + OSS Studio
- **Buyer:** zero-ops builder; nerd gets the full platform for $0
- **Packaging:** AGPL engine + OSS Studio + hosted GPUs/credits
- **Schema:** workspace isolation before the second customer
- **Cost:** parse once, OCR only when needed, Flash default

Next implementation PR: P0 (`workspace_id` + `/v1/extract` + license honesty), then parse IR + `/v1/parse`, not a greenfield monorepo.
