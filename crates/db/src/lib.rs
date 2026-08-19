#![allow(unused_variables, unused_imports)]

mod codec;
pub mod error;
pub mod records;
pub mod repositories;

use sqlx::postgres::{PgPool, PgPoolOptions};

pub use error::ContractStoreError;
pub use records::{
    EvalRunMetrics, NewEvalRunResult, NewValidationReport, StoredContractVersion, StoredEvalRun,
    StoredEvalRunResult, StoredEvidenceAttachment, StoredEvidenceSidecar, StoredFixture,
    StoredValidationReport,
};
pub use repositories::evaluations::compute_eval_run_metrics;

pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
}
