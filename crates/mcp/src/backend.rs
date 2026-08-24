// SPDX-License-Identifier: AGPL-3.0-only

use std::future::Future;

use struxio_common::models::{
    BatchJob, CreateBatchRequest, CreateExtractionRequest, Extraction, ExtractionTemplate,
    InlineExtractionRequest,
};
use struxio_common::PrincipalContext;
use struxio_core::queue::QueueProducer;
use struxio_core::services::{
    batch_service::BatchService, extraction_service::ExtractionService,
    model_service::ModelService, template_service::TemplateService,
};
use uuid::Uuid;

use crate::error::{McpError, McpResult};
use crate::tools::{ExtractArgs, ExtractSource};

/// Application-side capability surface used by the MCP dispatcher.
///
/// Implementations must honor [`PrincipalContext`] for workspace isolation and
/// must not perform unscoped repository access. This crate never calls `struxio-db`.
pub trait McpBackend: Send + Sync {
    fn list_templates(
        &self,
        ctx: &PrincipalContext,
    ) -> impl Future<Output = McpResult<Vec<ExtractionTemplate>>> + Send;

    fn get_template(
        &self,
        ctx: &PrincipalContext,
        template_id: Uuid,
    ) -> impl Future<Output = McpResult<ExtractionTemplate>> + Send;

    fn create_inline(
        &self,
        ctx: &PrincipalContext,
        request: InlineExtractionRequest,
    ) -> impl Future<Output = McpResult<Extraction>> + Send;

    fn create_extraction(
        &self,
        ctx: &PrincipalContext,
        request: CreateExtractionRequest,
    ) -> impl Future<Output = McpResult<Extraction>> + Send;

    fn get_extraction(
        &self,
        ctx: &PrincipalContext,
        extraction_id: Uuid,
    ) -> impl Future<Output = McpResult<Extraction>> + Send;

    fn create_batch(
        &self,
        ctx: &PrincipalContext,
        request: CreateBatchRequest,
    ) -> impl Future<Output = McpResult<BatchJob>> + Send;

    fn get_batch(
        &self,
        ctx: &PrincipalContext,
        batch_id: Uuid,
    ) -> impl Future<Output = McpResult<BatchJob>> + Send;
}

/// Thin adapter over current `struxio-core` services. No repository, provider,
/// evidence, or queue-policy logic lives here.
#[derive(Clone)]
pub struct ServiceAdapter<Q: QueueProducer> {
    template_service: TemplateService,
    extraction_service: ExtractionService<Q>,
    batch_service: BatchService<Q>,
    model_service: ModelService,
}

impl<Q: QueueProducer> ServiceAdapter<Q> {
    pub fn new(
        template_service: TemplateService,
        extraction_service: ExtractionService<Q>,
        batch_service: BatchService<Q>,
        model_service: ModelService,
    ) -> Self {
        Self {
            template_service,
            extraction_service,
            batch_service,
            model_service,
        }
    }

    async fn default_model_id(&self) -> McpResult<String> {
        self.model_service
            .get_default_model()
            .await
            .map(|model| model.id)
            .map_err(McpError::from_app)
    }
}

impl<Q: QueueProducer> McpBackend for ServiceAdapter<Q> {
    async fn list_templates(&self, ctx: &PrincipalContext) -> McpResult<Vec<ExtractionTemplate>> {
        self.template_service
            .list(ctx)
            .await
            .map_err(McpError::from_app)
    }

    async fn get_template(
        &self,
        ctx: &PrincipalContext,
        template_id: Uuid,
    ) -> McpResult<ExtractionTemplate> {
        self.template_service
            .get(ctx, template_id)
            .await
            .map_err(McpError::from_app)
    }

    async fn create_inline(
        &self,
        ctx: &PrincipalContext,
        request: InlineExtractionRequest,
    ) -> McpResult<Extraction> {
        let model_id = self.default_model_id().await?;
        self.extraction_service
            .create_inline(ctx, &request, &model_id)
            .await
            .map_err(McpError::from_app)
    }

    async fn create_extraction(
        &self,
        ctx: &PrincipalContext,
        request: CreateExtractionRequest,
    ) -> McpResult<Extraction> {
        let model_id = self.default_model_id().await?;
        self.extraction_service
            .create_sync(ctx, &request, &model_id)
            .await
            .map_err(McpError::from_app)
    }

    async fn get_extraction(
        &self,
        ctx: &PrincipalContext,
        extraction_id: Uuid,
    ) -> McpResult<Extraction> {
        self.extraction_service
            .get(ctx, extraction_id)
            .await
            .map_err(McpError::from_app)
    }

    async fn create_batch(
        &self,
        ctx: &PrincipalContext,
        request: CreateBatchRequest,
    ) -> McpResult<BatchJob> {
        let model_id = self.default_model_id().await?;
        self.batch_service
            .create(ctx, &request, &model_id)
            .await
            .map_err(McpError::from_app)
    }

    async fn get_batch(&self, ctx: &PrincipalContext, batch_id: Uuid) -> McpResult<BatchJob> {
        self.batch_service
            .get(ctx, batch_id)
            .await
            .map_err(McpError::from_app)
    }
}

pub async fn extract_with<B: McpBackend>(
    backend: &B,
    ctx: &PrincipalContext,
    args: ExtractArgs,
) -> McpResult<Extraction> {
    match args.source {
        ExtractSource::Inline {
            file_name,
            file_type,
            file_base64,
        } => {
            backend
                .create_inline(
                    ctx,
                    InlineExtractionRequest {
                        file_name,
                        file_type,
                        file_base64,
                        template_id: args.template_id,
                    },
                )
                .await
        }
        ExtractSource::Document { document_id } => {
            backend
                .create_extraction(
                    ctx,
                    CreateExtractionRequest {
                        document_id,
                        template_id: args.template_id,
                    },
                )
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use struxio_common::{AppError, PrincipalId, Scope, ScopeSet, WorkspaceId};

    #[test]
    fn adapter_signature_requires_principal_context() {
        fn assert_ctx(_: &PrincipalContext) {}
        let ctx = PrincipalContext::new(
            WorkspaceId::local(),
            PrincipalId::local(),
            ScopeSet::from_scopes([Scope::TemplatesRead]),
        );
        assert_ctx(&ctx);
        assert!(!ctx.workspace_id().as_uuid().is_nil());
    }

    #[test]
    fn service_errors_are_stable() {
        let err = McpError::from_app(AppError::NotFound("hidden-row".into()));
        assert_eq!(err.code(), "not_found");
        assert!(!err.message().contains("hidden-row"));
    }
}
