-- SPDX-License-Identifier: AGPL-3.0-only
-- Attach every tenant-owned row to the seeded local workspace, then enforce
-- workspace-local uniqueness and composite foreign keys. ai_models stays global.

-- 1. Nullable workspace columns (expand)
ALTER TABLE documents
    ADD COLUMN workspace_id UUID REFERENCES workspaces (id);
ALTER TABLE extraction_templates
    ADD COLUMN workspace_id UUID REFERENCES workspaces (id);
ALTER TABLE extractions
    ADD COLUMN workspace_id UUID REFERENCES workspaces (id);
ALTER TABLE batch_jobs
    ADD COLUMN workspace_id UUID REFERENCES workspaces (id);

-- 2. Backfill existing rows, including system templates, to the local workspace
UPDATE documents
SET workspace_id = '79ca2631-6230-5277-9d0e-879be839f744'
WHERE workspace_id IS NULL;

UPDATE extraction_templates
SET workspace_id = '79ca2631-6230-5277-9d0e-879be839f744'
WHERE workspace_id IS NULL;

UPDATE extractions
SET workspace_id = '79ca2631-6230-5277-9d0e-879be839f744'
WHERE workspace_id IS NULL;

UPDATE batch_jobs
SET workspace_id = '79ca2631-6230-5277-9d0e-879be839f744'
WHERE workspace_id IS NULL;

-- 3. Enforce presence and reject nil
ALTER TABLE documents
    ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE extraction_templates
    ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE extractions
    ALTER COLUMN workspace_id SET NOT NULL;
ALTER TABLE batch_jobs
    ALTER COLUMN workspace_id SET NOT NULL;

ALTER TABLE documents
    ADD CONSTRAINT documents_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000');
ALTER TABLE extraction_templates
    ADD CONSTRAINT extraction_templates_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000');
ALTER TABLE extractions
    ADD CONSTRAINT extractions_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000');
ALTER TABLE batch_jobs
    ADD CONSTRAINT batch_jobs_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000');

-- 4. Parent unique (workspace_id, id) required by composite FKs
ALTER TABLE documents
    ADD CONSTRAINT documents_workspace_id_id_key UNIQUE (workspace_id, id);
ALTER TABLE extraction_templates
    ADD CONSTRAINT extraction_templates_workspace_id_id_key UNIQUE (workspace_id, id);
ALTER TABLE extractions
    ADD CONSTRAINT extractions_workspace_id_id_key UNIQUE (workspace_id, id);
ALTER TABLE batch_jobs
    ADD CONSTRAINT batch_jobs_workspace_id_id_key UNIQUE (workspace_id, id);

-- 5. Document identity is unique per workspace, not globally
ALTER TABLE documents
    DROP CONSTRAINT documents_md5_hash_key;
ALTER TABLE documents
    ADD CONSTRAINT documents_workspace_id_md5_hash_key UNIQUE (workspace_id, md5_hash);

-- 6. Replace simple FKs with composite workspace-safe FKs
ALTER TABLE extractions
    DROP CONSTRAINT extractions_document_id_fkey;
ALTER TABLE extractions
    DROP CONSTRAINT extractions_template_id_fkey;
ALTER TABLE extractions
    DROP CONSTRAINT extractions_batch_job_id_fkey;
ALTER TABLE batch_jobs
    DROP CONSTRAINT batch_jobs_template_id_fkey;

ALTER TABLE extractions
    ADD CONSTRAINT extractions_workspace_document_fkey
        FOREIGN KEY (workspace_id, document_id)
        REFERENCES documents (workspace_id, id)
        ON DELETE CASCADE;

ALTER TABLE extractions
    ADD CONSTRAINT extractions_workspace_template_fkey
        FOREIGN KEY (workspace_id, template_id)
        REFERENCES extraction_templates (workspace_id, id)
        ON DELETE RESTRICT;

ALTER TABLE extractions
    ADD CONSTRAINT extractions_workspace_batch_job_fkey
        FOREIGN KEY (workspace_id, batch_job_id)
        REFERENCES batch_jobs (workspace_id, id)
        ON DELETE SET NULL (batch_job_id);

ALTER TABLE batch_jobs
    ADD CONSTRAINT batch_jobs_workspace_template_fkey
        FOREIGN KEY (workspace_id, template_id)
        REFERENCES extraction_templates (workspace_id, id)
        ON DELETE RESTRICT;

-- 7. Lookup indexes
CREATE INDEX idx_documents_workspace_id ON documents (workspace_id);
CREATE INDEX idx_extraction_templates_workspace_id ON extraction_templates (workspace_id);
CREATE INDEX idx_extractions_workspace_id ON extractions (workspace_id);
CREATE INDEX idx_batch_jobs_workspace_id ON batch_jobs (workspace_id);
