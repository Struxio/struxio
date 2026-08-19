# Prompt: adversarial swarm review of the Struxio extract-harness rebuild

Copy everything below the line into a new agent that has access to this repository. Do not summarize before pasting — the swarm needs the constraints intact.

---

## Your job

You are reviewing a **product + architecture proposal** for Struxio (github.com/Struxio/struxio), an AGPL-leaning Rust document-extraction engine that is “almost good” but not a sellable product yet.

**Do not implement the rebuild.** Do not rewrite the RFC unless you find a factual error in the code vs the docs. Your output is a critical review: value, problems, new possibilities.

**You must spawn a swarm of parallel sub-agents** (at least 5, ideally 6–8). Do not review this from a single pass of your own context. Split the work, then synthesize. If a sub-agent disagrees with the RFC, keep the disagreement — do not average it away.

## What the idea is (read the docs first)

The proposed rebuild, in one sentence:

> Keep `extract(document, schema) → JSON` as a tiny **kernel**, and build a production **harness** around it (ingest, parse-once IR, named templates, citations, validations, fast/accurate/agentic modes, batch/jobs, MCP, Studio). Open-source the full harness (Langfuse vs LangSmith). Sell hosted cloud to builders who will not run Postgres+Docling. Lead with **your JSON** (named templates), not parse-first like Reducto. Default must be **fast** (target p50 &lt; 3s on a digital invoice), not 13–30s agentic.

Read, in this order:

1. `docs/rebuild-harness.md` — kernel vs harness (the idea)
2. `docs/rebuild-rfc.md` — full plan, sequencing, license/tenancy
3. `docs/rebuild-landscape.md` — other vendors, template wedge, performance doctrine
4. `docs/rebuild-reducto-map.md` — Reducto verbs/IR and OSS libraries
5. Actual code: `crates/`, `migrations/`, `README.md`, `LICENSE` vs README license claim, `CONTRIBUTING.md`

Also skim `https://struxio.dev/` if reachable. Note docs/code/marketing contradictions.

## Swarm roster (spawn all of these in parallel)

Give each agent the idea summary above plus the file list. Tell each one: be specific; cite files, APIs, or vendors; no generic “consider scalability.”

### 1. Kernel/harness architect
Is “extract kernel + harness” a coherent architecture or a slogan? Where will the split rot (credits in GeminiClient, parse leaking into MCP, templates that are just UUIDs)? What belongs in kernel vs harness vs adapters that the RFC got wrong? Propose a cleaner module map if needed.

### 2. Adversarial product / strategy
Play VC who has seen Langfuse, Reducto, Extend, LlamaExtract, Landing ADE, Sensible, Kreuzberg, Docling. Is “open-source Reducto but template-first and faster” a real wedge or a mashup that loses to all of them? Who actually pays $29? Does the nerd/builder/enterprise split hold? What is the most likely way this dies?

### 3. Competitive holes and stolen features
Go beyond the landscape doc. What did the RFC miss (Chunkr, Mistral OCR, Datalab, Instabase, Affinda, Adobe, ABBYY, Google/Azure/AWS, Claude/Gemini native PDF, local VLMs)? Which “steal” items are actually traps? What should we **not** copy from Reducto? New possibilities the RFC never named.

### 4. Performance skeptic
The doctrine says p50 &lt; 3s on a 1–2 page digital invoice, Docling sidecar, Gemini Flash, parse-once. Tear this apart. What is the real p50 once you add network, cold start, Flash latency, pdfium, queue? Where does “fast default” collapse (scans, 40-page contracts, handwriting)? What instrumentation and SLOs are missing? How should the serial Redis worker / whole-file base64 Gemini path change, with numbers?

### 5. Codebase truth-teller
Read the Rust. List every way the current engine **cannot** support this harness without a painful rewrite (global unique md5, `AuthUser` unit struct, `credits_charged = 0`, serial worker, LICENSE Apache vs AGPL README, no tests, mime bugs, sync vs async docs). Rank: trivial / medium / “this is the 6-month rewrite they think they’re avoiding.”

### 6. MCP / agent-native UX
For a builder in Cursor, Claude Code, Codex: walk the proposed `extract` / `extract_batch` / templates flow. Where do agents fail (local files vs hosted, schema-in-tool-args, 8 MiB base64, no `upload_file` vs Reducto’s hop)? New MCP tools, resources, prompts, or SKILL.md ideas. OAuth vs bearer. What would make Struxio the default “drop a folder of invoices” MCP?

### 7. Open-source + cloud economics (optional 8th: evals/quality)
AGPL file vs Apache LICENSE. CLA. What must stay OSS vs cloud without killing the Langfuse sentence. Unit economics: can Flash + Docling CPU beat ~$0.03/page and still hit &lt;3s? Where hosted GPU parse destroys margin. New possibility: BYOK vs hosted-only SKUs.

### 8. Quality / evals / trust (optional if you only run 6)
Citations, confidence, validations, golden sets, RD-TableBench. How does the harness get *better* without a model lab? Human review: when is it necessary? Fake confidence on the homepage today — what is the honest quality story?

## What to return (single synthesized report)

After the swarm, **you** write one report the founder can read. Structure:

### A. The idea in one paragraph
Your words, not the RFC’s. Is it good?

### B. Value (keep / double down)
5–10 bullets. What is actually valuable vs what is fashion (MCP, “agentic,” Studio)?

### C. Problems (ranked)
P0 kill-the-company, P1 kill-the-wedge, P2 annoying. For each: evidence (code path, vendor, latency), and a concrete mitigation or a recommendation to **drop** that part of the plan.

### D. Internal contradictions
Places the four docs disagree with each other or with the code (license, sync extract, pagination, “open-source Reducto” vs “don’t clone Reducto,” Studio OSS vs cash-register week 1, etc.).

### E. New possibilities
Things the RFC did not propose that are high leverage. At least 8. Include 1–2 that are *weird* (distribution, evals, template marketplace, local-only kernel crate, embedding Struxio inside other MCP gateways, etc.). Mark each: do now / later / never.

### F. What to cut
A shorter v1 harness than the RFC’s P0–P3. What is the smallest thing that still *is* this idea?

### G. Swarm dissent
Where sub-agents disagreed. Do not hide it.

### H. Verdict
One of: **build this**, **build a thinner version**, **pivot the wedge**, **do not rebuild yet**. One paragraph why.

## Rules

- Prefer citing `path/file.rs` or a vendor URL over vibes.
- Do not be polite if the plan is bloated. “Open-source Reducto + Langfuse + MCP + Studio + Docling + Stripe” can be four products.
- Do not implement. If you must sketch an API, keep it &lt; 30 lines.
- Work in English. The founder is technical.

Begin by spawning the swarm. Then synthesize.
