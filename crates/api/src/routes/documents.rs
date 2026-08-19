use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use struxio_common::models::{
    CheckDocumentRequest, CheckDocumentResponse, ConfirmUploadRequest, Document,
};
use struxio_common::{PrincipalContext, Scope};

use crate::errors::ApiError;
use crate::middleware::auth_provider::require_scope;
use crate::state::AppState;

pub async fn check_document(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Json(body): Json<CheckDocumentRequest>,
) -> Result<Json<CheckDocumentResponse>, ApiError> {
    require_scope(&ctx, Scope::DocumentsWrite).map_err(ApiError)?;
    let response = state
        .document_service
        .check_document(&ctx, &body)
        .await
        .map_err(ApiError)?;
    Ok(Json(response))
}

pub async fn confirm_upload(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Json(body): Json<ConfirmUploadRequest>,
) -> Result<Json<Document>, ApiError> {
    require_scope(&ctx, Scope::DocumentsWrite).map_err(ApiError)?;
    let doc = state
        .document_service
        .confirm_upload(&ctx, &body)
        .await
        .map_err(ApiError)?;
    Ok(Json(doc))
}

pub async fn list_documents(
    ctx: PrincipalContext,
    State(state): State<AppState>,
) -> Result<Json<Vec<Document>>, ApiError> {
    require_scope(&ctx, Scope::DocumentsRead).map_err(ApiError)?;
    let docs = state.document_service.list(&ctx).await.map_err(ApiError)?;
    Ok(Json(docs))
}

pub async fn get_document(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<Document>, ApiError> {
    require_scope(&ctx, Scope::DocumentsRead).map_err(ApiError)?;
    let doc = state
        .document_service
        .get_by_id(&ctx, path.id)
        .await
        .map_err(ApiError)?;
    Ok(Json(doc))
}

pub async fn delete_document(
    ctx: PrincipalContext,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<StatusCode, ApiError> {
    require_scope(&ctx, Scope::DocumentsWrite).map_err(ApiError)?;
    state
        .document_service
        .delete(&ctx, path.id)
        .await
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
