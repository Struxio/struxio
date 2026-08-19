// SPDX-License-Identifier: AGPL-3.0-only

//! Service-level PostgreSQL coverage for contract catalog and evaluation recording.
//! Requires `DATABASE_URL`.

use std::collections::BTreeMap;

use sqlx::PgPool;
use uuid::Uuid;

use struxio_common::PrincipalContext;
use struxio_contracts::{
    ContractSlug, ContractSpec, EvalDecision, EvalThresholds, EvidenceEntry, EvidenceKind,
    EvidencePolicy, EvidenceSidecar, ExtractionContract, FixtureDescriptor, FixtureInput,
    JsonPointer, JsonSchema, JsonValueType, PositiveVersion, ProviderNeutralInstructions,
    Sha256ContentHash, Validator, ValidatorRule,
};
use struxio_core::services::{ContractCatalogService, EvaluationService, FixtureOutcome};
use struxio_db::repositories::extractions::ExtractionRepo;

async fn connect() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = struxio_db::create_pool(&url)
        .await
        .expect("connect to postgres");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

fn sample_contract(slug: &str) -> ExtractionContract {
    let pointer = JsonPointer::parse("/name").unwrap();
    let spec = ContractSpec::new(
        JsonSchema::new(serde_json::json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        }))
        .unwrap(),
        ProviderNeutralInstructions::new("Extract the name").unwrap(),
    )
    .with_validator(
        ValidatorRule::new(pointer.clone(), Validator::Type(JsonValueType::String)).unwrap(),
    )
    .with_evidence_policy(EvidencePolicy::required(vec![pointer.clone()]))
    .with_eval(EvalThresholds::strict())
    .with_fixture(
        FixtureDescriptor::new(
            "basic",
            FixtureInput::new(
                "doc.pdf",
                "application/pdf",
                Sha256ContentHash::from_content(b"fixture-bytes"),
            )
            .unwrap(),
            serde_json::json!({"name": "Ada"}),
            EvidenceSidecar::from_entries(BTreeMap::new()).with(
                pointer,
                EvidenceEntry::new(EvidenceKind::Exact, Some("Ada".into()), Some("p1".into()))
                    .unwrap(),
            ),
            Some(EvalDecision::Accept),
        )
        .unwrap(),
    );
    ExtractionContract::new(
        ContractSlug::new(slug).unwrap(),
        PositiveVersion::new(1).unwrap(),
        spec,
    )
}

fn exact_name_sidecar() -> EvidenceSidecar {
    EvidenceSidecar::from_entries(BTreeMap::new()).with(
        JsonPointer::parse("/name").unwrap(),
        EvidenceEntry::new(EvidenceKind::Exact, Some("Ada".into()), Some("p1".into())).unwrap(),
    )
}

async fn seed_extraction(pool: &PgPool) -> uuid::Uuid {
    let workspace = struxio_common::WorkspaceId::local();
    let template_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM extraction_templates WHERE workspace_id = $1 LIMIT 1")
            .bind(workspace.as_uuid())
            .fetch_one(pool)
            .await
            .expect("template");
    let doc_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'a.pdf', 'pdf', $3, 1) RETURNING id",
    )
    .bind(workspace.as_uuid())
    .bind(format!("md5-{}", Uuid::new_v4().simple()))
    .bind(format!("s3-{}", Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .expect("document");
    ExtractionRepo::create(pool, workspace, doc_id, template_id, None)
        .await
        .expect("extraction")
        .id
}

#[tokio::test]
async fn evaluation_service_keeps_extraction_json_schema_pure() {
    let pool = connect().await;
    let ctx = PrincipalContext::local_operator();
    let catalog = ContractCatalogService::new(pool.clone());
    let evals = EvaluationService::new(pool.clone());
    let contract = sample_contract(&format!("svc-{}", Uuid::new_v4().simple()));
    let stored = catalog.publish(&ctx, &contract).await.unwrap();
    let extraction_id = seed_extraction(&pool).await;
    ExtractionRepo::update_completed(
        &pool,
        ctx.workspace_id(),
        extraction_id,
        &serde_json::json!({"name": "Ada"}),
        1,
        1,
        5,
        "gemini-2.5-flash",
        0,
    )
    .await
    .unwrap();

    let err = evals
        .record_extraction_evaluation(
            &ctx,
            extraction_id,
            stored.id,
            &serde_json::json!({"name": "Ada", "evidence": {"name": "Ada"}}),
            &exact_name_sidecar(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("schema-pure"));

    let (sidecar, report) = evals
        .record_extraction_evaluation(
            &ctx,
            extraction_id,
            stored.id,
            &serde_json::json!({"name": "Ada"}),
            &exact_name_sidecar(),
        )
        .await
        .unwrap();
    assert!(report.evaluation.is_valid());
    let entries = evals
        .evidence_for_pointer(&ctx, sidecar.id, &JsonPointer::parse("/name").unwrap())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);

    let extraction = ExtractionRepo::find_by_id(&pool, ctx.workspace_id(), extraction_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        extraction.result.unwrap(),
        serde_json::json!({"name": "Ada"})
    );
}

#[tokio::test]
async fn evaluation_service_records_a_passing_run() {
    let pool = connect().await;
    let ctx = PrincipalContext::local_operator();
    let catalog = ContractCatalogService::new(pool.clone());
    let evals = EvaluationService::new(pool.clone());
    let stored = catalog
        .publish(
            &ctx,
            &sample_contract(&format!("run-{}", Uuid::new_v4().simple())),
        )
        .await
        .unwrap();
    let run = evals
        .record_eval_run(
            &ctx,
            stored.id,
            &[FixtureOutcome {
                fixture_name: "basic".into(),
                actual_data: serde_json::json!({"name": "Ada"}),
                actual_evidence: exact_name_sidecar(),
                actual_decision: Some(EvalDecision::Accept),
            }],
        )
        .await
        .unwrap();
    assert!(run.passed);
    assert_eq!(run.results.len(), 1);
    assert!(run.results[0].actual_data.get("evidence").is_none());
}
