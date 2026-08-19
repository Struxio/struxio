-- SPDX-License-Identifier: AGPL-3.0-only
-- Wave 2: immutable contract publications and append-only evidence/evaluation
-- records. Every reference carries workspace_id so tenant boundaries are
-- enforced by PostgreSQL rather than by repository query conventions.

CREATE TABLE extraction_contracts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    slug TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    content_sha256 BYTEA NOT NULL CHECK (octet_length(content_sha256) = 32),
    contract_json JSONB NOT NULL,
    published_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_contracts_id_not_nil
        CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_contracts_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_contracts_identity_key
        UNIQUE (workspace_id, slug, version),
    CONSTRAINT extraction_contracts_workspace_id_id_hash_key
        UNIQUE (workspace_id, id, content_sha256),
    CONSTRAINT extraction_contracts_slug_matches_json
        CHECK (contract_json #>> '{identity,slug}' = slug),
    CONSTRAINT extraction_contracts_version_matches_json
        CHECK (contract_json #>> '{identity,version}' = version::text),
    CONSTRAINT extraction_contracts_hash_matches_json
        CHECK (contract_json #>> '{identity,content_hash}' = encode(content_sha256, 'hex'))
);

CREATE INDEX idx_extraction_contracts_workspace
    ON extraction_contracts (workspace_id, slug, version DESC);

ALTER TABLE extractions
    ADD COLUMN contract_id UUID,
    ADD COLUMN contract_content_sha256 BYTEA,
    ADD CONSTRAINT extractions_contract_hash_length
        CHECK (contract_content_sha256 IS NULL OR octet_length(contract_content_sha256) = 32),
    ADD CONSTRAINT extractions_workspace_contract_fkey
        FOREIGN KEY (workspace_id, contract_id, contract_content_sha256)
        REFERENCES extraction_contracts (workspace_id, id, content_sha256);

ALTER TABLE extractions
    ADD CONSTRAINT extractions_workspace_id_id_contract_key
        UNIQUE (workspace_id, id, contract_id, contract_content_sha256);

CREATE TABLE evaluation_fixtures (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    contract_id UUID NOT NULL,
    contract_content_sha256 BYTEA NOT NULL CHECK (octet_length(contract_content_sha256) = 32),
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    input_file_name TEXT NOT NULL CHECK (length(trim(input_file_name)) > 0),
    input_media_type TEXT NOT NULL CHECK (length(trim(input_media_type)) > 0),
    input_content_sha256 BYTEA NOT NULL CHECK (octet_length(input_content_sha256) = 32),
    expected_result JSONB NOT NULL,
    expected_evidence JSONB NOT NULL,
    expected_decision TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT evaluation_fixtures_id_not_nil
        CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT evaluation_fixtures_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT evaluation_fixtures_contract_fkey
        FOREIGN KEY (workspace_id, contract_id, contract_content_sha256)
        REFERENCES extraction_contracts (workspace_id, id, content_sha256),
    CONSTRAINT evaluation_fixtures_workspace_id_id_contract_key
        UNIQUE (workspace_id, id, contract_id, contract_content_sha256),
    CONSTRAINT evaluation_fixtures_contract_name_key
        UNIQUE (workspace_id, contract_id, contract_content_sha256, name)
);

CREATE TABLE evaluation_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    contract_id UUID NOT NULL,
    contract_content_sha256 BYTEA NOT NULL CHECK (octet_length(contract_content_sha256) = 32),
    policy_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT evaluation_runs_id_not_nil
        CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT evaluation_runs_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT evaluation_runs_contract_fkey
        FOREIGN KEY (workspace_id, contract_id, contract_content_sha256)
        REFERENCES extraction_contracts (workspace_id, id, content_sha256),
    CONSTRAINT evaluation_runs_workspace_id_id_contract_key
        UNIQUE (workspace_id, id, contract_id, contract_content_sha256)
);

CREATE TABLE evaluation_run_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    evaluation_run_id UUID NOT NULL,
    fixture_id UUID NOT NULL,
    contract_id UUID NOT NULL,
    contract_content_sha256 BYTEA NOT NULL CHECK (octet_length(contract_content_sha256) = 32),
    result_json JSONB NOT NULL,
    evidence_json JSONB NOT NULL,
    decision TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT evaluation_run_results_id_not_nil
        CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT evaluation_run_results_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT evaluation_run_results_run_fkey
        FOREIGN KEY (
            workspace_id, evaluation_run_id, contract_id, contract_content_sha256
        )
        REFERENCES evaluation_runs (
            workspace_id, id, contract_id, contract_content_sha256
        ),
    CONSTRAINT evaluation_run_results_fixture_fkey
        FOREIGN KEY (
            workspace_id, fixture_id, contract_id, contract_content_sha256
        )
        REFERENCES evaluation_fixtures (
            workspace_id, id, contract_id, contract_content_sha256
        ),
    CONSTRAINT evaluation_run_results_workspace_id_id_contract_key
        UNIQUE (workspace_id, id, contract_id, contract_content_sha256),
    CONSTRAINT evaluation_run_results_fixture_once_key
        UNIQUE (workspace_id, evaluation_run_id, fixture_id)
);

CREATE TABLE extraction_evidence_sidecars (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    extraction_id UUID NOT NULL,
    contract_id UUID NOT NULL,
    contract_content_sha256 BYTEA NOT NULL CHECK (octet_length(contract_content_sha256) = 32),
    sidecar_json JSONB NOT NULL,
    sidecar_sha256 BYTEA NOT NULL CHECK (octet_length(sidecar_sha256) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_evidence_sidecars_id_not_nil
        CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_evidence_sidecars_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_evidence_sidecars_extraction_fkey
        FOREIGN KEY (
            workspace_id, extraction_id, contract_id, contract_content_sha256
        )
        REFERENCES extractions (
            workspace_id, id, contract_id, contract_content_sha256
        ),
    CONSTRAINT extraction_evidence_sidecars_contract_fkey
        FOREIGN KEY (workspace_id, contract_id, contract_content_sha256)
        REFERENCES extraction_contracts (workspace_id, id, content_sha256),
    CONSTRAINT extraction_evidence_sidecars_extraction_key
        UNIQUE (workspace_id, extraction_id)
);

CREATE TABLE validation_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    extraction_id UUID,
    evaluation_run_result_id UUID,
    contract_id UUID NOT NULL,
    contract_content_sha256 BYTEA NOT NULL CHECK (octet_length(contract_content_sha256) = 32),
    report_json JSONB NOT NULL,
    report_sha256 BYTEA NOT NULL CHECK (octet_length(report_sha256) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT validation_reports_id_not_nil
        CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT validation_reports_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT validation_reports_one_subject
        CHECK ((extraction_id IS NOT NULL) <> (evaluation_run_result_id IS NOT NULL)),
    CONSTRAINT validation_reports_extraction_fkey
        FOREIGN KEY (
            workspace_id, extraction_id, contract_id, contract_content_sha256
        )
        REFERENCES extractions (
            workspace_id, id, contract_id, contract_content_sha256
        ),
    CONSTRAINT validation_reports_evaluation_result_fkey
        FOREIGN KEY (
            workspace_id, evaluation_run_result_id, contract_id, contract_content_sha256
        )
        REFERENCES evaluation_run_results (
            workspace_id, id, contract_id, contract_content_sha256
        )
);

CREATE UNIQUE INDEX validation_reports_extraction_key
    ON validation_reports (workspace_id, extraction_id)
    WHERE extraction_id IS NOT NULL;
CREATE UNIQUE INDEX validation_reports_evaluation_result_key
    ON validation_reports (workspace_id, evaluation_run_result_id)
    WHERE evaluation_run_result_id IS NOT NULL;

CREATE INDEX idx_evaluation_fixtures_contract
    ON evaluation_fixtures (workspace_id, contract_id, created_at);
CREATE INDEX idx_evaluation_runs_contract
    ON evaluation_runs (workspace_id, contract_id, created_at);
CREATE INDEX idx_evaluation_run_results_run
    ON evaluation_run_results (workspace_id, evaluation_run_id, created_at);
CREATE INDEX idx_validation_reports_workspace
    ON validation_reports (workspace_id, created_at);
CREATE INDEX idx_extraction_evidence_sidecars_workspace
    ON extraction_evidence_sidecars (workspace_id, created_at);

CREATE OR REPLACE FUNCTION struxio_reject_append_only_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION '% is append-only; % is not permitted', TG_TABLE_NAME, TG_OP;
END;
$$;

CREATE TRIGGER extraction_contracts_append_only
    BEFORE UPDATE OR DELETE ON extraction_contracts
    FOR EACH ROW EXECUTE FUNCTION struxio_reject_append_only_mutation();
CREATE TRIGGER evaluation_fixtures_append_only
    BEFORE UPDATE OR DELETE ON evaluation_fixtures
    FOR EACH ROW EXECUTE FUNCTION struxio_reject_append_only_mutation();
CREATE TRIGGER evaluation_runs_append_only
    BEFORE UPDATE OR DELETE ON evaluation_runs
    FOR EACH ROW EXECUTE FUNCTION struxio_reject_append_only_mutation();
CREATE TRIGGER evaluation_run_results_append_only
    BEFORE UPDATE OR DELETE ON evaluation_run_results
    FOR EACH ROW EXECUTE FUNCTION struxio_reject_append_only_mutation();
CREATE TRIGGER extraction_evidence_sidecars_append_only
    BEFORE UPDATE OR DELETE ON extraction_evidence_sidecars
    FOR EACH ROW EXECUTE FUNCTION struxio_reject_append_only_mutation();
CREATE TRIGGER validation_reports_append_only
    BEFORE UPDATE OR DELETE ON validation_reports
    FOR EACH ROW EXECUTE FUNCTION struxio_reject_append_only_mutation();
