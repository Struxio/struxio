// SPDX-License-Identifier: AGPL-3.0-only

//! PostgreSQL coverage for Wave 2 contract, evidence, and evaluation persistence.
//! Requires `DATABASE_URL`.

use std::collections::BTreeMap;

use sqlx::PgPool;
use uuid::Uuid;

use struxio_common::{WorkspaceId, LOCAL_WORKSPACE_UUID};
use struxio_contracts::{
    ContractSlug, ContractSpec, EvalDecision, EvalThresholds, EvidenceEntry, EvidenceKind,
    EvidencePolicy, EvidenceSidecar, EvidenceSidecarStatus, ExtractionContract, FixtureDescriptor,
    FixtureInput, JsonPointer, JsonSchema, JsonValueType, PositiveVersion,
    ProviderNeutralInstructions, Sha256ContentHash, Validator, ValidatorRule,
};
use struxio_db::repositories::contracts::ContractRepo;
use struxio_db::repositories::evaluations::{EvaluationRepo, ValidationRepo};
use struxio_db::repositories::evidence::EvidenceRepo;
use struxio_db::repositories::extractions::ExtractionRepo;
use struxio_db::{compute_eval_run_metrics, NewEvalRunResult, NewValidationReport};

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

async fn insert_workspace(pool: &PgPool, id: Uuid, slug: &str) {
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(slug)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert workspace");
}

fn sample_spec(name: &str) -> ContractSpec {
    let pointer = JsonPointer::parse("/name").unwrap();
    ContractSpec::new(
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
            name,
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
    )
}

fn sample_contract(slug: &str, version: u32, fixture_name: &str) -> ExtractionContract {
    ExtractionContract::new(
        ContractSlug::new(slug).unwrap(),
        PositiveVersion::new(version).unwrap(),
        sample_spec(fixture_name),
    )
}

fn exact_name_sidecar() -> EvidenceSidecar {
    EvidenceSidecar::from_entries(BTreeMap::new()).with(
        JsonPointer::parse("/name").unwrap(),
        EvidenceEntry::new(EvidenceKind::Exact, Some("Ada".into()), Some("p1".into())).unwrap(),
    )
}

async fn seed_extraction(pool: &PgPool, workspace_id: WorkspaceId) -> Uuid {
    let template_id: Uuid =
        sqlx::query_scalar("SELECT id FROM extraction_templates WHERE workspace_id = $1 LIMIT 1")
            .bind(workspace_id.as_uuid())
            .fetch_one(pool)
            .await
            .expect("template");
    let doc_id: Uuid = sqlx::query_scalar(
        "INSERT INTO documents (workspace_id, md5_hash, file_name, file_type, s3_key, size_bytes) \
         VALUES ($1, $2, 'a.pdf', 'pdf', $3, 1) RETURNING id",
    )
    .bind(workspace_id.as_uuid())
    .bind(format!("md5-{}", Uuid::new_v4().simple()))
    .bind(format!("s3-{}", Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .expect("document");
    ExtractionRepo::create(pool, workspace_id, doc_id, template_id, None)
        .await
        .expect("extraction")
        .id
}

#[tokio::test]
async fn published_contract_is_hash_addressed_and_round_trips() {
    let pool = connect().await;
    let workspace = WorkspaceId::local();
    let slug = format!("person-{}", Uuid::new_v4().simple());
    let contract = sample_contract(&slug, 1, "basic");
    let stored = ContractRepo::publish(&pool, workspace, &contract)
        .await
        .expect("publish");

    assert_eq!(stored.contract.content_hash(), contract.content_hash());
    assert!(stored.contract.verify_content_hash());
    assert_eq!(
        stored.contract.canonical_payload(),
        contract.canonical_payload()
    );

    let loaded = ContractRepo::find_by_slug_version(
        &pool,
        workspace,
        contract.identity().slug().as_str(),
        contract.identity().version(),
    )
    .await
    .unwrap()
    .expect("loaded");
    assert_eq!(loaded.contract, contract);

    let by_hash = ContractRepo::find_by_content_hash(&pool, workspace, contract.content_hash())
        .await
        .unwrap();
    assert!(
        by_hash.iter().any(|row| row.id == stored.id),
        "hash lookup must include the published version"
    );
    assert!(by_hash
        .iter()
        .all(|row| row.contract.content_hash() == contract.content_hash()));

    let fixtures = ContractRepo::list_fixtures(&pool, workspace, stored.id)
        .await
        .unwrap();
    assert_eq!(fixtures.len(), 1);
    assert_eq!(fixtures[0].descriptor.name(), "basic");
    assert_eq!(
        fixtures[0].descriptor.expected_data(),
        &serde_json::json!({"name": "Ada"})
    );
    assert!(fixtures[0]
        .descriptor
        .expected_data()
        .get("evidence")
        .is_none());
}

#[tokio::test]
async fn identical_specs_reuse_hash_addressed_content_in_a_workspace() {
    let pool = connect().await;
    let workspace = WorkspaceId::local();
    let unique = Uuid::new_v4().simple().to_string();
    let first = sample_contract(&format!("alpha-{unique}"), 1, &format!("fix-{unique}"));
    let second = sample_contract(&format!("beta-{unique}"), 1, &format!("fix-{unique}"));
    assert_eq!(first.content_hash(), second.content_hash());

    let stored_first = ContractRepo::publish(&pool, workspace, &first)
        .await
        .unwrap();
    let stored_second = ContractRepo::publish(&pool, workspace, &second)
        .await
        .unwrap();
    assert_ne!(stored_first.id, stored_second.id);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM extraction_contract_contents \
         WHERE workspace_id = $1 AND content_hash = $2",
    )
    .bind(workspace.as_uuid())
    .bind(first.content_hash().as_hex())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    let labeled = ContractRepo::find_by_content_hash(&pool, workspace, first.content_hash())
        .await
        .unwrap();
    assert!(labeled.iter().any(|row| row.id == stored_first.id));
    assert!(labeled.iter().any(|row| row.id == stored_second.id));
}

#[tokio::test]
async fn published_contract_rows_are_append_only() {
    let pool = connect().await;
    let workspace = WorkspaceId::local();
    let contract = sample_contract(&format!("immut-{}", Uuid::new_v4().simple()), 1, "basic");
    let stored = ContractRepo::publish(&pool, workspace, &contract)
        .await
        .unwrap();

    let update_err =
        sqlx::query("UPDATE extraction_contract_versions SET slug = 'nope' WHERE id = $1")
            .bind(stored.id)
            .execute(&pool)
            .await
            .expect_err("update version");
    assert!(update_err.to_string().contains("append-only"));

    let delete_err =
        sqlx::query("DELETE FROM extraction_contract_contents WHERE content_hash = $1")
            .bind(contract.content_hash().as_hex())
            .execute(&pool)
            .await
            .expect_err("delete content");
    assert!(delete_err.to_string().contains("append-only"));
}

#[tokio::test]
async fn postgres_rejects_content_hash_that_does_not_match_payload() {
    let pool = connect().await;
    let payload = b"{\"canonical_hash_version\":1,\"spec\":{}}";
    let err = sqlx::query(
        "INSERT INTO extraction_contract_contents \
         (workspace_id, content_hash, canonical_payload, spec) \
         VALUES ($1, $2, $3, '{}'::jsonb)",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind("0".repeat(64))
    .bind(&payload[..])
    .execute(&pool)
    .await
    .expect_err("mismatched hash");
    assert!(
        err.to_string()
            .contains("extraction_contract_contents_hash_matches_payload")
            || err.to_string().to_lowercase().contains("check")
    );
}

#[tokio::test]
async fn contract_identity_is_workspace_local() {
    let pool = connect().await;
    let other =
        WorkspaceId::new(Uuid::from_u128(0xaaa1_bbb2_4cc3_8dd4_eee5_fff6_0007_1118)).unwrap();
    insert_workspace(
        &pool,
        other.as_uuid(),
        &format!("c-{}", other.as_uuid().simple()),
    )
    .await;

    let slug = format!("shared-{}", Uuid::new_v4().simple());
    let contract = sample_contract(&slug, 1, "basic");
    ContractRepo::publish(&pool, WorkspaceId::local(), &contract)
        .await
        .unwrap();
    ContractRepo::publish(&pool, other, &contract)
        .await
        .expect("same slug/version in another workspace");

    let dup = ContractRepo::publish(&pool, WorkspaceId::local(), &contract)
        .await
        .expect_err("duplicate version");
    assert!(
        dup.to_string().contains("duplicate")
            || matches!(dup, struxio_db::ContractStoreError::Conflict(_))
    );

    let local = ContractRepo::find_by_slug_version(
        &pool,
        WorkspaceId::local(),
        &slug,
        PositiveVersion::new(1).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(ContractRepo::find_by_id(&pool, other, local.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn nil_workspace_rejected_on_contract_content() {
    let pool = connect().await;
    let payload = b"abc";
    let err = sqlx::query(
        "INSERT INTO extraction_contract_contents \
         (workspace_id, content_hash, canonical_payload, spec) \
         VALUES ($1, $2, $3, '{}'::jsonb)",
    )
    .bind(Uuid::nil())
    .bind("a".repeat(64))
    .bind(&payload[..])
    .execute(&pool)
    .await
    .expect_err("nil workspace");
    assert!(
        err.to_string()
            .contains("extraction_contract_contents_workspace_id_not_nil")
            || err.to_string().to_lowercase().contains("check")
            || err.to_string().to_lowercase().contains("foreign key")
    );
}

#[tokio::test]
async fn evidence_is_keyed_by_json_pointer_and_stays_off_extraction_json() {
    let pool = connect().await;
    let workspace = WorkspaceId::local();
    let extraction_id = seed_extraction(&pool, workspace).await;
    ExtractionRepo::update_completed(
        &pool,
        workspace,
        extraction_id,
        &serde_json::json!({"name": "Ada"}),
        1,
        1,
        10,
        "gemini-2.5-flash",
        0,
    )
    .await
    .unwrap();

    let sidecar = exact_name_sidecar();
    let stored = EvidenceRepo::insert(&pool, workspace, &sidecar)
        .await
        .unwrap();
    EvidenceRepo::attach_to_extraction(&pool, workspace, extraction_id, stored.id)
        .await
        .unwrap();

    let pointer = JsonPointer::parse("/name").unwrap();
    let entries = EvidenceRepo::entries_for_pointer(&pool, workspace, stored.id, &pointer)
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind(), EvidenceKind::Exact);
    assert_eq!(entries[0].quote(), Some("Ada"));

    let missing = EvidenceRepo::entries_for_pointer(
        &pool,
        workspace,
        stored.id,
        &JsonPointer::parse("/total").unwrap(),
    )
    .await
    .unwrap();
    assert!(missing.is_empty());

    let loaded = EvidenceRepo::find_by_id(&pool, workspace, stored.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.sidecar, sidecar);

    let extraction = ExtractionRepo::find_by_id(&pool, workspace, extraction_id)
        .await
        .unwrap()
        .unwrap();
    let result = extraction.result.expect("result");
    assert_eq!(result, serde_json::json!({"name": "Ada"}));
    assert!(result.get("evidence").is_none());

    let update_err =
        sqlx::query("UPDATE extraction_evidence_entries SET quote = 'nope' WHERE sidecar_id = $1")
            .bind(stored.id)
            .execute(&pool)
            .await
            .expect_err("append-only entries");
    assert!(update_err.to_string().contains("append-only"));
}

#[tokio::test]
async fn unavailable_sidecar_round_trips_status_without_inferred_partial() {
    let pool = connect().await;
    let stored = EvidenceRepo::insert(&pool, WorkspaceId::local(), &EvidenceSidecar::unavailable())
        .await
        .unwrap();
    let loaded = EvidenceRepo::find_by_id(&pool, WorkspaceId::local(), stored.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.sidecar.status(), EvidenceSidecarStatus::Unavailable);
    assert!(loaded.sidecar.is_empty());
}

#[tokio::test]
async fn json_pointer_format_is_enforced() {
    let pool = connect().await;
    let sidecar = EvidenceRepo::insert(
        &pool,
        WorkspaceId::local(),
        &EvidenceSidecar::not_requested(),
    )
    .await
    .unwrap();
    let err = sqlx::query(
        "INSERT INTO extraction_evidence_entries \
         (workspace_id, sidecar_id, json_pointer, ordinal, kind) \
         VALUES ($1, $2, 'name', 0, 'exact')",
    )
    .bind(LOCAL_WORKSPACE_UUID)
    .bind(sidecar.id)
    .execute(&pool)
    .await
    .expect_err("bad pointer");
    assert!(
        err.to_string()
            .contains("extraction_evidence_entries_pointer_format")
            || err.to_string().to_lowercase().contains("check")
    );
}

#[tokio::test]
async fn evidence_attachment_rejects_cross_workspace_extraction() {
    let pool = connect().await;
    let other =
        WorkspaceId::new(Uuid::from_u128(0x2222_3333_4444_8555_1666_7777_8888_9999)).unwrap();
    insert_workspace(
        &pool,
        other.as_uuid(),
        &format!("e-{}", other.as_uuid().simple()),
    )
    .await;

    let local_extraction = seed_extraction(&pool, WorkspaceId::local()).await;
    let sidecar = EvidenceRepo::insert(&pool, other, &exact_name_sidecar())
        .await
        .unwrap();
    let err = EvidenceRepo::attach_to_extraction(&pool, other, local_extraction, sidecar.id)
        .await
        .expect_err("cross-workspace attachment");
    let message = err.to_string().to_lowercase();
    assert!(message.contains("foreign key") || message.contains("fkey"));
}

#[tokio::test]
async fn validation_reports_are_append_only_and_schema_pure() {
    let pool = connect().await;
    let workspace = WorkspaceId::local();
    let contract = sample_contract(&format!("val-{}", Uuid::new_v4().simple()), 1, "basic");
    let stored = ContractRepo::publish(&pool, workspace, &contract)
        .await
        .unwrap();
    let extraction_id = seed_extraction(&pool, workspace).await;
    let sidecar = EvidenceRepo::insert(&pool, workspace, &exact_name_sidecar())
        .await
        .unwrap();
    let evaluation = contract
        .evaluate_candidate(&serde_json::json!({"name": "Ada"}), &exact_name_sidecar())
        .unwrap();
    assert!(evaluation.normalized().get("evidence").is_none());

    let report = ValidationRepo::insert(
        &pool,
        workspace,
        NewValidationReport {
            extraction_id,
            contract_version_id: stored.id,
            sidecar_id: Some(sidecar.id),
            evaluation: evaluation.clone(),
        },
    )
    .await
    .unwrap();
    assert!(report.evaluation.is_valid());
    assert_eq!(report.evaluation, evaluation);

    let loaded = ValidationRepo::find_by_id(&pool, workspace, report.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.evaluation.normalized(), evaluation.normalized());

    let err =
        sqlx::query("UPDATE extraction_validation_reports SET is_valid = false WHERE id = $1")
            .bind(report.id)
            .execute(&pool)
            .await
            .expect_err("append-only report");
    assert!(err.to_string().contains("append-only"));
}

#[tokio::test]
async fn evaluation_run_persists_fixture_outcomes_and_metrics() {
    let pool = connect().await;
    let workspace = WorkspaceId::local();
    let contract = sample_contract(&format!("eval-{}", Uuid::new_v4().simple()), 1, "basic");
    let stored = ContractRepo::publish(&pool, workspace, &contract)
        .await
        .unwrap();
    let fixtures = ContractRepo::list_fixtures(&pool, workspace, stored.id)
        .await
        .unwrap();
    let sidecar = EvidenceRepo::insert(&pool, workspace, &exact_name_sidecar())
        .await
        .unwrap();
    let evaluation = contract
        .evaluate_candidate(&serde_json::json!({"name": "Ada"}), &exact_name_sidecar())
        .unwrap();
    let results = vec![NewEvalRunResult {
        fixture_id: fixtures[0].id,
        sidecar_id: sidecar.id,
        actual_data: serde_json::json!({"name": "Ada"}),
        actual_decision: Some(EvalDecision::Accept),
        schema_valid: evaluation.schema().is_valid(),
        fields_match: evaluation.normalized() == fixtures[0].descriptor.expected_data(),
        evidence_satisfied: evaluation.evidence().is_satisfied(),
        schema_report: evaluation.schema().clone(),
        validation_report: evaluation.validation().clone(),
        evidence_report: evaluation.evidence().clone(),
    }];
    let metrics = compute_eval_run_metrics(contract.spec().eval(), &results);
    let run = EvaluationRepo::insert_completed_run(&pool, workspace, stored.id, metrics, &results)
        .await
        .unwrap();
    assert!(run.passed);
    assert_eq!(run.schema_valid_rate_bps, 10_000);
    assert_eq!(run.field_accuracy_bps, 10_000);
    assert_eq!(run.evidence_coverage_bps, 10_000);
    assert_eq!(run.results.len(), 1);
    assert!(run.results[0].actual_data.get("evidence").is_none());

    let loaded = EvaluationRepo::find_by_id(&pool, workspace, run.id)
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.results[0].fields_match);

    let err = sqlx::query("DELETE FROM extraction_eval_runs WHERE id = $1")
        .bind(run.id)
        .execute(&pool)
        .await
        .expect_err("append-only eval run");
    assert!(err.to_string().contains("append-only"));
}

#[tokio::test]
async fn eval_runs_are_isolated_by_workspace() {
    let pool = connect().await;
    let other =
        WorkspaceId::new(Uuid::from_u128(0x3456_789a_4bcd_8ef0_1234_5678_9abc_def0)).unwrap();
    insert_workspace(
        &pool,
        other.as_uuid(),
        &format!("r-{}", other.as_uuid().simple()),
    )
    .await;

    let slug = format!("iso-{}", Uuid::new_v4().simple());
    let contract = sample_contract(&slug, 1, "basic");
    let local = ContractRepo::publish(&pool, WorkspaceId::local(), &contract)
        .await
        .unwrap();
    let remote = ContractRepo::publish(&pool, other, &contract)
        .await
        .unwrap();

    let fixtures = ContractRepo::list_fixtures(&pool, WorkspaceId::local(), local.id)
        .await
        .unwrap();
    let sidecar = EvidenceRepo::insert(&pool, WorkspaceId::local(), &exact_name_sidecar())
        .await
        .unwrap();
    let evaluation = contract
        .evaluate_candidate(&serde_json::json!({"name": "Ada"}), &exact_name_sidecar())
        .unwrap();
    let results = vec![NewEvalRunResult {
        fixture_id: fixtures[0].id,
        sidecar_id: sidecar.id,
        actual_data: serde_json::json!({"name": "Ada"}),
        actual_decision: Some(EvalDecision::Accept),
        schema_valid: true,
        fields_match: true,
        evidence_satisfied: true,
        schema_report: evaluation.schema().clone(),
        validation_report: evaluation.validation().clone(),
        evidence_report: evaluation.evidence().clone(),
    }];
    let metrics = compute_eval_run_metrics(contract.spec().eval(), &results);
    let run = EvaluationRepo::insert_completed_run(
        &pool,
        WorkspaceId::local(),
        local.id,
        metrics,
        &results,
    )
    .await
    .unwrap();

    assert!(EvaluationRepo::find_by_id(&pool, other, run.id)
        .await
        .unwrap()
        .is_none());
    assert!(
        EvaluationRepo::list_for_contract_version(&pool, other, remote.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        EvaluationRepo::list_for_contract_version(&pool, WorkspaceId::local(), local.id)
            .await
            .unwrap()
            .len(),
        1
    );
}
