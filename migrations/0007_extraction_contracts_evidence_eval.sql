-- SPDX-License-Identifier: AGPL-3.0-only
-- Wave 2: tenant-safe, append-only persistence for immutable extraction
-- contract versions, JSON-Pointer evidence sidecars, validation reports,
-- fixtures, and evaluation runs. Published contract bytes are hash-addressed.

CREATE FUNCTION struxio_forbid_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'append-only table % does not allow %', TG_TABLE_NAME, TG_OP
        USING ERRCODE = 'P0001';
END;
$$;

-- ---------------------------------------------------------------------------
-- Hash-addressed published contract content (workspace-local).
-- canonical_payload is the exact preimage of content_hash.
-- ---------------------------------------------------------------------------
CREATE TABLE extraction_contract_contents (
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    content_hash TEXT NOT NULL,
    canonical_payload BYTEA NOT NULL,
    spec JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, content_hash),
    CONSTRAINT extraction_contract_contents_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_contract_contents_hash_format
        CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT extraction_contract_contents_payload_not_empty
        CHECK (octet_length(canonical_payload) > 0),
    CONSTRAINT extraction_contract_contents_hash_matches_payload
        CHECK (content_hash = encode(digest(canonical_payload, 'sha256'), 'hex'))
);

CREATE TRIGGER extraction_contract_contents_append_only
    BEFORE UPDATE OR DELETE ON extraction_contract_contents
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_contract_contents_append_only_truncate
    BEFORE TRUNCATE ON extraction_contract_contents
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

-- ---------------------------------------------------------------------------
-- Immutable published versions: (slug, version) labels a content hash.
-- ---------------------------------------------------------------------------
CREATE TABLE extraction_contract_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    slug TEXT NOT NULL,
    version BIGINT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_contract_versions_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_contract_versions_workspace_slug_version_key
        UNIQUE (workspace_id, slug, version),
    CONSTRAINT extraction_contract_versions_workspace_slug_hash_key
        UNIQUE (workspace_id, slug, content_hash),
    CONSTRAINT extraction_contract_versions_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_contract_versions_version_positive
        CHECK (version > 0),
    CONSTRAINT extraction_contract_versions_slug_format
        CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    CONSTRAINT extraction_contract_versions_content_fkey
        FOREIGN KEY (workspace_id, content_hash)
        REFERENCES extraction_contract_contents (workspace_id, content_hash)
);

CREATE INDEX idx_extraction_contract_versions_workspace_slug
    ON extraction_contract_versions (workspace_id, slug);
CREATE INDEX idx_extraction_contract_versions_workspace_hash
    ON extraction_contract_versions (workspace_id, content_hash);

CREATE TRIGGER extraction_contract_versions_append_only
    BEFORE UPDATE OR DELETE ON extraction_contract_versions
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_contract_versions_append_only_truncate
    BEFORE TRUNCATE ON extraction_contract_versions
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

-- ---------------------------------------------------------------------------
-- Evidence sidecars and JSON-Pointer-keyed entries.
-- ---------------------------------------------------------------------------
CREATE TABLE extraction_evidence_sidecars (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    sidecar_version INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_evidence_sidecars_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_evidence_sidecars_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_evidence_sidecars_version_positive
        CHECK (sidecar_version > 0),
    CONSTRAINT extraction_evidence_sidecars_status_known
        CHECK (status IN ('not_requested', 'complete', 'partial', 'unavailable'))
);

CREATE INDEX idx_extraction_evidence_sidecars_workspace_id
    ON extraction_evidence_sidecars (workspace_id);

CREATE TRIGGER extraction_evidence_sidecars_append_only
    BEFORE UPDATE OR DELETE ON extraction_evidence_sidecars
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_evidence_sidecars_append_only_truncate
    BEFORE TRUNCATE ON extraction_evidence_sidecars
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TABLE extraction_evidence_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    sidecar_id UUID NOT NULL,
    json_pointer TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL,
    quote TEXT,
    source TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_evidence_entries_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_evidence_entries_pointer_ordinal_key
        UNIQUE (workspace_id, sidecar_id, json_pointer, ordinal),
    CONSTRAINT extraction_evidence_entries_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_evidence_entries_pointer_format
        CHECK (json_pointer = '' OR json_pointer LIKE '/%'),
    CONSTRAINT extraction_evidence_entries_ordinal_nonnegative
        CHECK (ordinal >= 0),
    CONSTRAINT extraction_evidence_entries_kind_known
        CHECK (kind IN ('exact', 'normalized', 'computed', 'inferred')),
    CONSTRAINT extraction_evidence_entries_quote_not_blank
        CHECK (quote IS NULL OR btrim(quote) <> ''),
    CONSTRAINT extraction_evidence_entries_sidecar_fkey
        FOREIGN KEY (workspace_id, sidecar_id)
        REFERENCES extraction_evidence_sidecars (workspace_id, id)
);

CREATE INDEX idx_extraction_evidence_entries_workspace_pointer
    ON extraction_evidence_entries (workspace_id, json_pointer);
CREATE INDEX idx_extraction_evidence_entries_workspace_sidecar
    ON extraction_evidence_entries (workspace_id, sidecar_id);

CREATE TRIGGER extraction_evidence_entries_append_only
    BEFORE UPDATE OR DELETE ON extraction_evidence_entries
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_evidence_entries_append_only_truncate
    BEFORE TRUNCATE ON extraction_evidence_entries
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TABLE extraction_evidence_attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    extraction_id UUID NOT NULL,
    sidecar_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_evidence_attachments_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_evidence_attachments_extraction_sidecar_key
        UNIQUE (workspace_id, extraction_id, sidecar_id),
    CONSTRAINT extraction_evidence_attachments_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_evidence_attachments_extraction_fkey
        FOREIGN KEY (workspace_id, extraction_id)
        REFERENCES extractions (workspace_id, id),
    CONSTRAINT extraction_evidence_attachments_sidecar_fkey
        FOREIGN KEY (workspace_id, sidecar_id)
        REFERENCES extraction_evidence_sidecars (workspace_id, id)
);

CREATE INDEX idx_extraction_evidence_attachments_extraction
    ON extraction_evidence_attachments (workspace_id, extraction_id);

CREATE TRIGGER extraction_evidence_attachments_append_only
    BEFORE UPDATE OR DELETE ON extraction_evidence_attachments
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_evidence_attachments_append_only_truncate
    BEFORE TRUNCATE ON extraction_evidence_attachments
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

-- ---------------------------------------------------------------------------
-- Fixtures projected from a published contract version.
-- expected_data is schema-pure; expected evidence is a sidecar.
-- ---------------------------------------------------------------------------
CREATE TABLE extraction_contract_fixtures (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    contract_version_id UUID NOT NULL,
    name TEXT NOT NULL,
    input_file_name TEXT NOT NULL,
    input_media_type TEXT NOT NULL,
    input_content_sha256 TEXT NOT NULL,
    expected_data JSONB NOT NULL,
    expected_sidecar_id UUID NOT NULL,
    expected_decision TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_contract_fixtures_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_contract_fixtures_version_name_key
        UNIQUE (workspace_id, contract_version_id, name),
    CONSTRAINT extraction_contract_fixtures_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_contract_fixtures_name_not_blank
        CHECK (btrim(name) <> ''),
    CONSTRAINT extraction_contract_fixtures_file_name_not_blank
        CHECK (btrim(input_file_name) <> ''),
    CONSTRAINT extraction_contract_fixtures_media_type_not_blank
        CHECK (btrim(input_media_type) <> ''),
    CONSTRAINT extraction_contract_fixtures_hash_format
        CHECK (input_content_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT extraction_contract_fixtures_decision_known
        CHECK (expected_decision IS NULL OR expected_decision IN ('accept', 'review', 'abstain')),
    CONSTRAINT extraction_contract_fixtures_version_fkey
        FOREIGN KEY (workspace_id, contract_version_id)
        REFERENCES extraction_contract_versions (workspace_id, id),
    CONSTRAINT extraction_contract_fixtures_sidecar_fkey
        FOREIGN KEY (workspace_id, expected_sidecar_id)
        REFERENCES extraction_evidence_sidecars (workspace_id, id)
);

CREATE INDEX idx_extraction_contract_fixtures_version
    ON extraction_contract_fixtures (workspace_id, contract_version_id);

CREATE TRIGGER extraction_contract_fixtures_append_only
    BEFORE UPDATE OR DELETE ON extraction_contract_fixtures
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_contract_fixtures_append_only_truncate
    BEFORE TRUNCATE ON extraction_contract_fixtures
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

-- ---------------------------------------------------------------------------
-- Validation reports for production extractions. Normalized JSON is schema-pure.
-- ---------------------------------------------------------------------------
CREATE TABLE extraction_validation_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    extraction_id UUID NOT NULL,
    contract_version_id UUID NOT NULL,
    sidecar_id UUID,
    normalized_data JSONB NOT NULL,
    schema_report JSONB NOT NULL,
    validation_report JSONB NOT NULL,
    evidence_report JSONB NOT NULL,
    is_valid BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_validation_reports_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_validation_reports_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_validation_reports_extraction_fkey
        FOREIGN KEY (workspace_id, extraction_id)
        REFERENCES extractions (workspace_id, id),
    CONSTRAINT extraction_validation_reports_version_fkey
        FOREIGN KEY (workspace_id, contract_version_id)
        REFERENCES extraction_contract_versions (workspace_id, id),
    CONSTRAINT extraction_validation_reports_sidecar_fkey
        FOREIGN KEY (workspace_id, sidecar_id)
        REFERENCES extraction_evidence_sidecars (workspace_id, id)
);

CREATE INDEX idx_extraction_validation_reports_extraction
    ON extraction_validation_reports (workspace_id, extraction_id);
CREATE INDEX idx_extraction_validation_reports_version
    ON extraction_validation_reports (workspace_id, contract_version_id);

CREATE TRIGGER extraction_validation_reports_append_only
    BEFORE UPDATE OR DELETE ON extraction_validation_reports
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_validation_reports_append_only_truncate
    BEFORE TRUNCATE ON extraction_validation_reports
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

-- ---------------------------------------------------------------------------
-- Completed evaluation runs (insert-only snapshots).
-- ---------------------------------------------------------------------------
CREATE TABLE extraction_eval_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    contract_version_id UUID NOT NULL,
    schema_valid_rate_bps INTEGER NOT NULL,
    field_accuracy_bps INTEGER NOT NULL,
    evidence_coverage_bps INTEGER NOT NULL,
    abstain_rate_bps INTEGER NOT NULL,
    passed BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_eval_runs_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_eval_runs_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_eval_runs_schema_bps
        CHECK (schema_valid_rate_bps BETWEEN 0 AND 10000),
    CONSTRAINT extraction_eval_runs_field_bps
        CHECK (field_accuracy_bps BETWEEN 0 AND 10000),
    CONSTRAINT extraction_eval_runs_evidence_bps
        CHECK (evidence_coverage_bps BETWEEN 0 AND 10000),
    CONSTRAINT extraction_eval_runs_abstain_bps
        CHECK (abstain_rate_bps BETWEEN 0 AND 10000),
    CONSTRAINT extraction_eval_runs_version_fkey
        FOREIGN KEY (workspace_id, contract_version_id)
        REFERENCES extraction_contract_versions (workspace_id, id)
);

CREATE INDEX idx_extraction_eval_runs_version
    ON extraction_eval_runs (workspace_id, contract_version_id);

CREATE TRIGGER extraction_eval_runs_append_only
    BEFORE UPDATE OR DELETE ON extraction_eval_runs
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_eval_runs_append_only_truncate
    BEFORE TRUNCATE ON extraction_eval_runs
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TABLE extraction_eval_run_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces (id),
    eval_run_id UUID NOT NULL,
    fixture_id UUID NOT NULL,
    sidecar_id UUID NOT NULL,
    actual_data JSONB NOT NULL,
    actual_decision TEXT,
    schema_valid BOOLEAN NOT NULL,
    fields_match BOOLEAN NOT NULL,
    evidence_satisfied BOOLEAN NOT NULL,
    schema_report JSONB NOT NULL,
    validation_report JSONB NOT NULL,
    evidence_report JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_eval_run_results_workspace_id_id_key
        UNIQUE (workspace_id, id),
    CONSTRAINT extraction_eval_run_results_run_fixture_key
        UNIQUE (workspace_id, eval_run_id, fixture_id),
    CONSTRAINT extraction_eval_run_results_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT extraction_eval_run_results_decision_known
        CHECK (actual_decision IS NULL OR actual_decision IN ('accept', 'review', 'abstain')),
    CONSTRAINT extraction_eval_run_results_run_fkey
        FOREIGN KEY (workspace_id, eval_run_id)
        REFERENCES extraction_eval_runs (workspace_id, id),
    CONSTRAINT extraction_eval_run_results_fixture_fkey
        FOREIGN KEY (workspace_id, fixture_id)
        REFERENCES extraction_contract_fixtures (workspace_id, id),
    CONSTRAINT extraction_eval_run_results_sidecar_fkey
        FOREIGN KEY (workspace_id, sidecar_id)
        REFERENCES extraction_evidence_sidecars (workspace_id, id)
);

CREATE INDEX idx_extraction_eval_run_results_run
    ON extraction_eval_run_results (workspace_id, eval_run_id);

CREATE TRIGGER extraction_eval_run_results_append_only
    BEFORE UPDATE OR DELETE ON extraction_eval_run_results
    FOR EACH ROW
    EXECUTE FUNCTION struxio_forbid_mutation();

CREATE TRIGGER extraction_eval_run_results_append_only_truncate
    BEFORE TRUNCATE ON extraction_eval_run_results
    FOR EACH STATEMENT
    EXECUTE FUNCTION struxio_forbid_mutation();
