// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::BTreeMap;

use sqlx::PgPool;
use struxio_common::{WorkspaceId, LOCAL_WORKSPACE_UUID};
use struxio_contracts::{
    ContractSlug, EvalDecision, EvalThresholds, EvidenceEntry, EvidenceKind, EvidencePolicy,
    EvidenceSidecar, ExtractionContract, FixtureDescriptor, FixtureInput, JsonPointer, JsonSchema,
    PositiveVersion, ProviderNeutralInstructions, Sha256ContentHash,
};
use struxio_db::repositories::{
    contracts::ContractRepo,
    evaluations::{EvaluationFixtureRepo, EvaluationRunRepo},
    evidence::EvidenceSidecarRepo,
    extractions::ExtractionRepo,
    validation_reports::{ValidationReportPayload, ValidationReportRepo},
};
use uuid::Uuid;

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

fn sample_contract(slug: String) -> ExtractionContract {
    let name = JsonPointer::parse("/name").unwrap();
    let schema = JsonSchema::new(serde_json::json!({
        "type": "object",
        "required": ["name"],
        "properties": {"name": {"type": "string"}}
    }))
    .unwrap();
    let instructions = ProviderNeutralInstructions::new("Extract the name").unwrap();
    let spec = struxio_contracts::ContractSpec::new(schema, instructions)
        .with_evidence_policy(EvidencePolicy::required(vec![name]));
    ExtractionContract::new(
        ContractSlug::new(slug).unwrap(),
        PositiveVersion::new(1).unwrap(),
        spec,
    )
}

fn evidence() -> EvidenceSidecar {
    EvidenceSidecar::from_entries(BTreeMap::new()).with(
        JsonPointer::parse("/name").unwrap(),
        EvidenceEntry::new(
            EvidenceKind::Exact,
            Some("Ada".to_owned()),
            Some("page-1".to_owned()),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn persists_contract_evidence_validation_and_evaluation_separately() {
    let pool = connect().await;
    let slug = format!("wave-two-{}", Uuid::new_v4().simple());
    let contract = sample_contract(slug);
    let stored_contract = ContractRepo::publish(&pool, WorkspaceId::local(), &contract)
        .await
        .expect("publish contract");

    let loaded = ContractRepo::find_by_identity(
        &pool,
        WorkspaceId::local(),
        contract.identity().slug().as_str(),
        1,
        contract.content_hash(),
    )
    .await
    .expect("load contract")
    .expect("published contract");
    assert_eq!(loaded.contract, contract);
    assert_eq!(loaded.identity, *contract.identity());

    let fixture = FixtureDescriptor::new(
        "happy-path",
        FixtureInput::new(
            "person.pdf",
            "application/pdf",
            Sha256ContentHash::from_content(b"pdf"),
        )
        .unwrap(),
        serde_json::json!({"name": "Ada"}),
        evidence(),
        Some(EvalDecision::Accept),
    )
    .unwrap();
    let stored_fixture = EvaluationFixtureRepo::append(
        &pool,
        WorkspaceId::local(),
        stored_contract.id,
        contract.content_hash(),
        &fixture,
    )
    .await
    .expect("append fixture");

    let template_id: Uuid =
        sqlx::query_scalar("SELECT id FROM extraction_templates WHERE workspace_id = $1 LIMIT 1")
            .bind(LOCAL_WORKSPACE_UUID)
            .fetch_one(&pool)
            .await
            .expect("local template");
    let document_id: Uuid = sqlx::query_scalar(
        "INSERT INTO documents \
         (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'person.pdf', 'application/pdf', $3, 3) RETURNING id",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(format!("wave-two-{}", Uuid::new_v4().simple()))
    .bind(format!("wave-two/{}", Uuid::new_v4()))
    .fetch_one(&pool)
    .await
    .expect("document");
    let extraction = ExtractionRepo::create_with_contract(
        &pool,
        WorkspaceId::local(),
        document_id,
        template_id,
        None,
        stored_contract.id,
        contract.content_hash(),
    )
    .await
    .expect("contract-bound extraction");

    let sidecar = evidence();
    let stored_sidecar = EvidenceSidecarRepo::append(
        &pool,
        WorkspaceId::local(),
        extraction.id,
        stored_contract.id,
        contract.content_hash(),
        &sidecar,
    )
    .await
    .expect("append evidence sidecar");

    let candidate = contract
        .evaluate_candidate(&serde_json::json!({"name": "Ada"}), &sidecar)
        .expect("evaluate candidate");
    assert!(candidate.is_valid());
    let validation = ValidationReportPayload::from_candidate(&candidate);
    let stored_extraction_report = ValidationReportRepo::append_for_extraction(
        &pool,
        WorkspaceId::local(),
        extraction.id,
        stored_contract.id,
        contract.content_hash(),
        &validation,
    )
    .await
    .expect("append extraction validation report");

    let run = EvaluationRunRepo::start(
        &pool,
        WorkspaceId::local(),
        stored_contract.id,
        contract.content_hash(),
        EvalThresholds::strict(),
    )
    .await
    .expect("start evaluation run");
    let result = EvaluationRunRepo::append_candidate_result(
        &pool,
        WorkspaceId::local(),
        run.id,
        stored_fixture.id,
        stored_contract.id,
        contract.content_hash(),
        &candidate,
        &sidecar,
        Some(EvalDecision::Accept),
    )
    .await
    .expect("append evaluation result");
    ValidationReportRepo::append_for_evaluation_result(
        &pool,
        WorkspaceId::local(),
        result.id,
        stored_contract.id,
        contract.content_hash(),
        &validation,
    )
    .await
    .expect("append evaluation validation report");

    let stored_result = EvaluationRunRepo::list_results(&pool, WorkspaceId::local(), run.id)
        .await
        .expect("list evaluation results");
    assert_eq!(stored_result.len(), 1);
    assert_eq!(stored_result[0].result, serde_json::json!({"name": "Ada"}));
    assert!(stored_result[0].result.get("evidence").is_none());
    assert_eq!(stored_result[0].evidence, sidecar);
    assert_eq!(
        EvidenceSidecarRepo::find_for_extraction(&pool, WorkspaceId::local(), extraction.id,)
            .await
            .unwrap()
            .unwrap()
            .id,
        stored_sidecar.id
    );
    assert_eq!(
        ValidationReportRepo::find_for_extraction(&pool, WorkspaceId::local(), extraction.id,)
            .await
            .unwrap()
            .unwrap()
            .id,
        stored_extraction_report.id
    );
}

#[tokio::test]
async fn tenant_fks_and_append_only_contracts_are_enforced() {
    let pool = connect().await;
    let slug = format!("wave-two-fk-{}", Uuid::new_v4().simple());
    let contract = sample_contract(slug);
    let stored = ContractRepo::publish(&pool, WorkspaceId::local(), &contract)
        .await
        .expect("publish contract");
    let other_workspace = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
        .bind(other_workspace)
        .bind(format!("wave-two-{}", other_workspace.simple()))
        .bind("Wave 2 test workspace")
        .execute(&pool)
        .await
        .expect("other workspace");

    let cross_tenant = EvaluationRunRepo::start(
        &pool,
        struxio_common::WorkspaceId::new(other_workspace).unwrap(),
        stored.id,
        contract.content_hash(),
        EvalThresholds::strict(),
    )
    .await
    .expect_err("composite contract FK must reject cross-tenant reference");
    assert!(cross_tenant
        .to_string()
        .to_lowercase()
        .contains("foreign key"));

    let immutable = sqlx::query(
        "UPDATE extraction_contracts SET slug = 'mutated' WHERE workspace_id = $1 AND id = $2",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(stored.id)
    .execute(&pool)
    .await
    .expect_err("published contracts must be immutable");
    assert!(immutable.to_string().contains("append-only"));
}
