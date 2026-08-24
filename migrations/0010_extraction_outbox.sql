-- SPDX-License-Identifier: AGPL-3.0-only

-- Batch creation writes extraction jobs here in the same Postgres transaction
-- as the batch and extraction rows. Publishing to Redis is then retryable and
-- at-least-once instead of being an unsafe cross-system partial commit.
CREATE TABLE extraction_outbox (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL,
    extraction_id UUID NOT NULL,
    document_id UUID NOT NULL,
    template_id UUID NOT NULL,
    batch_job_id UUID,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT extraction_outbox_attempts_non_negative CHECK (attempts >= 0),
    CONSTRAINT extraction_outbox_workspace_extraction_key
        UNIQUE (workspace_id, extraction_id),
    CONSTRAINT extraction_outbox_workspace_extraction_fkey
        FOREIGN KEY (workspace_id, extraction_id)
        REFERENCES extractions (workspace_id, id)
        ON DELETE CASCADE
);

CREATE INDEX idx_extraction_outbox_pending
    ON extraction_outbox (created_at, id)
    WHERE published_at IS NULL;
