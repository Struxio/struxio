# Struxio rebuild RFC

**Status:** proposed  
**Audience:** maintainers deciding how to turn this engine into a product people pay for  
**Companion research:** four parallel reviews of the current code, MCP 2026, OSS+cloud comps, and the extract market (Reducto, Extend, LlamaExtract, Kreuzberg, Docling, cloud IDPs)

This is the plan for the rebuild. It is not a rewrite of Gemini extraction from scratch. The extractor already works. The product around it does not.

---

## 1. Verdict

The thesis is right.

The person who deploys this on a VM, sets `STRUXIO_API_KEY`, and calls the REST API will not pay. That is fine. Open source is for them: a fence, a trust signal, and a distribution channel. AGPL is the lawyer that stops someone else from hosting our engine as a competing cloud without sharing changes. It is not a conversion funnel.

The person who pays is the product builder sitting in Cursor, Claude Code, or Codex. He wants structured data out of a folder of invoices. He does not want Postgres, Redis, MinIO, Gemini keys, or a three-step presigned S3 upload. He will pay extra to skip that work.

Enterprise (SSO, VPC, DPA, SLA) is a later check. Do not build it first. Do not paint the schema into a corner that makes it impossible.

**The main product is not the REST API.** The main product is an MCP server that extracts file data — one file or a batch — for any agent that speaks MCP. REST stays as the power-user / CI / self-host interface. The web app (`struxio-web`) is the cash register, the template studio, and the “Add to Cursor” button. It is not the thing agents use.

The website already markets a one-shot extract (`documentUrl` + schema → JSON). The code does not expose that shape. Close that gap and the rebuild has a spine.

---

## 2. Who we are selling to

| Persona | What they do | Will they pay? | What we give them |
|---|---|---|---|
| **Nerd / self-hoster** | Docker Compose on a VM, BYO Gemini key, curl / scripts | No | Complete AGPL engine + local MCP. Full extractor. No dark patterns. |
| **Builder (primary buyer)** | Uses Cursor / Codex / Claude Code. Building a product. Hates extra ops. | Yes, gladly, for simplicity | Hosted MCP URL + credits. Sign in, click Add to Cursor, extract. |
| **Enterprise** | Security, procurement, volume | Yes, later, big check | Same cloud, plus SSO / VPC / DPA / SLA. Packaging, not a third engine. |

Do not design pricing, UX, or the first roadmap around the nerd. Do not feature-gate extraction quality to “upsell” him. He will fork Gemini himself. GitLab-style open-core (paywall the graphs) is the wrong model for this category.

Closest analogs: Plausible (AGPL CE + hosted convenience is the business) mixed with LlamaParse / Reducto (hosted MCP + free pages is how agents convert).

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

## 4. Competitive position (do not fight the wrong war)

| Player | What they are | Implication |
|---|---|---|
| **Reducto, Extend, LlamaExtract, Landing ADE** | Closed agentic IDP: parse, extract, citations, Studio, evals, MCP | They win bake-offs on hard docs. Do not try to out-parse them in year one. |
| **Textract / Document AI / Azure DI** | Cloud-console IDP, training or shallow query fields | Different buyer. Our wedge is nested JSON Schema inside an agent loop. |
| **MinerU, Docling, Marker** | OSS parsers → markdown / layout trees | Complement later as a parse layer. They win RAG; we win typed extract. |
| **Kreuzberg** | Apache, Rust, MCP, `extract_structured`, 100 formats | Real OSS threat. They are a **library**. We must be a **product**: hosted MCP, templates, batch, Cursor packaging, credits. |

**Where a small AGPL + cloud player wins in 2026:** the agent-runtime slot. “Drop files into Cursor, get JSON.” Not the AP-automation RFP.

Gemini Flash on a typical invoice is roughly **$0.001–0.01/page**. Reducto extract is ~$0.03/page plus parse; Extend ~$0.06/page. A thin hosted wrapper at Gemini cost + markup is a 5–30× price story for the 80% of docs Gemini already handles (invoices, receipts, simple forms). The businessman is not shopping $0.001. He is shopping time-to-working-agent.

**Anti-promise:** we will not out-parse Reducto this year. Citations before bounding boxes. Schema-in-request before a Studio. MCP before evals. Wrap a parser before building OCR.

---

## 5. Product architecture

Three planes. Only the control plane is what we sell this month.

```
┌─────────────────────────────────────────────────────────────────┐
│  Agents: Cursor, Claude Code, Codex, anything MCP               │
│  Primary UX: hosted MCP URL + optional desktop bridge           │
└────────────────────────────┬────────────────────────────────────┘
                             │ Streamable HTTP MCP  or  REST
┌────────────────────────────▼────────────────────────────────────┐
│  CONTROL PLANE  — proprietary (struxio-cloud + struxio-web)     │
│  Login, workspaces, API keys, Stripe credits, hosted models,    │
│  hosted MCP (OAuth + bearer), “Add to Cursor”, job history      │
└────────────────────────────┬────────────────────────────────────┘
                             │ workspace_id + auth context
┌────────────────────────────▼────────────────────────────────────┐
│  ENGINE  — AGPL (this repo)                                     │
│  ingest · templates · extract · batch · worker · storage        │
│  REST /v1  ·  stdio MCP (self-host / lite)                      │
└─────────────────────────────────────────────────────────────────┘
```

### What “Struxio Web” is

The rebuild should include a web app (`struxio-web`). It is not a document OS and not a human-in-the-loop review product (that is Extend’s $500/mo motion).

**Must exist to take money:**

- GitHub / Google login
- One workspace per user
- API key create / revoke
- **Add to Cursor** (and copy-paste for Codex / Claude Code)
- Credit balance + Stripe
- One “try it” upload that hits the same one-shot extract the MCP uses

**Soon after first dollar:** template studio (schema + prompt + live sample), job history, usage chart.

Marketing can live in the same Next.js app. Homepage should sell the Cursor path. Self-host is a footer link to GitHub. Kill fake SDK claims until SDKs exist.

### What the OSS engine must stay

A **complete extractor**. If self-host cannot extract an invoice, GitHub is a lie. OSS does not need Clerk, Stripe, or a pretty UI. It does need MCP (stdio + optional local HTTP) so the nerd’s agent works without our cloud.

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

Copy Linear / LlamaParse for connect UX (one URL, Connect, done). Copy Reducto for `next_steps` on every tool result, schema-as-object-or-string (clients mangle nested JSON), and truncation + `get_job`. Copy Kreuzberg for `glob` batch. Do **not** copy Reducto’s API-key-first hosted MCP, Unstructured’s workflow-CRUD tools, or our own three-step S3.

### Tool surface (keep it small)

Fat tool lists make agents waffle. v1:

| Tool | Job |
|---|---|
| `extract` | One file + `template_id` **or** inline `schema` → JSON |
| `extract_batch` | Glob / paths / urls / document ids, **one** call, not a loop of `extract` |
| `suggest_schema` | NL description (+ optional sample file) → JSON Schema + prompt |
| `list_templates` / `get_template` | System + user templates (`invoice`, `receipt` already seeded) |
| `create_template` / `update_template` | Save a schema so the agent stops reinventing it |
| `get_job` | Poll; page batch items. Also support MCP Tasks when the client advertises them |
| `get_account` | Cloud only: credits remaining, top-up URL |

**Never MCP-expose:** `documents/check`, `confirm`, `upload_url`, `s3_key`, `md5_hash`, Redis IDs, parse-as-required-prelude, classify/split/edit.

System instructions for the server should be short:

> Extract structured JSON from PDFs and images. Prefer a saved template. For one file call `extract`. For a folder call `extract_batch` once. Never ask the user for S3 URLs. If status is pending, call `get_job`.

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
- Gemini `responseSchema` structured output
- Sync path for small interactive extracts; batch + worker for volume
- Static API key for self-host

### Throw away or quarantine

- Fake concurrency / Kafka promises until they exist
- `oss_router` naming (just `router`)
- Clerk / `_require_role` theater in OSS
- Credits columns **as cloud-only concern** — either wire them honestly behind a workspace or move them out of the OSS schema. Do not leave `credits_charged = 0` forever
- Homepage `confidenceScore` until we compute something real (page-level `sources` is the honest first quality signal)

### Must land before a second paying tenant

1. **`workspace_id`** on documents, templates, extractions, batch_jobs. Default nil UUID in OSS. Unique `(workspace_id, md5_hash)` instead of global unique hash. S3 keys prefixed `/{workspace_id}/`. `AuthUser` becomes `{ workspace_id }` (OSS fills default). Queue already has `org_id` — use it as workspace id. **Skipping this is the 6-month rewrite.**
2. **`POST /v1/extract`** — one-shot: `file_base64` | `file_url` + `schema` or `template_id`. This is what the homepage already shows and what MCP wraps. Keep `/v1/extractions/inline` as an alias if needed.
3. **MIME normalization** — store real MIME; accept `pdf` and `application/pdf`.
4. **Page count** — stop writing `page_count = 1`. Metering depends on it.
5. **Gemini production path** — size threshold, timeouts, retries, Files API or equivalent for large docs, model allowlist matching the seed.
6. **Worker** — concurrency limit, retries / DLQ, PEL reclaim, stream trim, honest batch terminal states, set `model_id`.
7. **Pagination** on every list. Failed sync extract must not look like HTTP success to naive clients (explicit envelope or non-200).
8. **Confirm verifies the object.** Unique violations → 409. Transactional batch create.
9. **Packaging** — Dockerfile (api / worker / mcp), Compose profiles that actually run the app, working MinIO healthcheck.
10. **Tests + CI** — service, mime, queue contract, MCP JSON-RPC transcripts with fixture PDFs (no live Gemini required).
11. **Observability** — request ids, Gemini latency/tokens, queue lag.
12. **Security** — body size limits, CORS not `*` by default in cloud, one security mailbox.

### Suggested crate layout

```
crates/
  common/     models, config, errors (principal with workspace_id)
  db/         repositories
  core/       extract (provider trait), ingest, templates, jobs, storage
  api/        REST (secondary interface)
  mcp/        NEW — primary agent interface; calls core, not HTTP-to-self
  worker/     job runner
```

`core` should grow a **provider trait** (`GeminiClient` behind it) so a second model is a weekend, not a rewrite. Do not multi-model as a quality strategy; do it as BYOK / outage insurance.

OSS **lite MCP**: in-process Gemini, templates on disk or sqlite, tokio semaphore for batch. No Compose required. That is the nerd’s “it just works” and a test harness for tools.

---

## 8. Repos, license, packaging

Keep the split `CONTRIBUTING.md` already states. MCP is a **surface**, not a fourth brand.

| Repo | Visibility | License | Contains |
|---|---|---|---|
| **`struxio`** (this) | Public | **AGPL-3.0-only** (fix the Apache file) | Engine, REST, worker, Compose, Docker, stdio MCP, `SKILL.md` |
| **`struxio-cloud`** | Private | Proprietary | Multi-tenant gateway, keys, Stripe, hosted `/mcp`, OAuth, model-key vault, rate limits |
| **`struxio-web`** | Private app; marketing may be public | Proprietary | Signup, studio, keys, billing, Add to Cursor |
| **Cursor plugin / `@struxio/mcp` proxy** | Public | **Apache-2.0 or MIT** | Thin client that talks HTTP to OSS or cloud. Corporate legal will vendor this. |

Do not put Stripe in the public repo. Do not put a private `ee/` folder in the public tree (leak + contributor confusion). Cloud path-depends on these crates (`oss_router` nested at `/v1`).

**CLA on day one** if we want a later commercial AGPL exception (SaaS embedders who cannot accept AGPL). Without a CLA, dual-license is painful.

Do **not** move to FSL/BSL yet. “Open source” is the nerd funnel and the HN sentence. Revisit only if a well-funded wrapper appears.

**Immediate legal fix (before any cloud launch):** replace `LICENSE` with AGPL-3.0 text, set `Cargo.toml` `license = "AGPL-3.0-only"`, pick one security email (`security@struxio.dev`), stop claiming SDKs and a fake extract route.

### Three-tier packaging

| | OSS | Cloud | Enterprise (later) |
|---|---|---|---|
| Price | $0 | Usage + simple monthly | Quote |
| Extractor | Complete | Same engine | Same engine |
| Models | BYO Gemini | Hosted (this is the SKU) | Dedicated / custom |
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

`struxio-web` (Next.js is the default unless we already have a marketing stack to reuse):

```
/                 marketing — Cursor path first, GitHub second
/docs             honest API + MCP install; bury the S3 three-step
/login            GitHub / Google
/app              remaining pages, Add to Cursor, try-it upload
/app/templates    studio
/app/jobs         history
/app/keys         API keys
/app/billing      Stripe
```

The post-login page for week 1 is allowed to be ugly: remaining pages, Add to Cursor, one sample invoice. That *is* the dashboard v0.

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

Do not start Stripe before `workspace_id` exists.

### P1 — Thinnest chargeable cloud

- [ ] Deploy current API + worker with **our** Gemini key, Postgres, Redis, S3
- [ ] `struxio-web`: GitHub OAuth → workspace → hashed API key → Stripe ($29 or page pack)
- [ ] Meter remaining pages; 402 when empty
- [ ] Hosted MCP with **two tools first**: `extract`, `list_templates`. Bearer key is enough for week 1; OAuth is week 2–4
- [ ] Post-login: Add to Cursor + remaining pages

**Skip in P1:** Clerk orgs, team invites, Kafka, custom models, hand-written SDKs, SSO, HITL, a new queue.

### P2 — Agent path is obvious

- [ ] Cursor deeplink + marketplace / directory listing
- [ ] `SKILL.md` + public demo repo: folder of messy invoices, one-sentence README
- [ ] `extract_batch` + `get_job` + `suggest_schema`
- [ ] Flash as default hosted model (Pro blows COGS)
- [ ] Dockerfile + Compose that run api + worker
- [ ] Tests for extract + MCP transcripts

### P3 — Cloud that does not embarrass us

- [ ] OAuth for MCP (Connect on first tool call)
- [ ] stdio lite + cloud desktop bridge (`npx @struxio/mcp`)
- [ ] Template studio + job list
- [ ] Provider trait; OpenAPI generated from Axum (SDKs generated later)
- [ ] Rate limits in cloud middleware
- [ ] Worker reliability (retries, concurrency, honest batch status)

### Explicitly later (when someone asks and will pay)

SSO, audit export, VPC, DPA, SLA, commercial AGPL exception, human review, parse-then-extract (wrap Docling/Kreuzberg, do not build OCR), bounding boxes, evals UI, Kafka, fake confidence scores.

**One-sentence definition of done for “rebuild v1”:** GitHub login → Add to Cursor → agent extracts a PDF against a schema → Stripe when free pages run out.

---

## 12. What we will not do

- Rewrite the extractor in Python/TS to “move faster.” The Rust core is the advantage (cost, deploy size). Add TypeScript only at the web and the MIT MCP proxy.
- Feature-gate extraction, batch, or templates in OSS.
- Make Studio / HITL / workflow builder the homepage.
- Expose S3 internals to agents.
- Optimize for enterprise procurement before a builder has paid $29.
- Promise parse quality we do not have.

---

## 13. Open decisions (do not block P0)

1. **CLA vendor** (GitHub CLA bot vs EasyCLA) — needed before outside PRs if we want relicensing rights.
2. **Auth provider for cloud** — GitHub OAuth in Next.js is enough for P1; Clerk is optional later (the comment in `static_auth.rs` is not a requirement).
3. **Hosted MCP OAuth in week 1 vs bearer-only** — bearer ships faster; OAuth is the businessman UX. Prefer OAuth as soon as Cursor Connect works; do not delay Stripe for it.
4. **Where templates live in OSS lite** — sqlite vs JSON file in `~/.struxio/`.
5. **Commercial license text** — can wait until the first embedder asks.
6. **EU MCP region** — wait for a customer.

---

## 14. Why this is a rebuild, not a polish

The current app is “almost good” as a **self-host API skeleton**. It is not good as a **product**.

A polish would add docs and a Dockerfile and still lose to Reducto’s MCP and Kreuzberg’s library UX.

A rebuild keeps the Gemini extract core and changes the center of gravity:

- **Interface:** MCP first, REST second
- **Buyer:** zero-ops builder, not the VM nerd
- **Packaging:** AGPL engine + hosted credits + `struxio-web` as cash register
- **Schema:** workspace isolation before the second customer
- **Promise:** the homepage’s one-shot extract, actually implemented

That is the structure. Next implementation PR should start at P0 (`workspace_id` + `/v1/extract` + license honesty), not at a greenfield monorepo.
