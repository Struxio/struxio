// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::{json, Value};
use struxio_common::models::ExtractionTemplate;
use struxio_common::{PrincipalContext, Scope};

use crate::backend::{extract_with, McpBackend};
use crate::error::{McpError, McpResult};
use crate::tools::{
    parse_batch, parse_batch_id, parse_empty, parse_extract, parse_extraction_id,
    parse_template_id, ToolName,
};

/// Dispatch one MCP tool against application services.
///
/// Scope checks happen here, matching the HTTP adapter, before any service
/// call. Tool arguments never include a workspace id; the supplied principal
/// is the only tenancy boundary.
pub async fn dispatch_tool<B: McpBackend>(
    backend: &B,
    ctx: &PrincipalContext,
    name: &str,
    arguments: Value,
) -> McpResult<Value> {
    let tool = ToolName::parse(name).ok_or_else(McpError::tool_not_found)?;
    match tool {
        ToolName::ListTemplates => {
            require_scope(ctx, Scope::TemplatesRead)?;
            parse_empty(arguments)?;
            let templates = backend.list_templates(ctx).await?;
            let summaries: Vec<Value> = templates.iter().map(template_summary).collect();
            Ok(json!({
                "templates": summaries,
                "next_steps": [
                    "Call get_template with a template id, then extract with that template_id."
                ]
            }))
        }
        ToolName::GetTemplate => {
            require_scope(ctx, Scope::TemplatesRead)?;
            let template_id = parse_template_id(arguments)?;
            let template = backend.get_template(ctx, template_id).await?;
            Ok(json!({
                "template": template,
                "next_steps": [
                    "Call extract with this template_id and either file_base64 or document_id."
                ]
            }))
        }
        ToolName::Extract => {
            require_scope(ctx, Scope::ExtractionsCreate)?;
            let args = parse_extract(arguments)?;
            let extraction = extract_with(backend, ctx, args).await?;
            Ok(json!({
                "extraction": extraction,
                "next_steps": extraction_next_steps(&extraction.status)
            }))
        }
        ToolName::GetExtraction => {
            require_scope(ctx, Scope::ExtractionsRead)?;
            let extraction_id = parse_extraction_id(arguments)?;
            let extraction = backend.get_extraction(ctx, extraction_id).await?;
            Ok(json!({
                "extraction": extraction,
                "next_steps": extraction_next_steps(&extraction.status)
            }))
        }
        ToolName::ExtractBatch => {
            require_scope(ctx, Scope::BatchesCreate)?;
            let (template_id, document_ids) = parse_batch(arguments)?;
            let batch = backend
                .create_batch(
                    ctx,
                    struxio_common::models::CreateBatchRequest {
                        document_ids,
                        template_id,
                    },
                )
                .await?;
            Ok(json!({
                "batch": batch,
                "next_steps": [
                    "Poll get_batch until status is completed, failed, or partially_completed."
                ]
            }))
        }
        ToolName::GetBatch => {
            require_scope(ctx, Scope::BatchesRead)?;
            let batch_id = parse_batch_id(arguments)?;
            let batch = backend.get_batch(ctx, batch_id).await?;
            Ok(json!({
                "batch": batch,
                "next_steps": [
                    "If the batch is still pending or processing, poll get_batch again."
                ]
            }))
        }
    }
}

fn require_scope(ctx: &PrincipalContext, scope: Scope) -> McpResult<()> {
    ctx.require_scope(scope).map_err(McpError::from_app)
}

fn template_summary(template: &ExtractionTemplate) -> Value {
    json!({
        "id": template.id,
        "name": template.name,
        "description": template.description,
        "is_system": template.is_system,
    })
}

fn extraction_next_steps(status: &str) -> Vec<&'static str> {
    match status {
        "pending" | "processing" => {
            vec!["Poll get_extraction until status is completed or failed."]
        }
        "failed" => vec!["Inspect extraction.error_message, then retry extract if appropriate."],
        _ => vec!["Read extraction.result. Use extract_batch for many documents."],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use struxio_common::models::{
        BatchJob, CreateBatchRequest, CreateExtractionRequest, Extraction,
    };
    use struxio_common::{PrincipalId, ScopeSet, WorkspaceId};
    use uuid::Uuid;

    struct CountingBackend {
        calls: AtomicUsize,
    }

    impl CountingBackend {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl McpBackend for CountingBackend {
        async fn list_templates(
            &self,
            ctx: &PrincipalContext,
        ) -> McpResult<Vec<ExtractionTemplate>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(!ctx.workspace_id().as_uuid().is_nil());
            Ok(vec![ExtractionTemplate {
                id: Uuid::from_u128(7),
                workspace_id: ctx.workspace_id(),
                name: "Invoice".into(),
                description: Some("totals".into()),
                json_schema: json!({"type":"object","properties":{"total":{"type":"number"}}}),
                prompt_template: "extract".into(),
                is_system: false,
                created_at: Utc::now(),
            }])
        }

        async fn get_template(
            &self,
            ctx: &PrincipalContext,
            template_id: Uuid,
        ) -> McpResult<ExtractionTemplate> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ExtractionTemplate {
                id: template_id,
                workspace_id: ctx.workspace_id(),
                name: "Invoice".into(),
                description: None,
                json_schema: json!({"type":"object","properties":{}}),
                prompt_template: "extract".into(),
                is_system: true,
                created_at: Utc::now(),
            })
        }

        async fn create_inline(
            &self,
            ctx: &PrincipalContext,
            request: struxio_common::models::InlineExtractionRequest,
        ) -> McpResult<Extraction> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_extraction(ctx, request.template_id, "completed"))
        }

        async fn create_extraction(
            &self,
            ctx: &PrincipalContext,
            request: CreateExtractionRequest,
        ) -> McpResult<Extraction> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample_extraction(ctx, request.template_id, "completed"))
        }

        async fn get_extraction(
            &self,
            ctx: &PrincipalContext,
            extraction_id: Uuid,
        ) -> McpResult<Extraction> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut extraction = sample_extraction(ctx, Uuid::from_u128(1), "pending");
            extraction.id = extraction_id;
            Ok(extraction)
        }

        async fn create_batch(
            &self,
            ctx: &PrincipalContext,
            request: CreateBatchRequest,
        ) -> McpResult<BatchJob> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(BatchJob {
                id: Uuid::from_u128(3),
                workspace_id: ctx.workspace_id(),
                template_id: request.template_id,
                status: "pending".into(),
                total_documents: request.document_ids.len() as i32,
                completed_documents: 0,
                failed_documents: 0,
                created_at: Utc::now(),
                completed_at: None,
            })
        }

        async fn get_batch(&self, ctx: &PrincipalContext, batch_id: Uuid) -> McpResult<BatchJob> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(BatchJob {
                id: batch_id,
                workspace_id: ctx.workspace_id(),
                template_id: Uuid::from_u128(1),
                status: "processing".into(),
                total_documents: 2,
                completed_documents: 1,
                failed_documents: 0,
                created_at: Utc::now(),
                completed_at: None,
            })
        }
    }

    fn sample_extraction(ctx: &PrincipalContext, template_id: Uuid, status: &str) -> Extraction {
        Extraction {
            id: Uuid::from_u128(11),
            workspace_id: ctx.workspace_id(),
            document_id: Uuid::from_u128(12),
            template_id,
            batch_job_id: None,
            status: status.into(),
            result: Some(json!({"total": 10})),
            error_message: None,
            model_id: Some("gemini-2.5-flash".into()),
            credits_charged: 0,
            input_tokens: 1,
            output_tokens: 1,
            processing_time_ms: Some(5),
            attempt: 0,
            created_at: Utc::now(),
            completed_at: Some(Utc::now()),
        }
    }

    fn principal(scopes: impl IntoIterator<Item = Scope>) -> PrincipalContext {
        PrincipalContext::new(
            WorkspaceId::local(),
            PrincipalId::local(),
            ScopeSet::from_scopes(scopes),
        )
    }

    #[tokio::test]
    async fn missing_scope_does_not_call_the_backend() {
        let backend = CountingBackend::new();
        let ctx = principal([Scope::TemplatesRead]);
        let err = dispatch_tool(
            &backend,
            &ctx,
            "extract",
            json!({
                "template_id": Uuid::from_u128(1),
                "document_id": Uuid::from_u128(2)
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "forbidden");
        assert_eq!(err.message(), "insufficient scope");
        assert!(!err.message().contains("extract"));
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn list_templates_omits_schema_and_uses_caller_workspace() {
        let backend = CountingBackend::new();
        let ctx = principal([Scope::TemplatesRead]);
        let value = dispatch_tool(&backend, &ctx, "list_templates", json!({}))
            .await
            .unwrap();
        assert_eq!(value["templates"][0]["id"], json!(Uuid::from_u128(7)));
        assert!(value["templates"][0].get("json_schema").is_none());
        assert!(value["next_steps"][0]
            .as_str()
            .unwrap()
            .contains("get_template"));
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unknown_tool_is_stable_and_does_not_touch_backend() {
        let backend = CountingBackend::new();
        let ctx = principal(Scope::ALL);
        let err = dispatch_tool(&backend, &ctx, "parse", json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code(), "tool_not_found");
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn extract_and_batch_happy_paths_require_matching_scopes() {
        let backend = CountingBackend::new();
        let ctx = principal([
            Scope::ExtractionsCreate,
            Scope::ExtractionsRead,
            Scope::BatchesCreate,
            Scope::BatchesRead,
        ]);
        let extracted = dispatch_tool(
            &backend,
            &ctx,
            "extract",
            json!({
                "template_id": Uuid::from_u128(1),
                "document_id": Uuid::from_u128(2)
            }),
        )
        .await
        .unwrap();
        assert_eq!(extracted["extraction"]["status"], "completed");

        let status = dispatch_tool(
            &backend,
            &ctx,
            "get_extraction",
            json!({ "extraction_id": Uuid::from_u128(11) }),
        )
        .await
        .unwrap();
        assert!(status["next_steps"][0]
            .as_str()
            .unwrap()
            .contains("get_extraction"));

        let batch = dispatch_tool(
            &backend,
            &ctx,
            "extract_batch",
            json!({
                "template_id": Uuid::from_u128(1),
                "document_ids": [Uuid::from_u128(2)]
            }),
        )
        .await
        .unwrap();
        assert_eq!(batch["batch"]["total_documents"], 1);

        let _ = dispatch_tool(
            &backend,
            &ctx,
            "get_batch",
            json!({ "batch_id": Uuid::from_u128(3) }),
        )
        .await
        .unwrap();
        assert_eq!(backend.calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn get_template_rejects_foreign_argument_fields() {
        let backend = CountingBackend::new();
        let ctx = principal([Scope::TemplatesRead]);
        let err = dispatch_tool(
            &backend,
            &ctx,
            "get_template",
            json!({
                "template_id": Uuid::from_u128(1),
                "extraction_id": Uuid::from_u128(2)
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), "invalid_arguments");
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }
}
