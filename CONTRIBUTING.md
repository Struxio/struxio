# Contributing to Struxio

Thank you for your interest in contributing! Struxio is an open-core project — the core extraction engine and API are fully open source under AGPL-3.0.

## What Belongs Here

This repo (`struxio`) is the **OSS engine**: REST API, worker, and MCP server. Contributions here should focus on:

- Document extraction logic and AI prompt improvements
- New API endpoints or expressive query parameters
- MCP tools that call `core` (extract, extract_batch, templates) — not S3 check/confirm
- Database schema improvements and migrations
- Performance improvements (batching, async processing)
- Developer experience (better error messages, docs, tests)
- Bug fixes

**The following are out of scope for this repo:**
- Authentication providers (Clerk, Auth0, etc.) → `struxio-cloud`
- Payment/billing flows (Stripe, etc.) → `struxio-cloud`
- Hosted MCP OAuth, multi-tenant metering, and the public MCP URL → `struxio-cloud`
- Frontend/dashboard UI → `struxio-web`

Product direction (MCP-first, hosted cloud for zero-ops buyers, OSS for self-hosters) is documented in [docs/rebuild-rfc.md](./docs/rebuild-rfc.md).

## Getting Started

### 1. Fork and clone

```bash
git clone https://github.com/your-org/struxio
cd struxio
```

### 2. Set up the dev environment

```bash
cp .env.example .env
# Fill in the required variables
docker compose up -d
cargo run -p struxio-api
```

### 3. Verify everything works

```bash
cargo check      # should produce no errors
cargo clippy     # check for lint issues
cargo test       # run the test suite
```

## Project Structure

```
crates/
├── api/       — Axum HTTP server, routes, middleware (power-user REST)
├── core/      — Business logic and service layer
├── db/        — SQLx repositories (no business logic)
├── common/    — Shared types (models, config, errors)
├── mcp/       — planned: MCP server (primary agent interface)
└── worker/    — Background job processor
```

**Key principle:** Business logic lives in `core/`, never in `api/` or `db/`. Routes call services. Services call repositories.

## Coding Standards

- **Rust edition 2021** throughout
- Use `anyhow` for application errors, `thiserror` for library errors
- All public functions must have doc comments
- Prefer `tracing::info!` / `warn!` / `error!` over `println!`
- No `unwrap()` in production code paths — use `?` or explicit error handling
- Follow [Conventional Commits](https://www.conventionalcommits.org/) for commit messages

## Making Changes

### Adding a new route

1. Add the handler in `crates/api/src/routes/<resource>.rs`
2. Register it in `crates/api/src/routes/mod.rs` under `oss_router()`
3. Add the corresponding service method in `crates/core/src/services/<resource>_service.rs`
4. Add the DB repository method in `crates/db/src/repositories/<resource>.rs`

### Adding a new migration

```bash
# Migrations are plain SQL files, numbered sequentially
touch migrations/000N_your_description.sql
```

Write forward-only migrations. We do not use down migrations.

### Adding a new model

All shared models live in `crates/common/src/models.rs`. Keep them lean — no business logic, just data.

## Pull Request Process

1. Open an issue first to discuss significant changes
2. Branch from `main` with a descriptive name: `feat/batch-webhook`, `fix/rate-limit-edge-case`
3. Keep PRs focused — one concern per PR
4. Ensure `cargo check`, `cargo clippy`, and `cargo test` all pass
5. Update relevant docs/`README.md` if the change affects the public API or setup steps
6. Request review from a maintainer

## Commit Message Format

```
type(scope): short description

feat(api): add pagination to /v1/extractions
fix(core): handle empty template gracefully
docs(readme): clarify S3 configuration
chore(deps): update sqlx to 0.8.3
```

Types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `perf`

## Security

Found a vulnerability? **Do not open a public issue.** Email security@struxio.com or see [SECURITY.md](./SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under [AGPL-3.0](./LICENSE).
