use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use struxio_common::models::{
    CheckDocumentRequest, CheckDocumentResponse, ConfirmUploadRequest, Document,
};

use crate::errors::ApiError;
use crate::middleware::auth_provider::AuthUser;
use crate::state::AppState;

pub async fn check_document(
    _auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CheckDocumentRequest>,
) -> Result<Json<CheckDocumentResponse>, ApiError> {
    let response = state
        .document_service
        .check_document(&body)
        .await
        .map_err(ApiError)?;
    Ok(Json(response))
}

pub async fn confirm_upload(
    _auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<ConfirmUploadRequest>,
) -> Result<Json<Document>, ApiError> {
    let doc = state
        .document_service
        .confirm_upload(&body)
        .await
        .map_err(ApiError)?;
    Ok(Json(doc))
}

pub async fn list_documents(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Document>>, ApiError> {
    let docs = state
        .document_service
        .list()
        .await
        .map_err(ApiError)?;
    Ok(Json(docs))
}

pub async fn get_document(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<Json<Document>, ApiError> {
    let doc = state
        .document_service
        .get_by_id(path.id)
        .await
        .map_err(ApiError)?;
    Ok(Json(doc))
}

pub async fn delete_document(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(path): Path<super::IdPath>,
) -> Result<StatusCode, ApiError> {
    state
        .document_service
        .delete(path.id)
        .await
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
