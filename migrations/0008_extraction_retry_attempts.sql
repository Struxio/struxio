-- SPDX-License-Identifier: AGPL-3.0-only
-- Track retry attempts on extraction jobs so reclaim does not lose try counts
-- and batch status can distinguish retrying work from terminal failure.

ALTER TABLE extractions
    ADD COLUMN attempt INTEGER NOT NULL DEFAULT 0;

ALTER TABLE extractions
    ADD CONSTRAINT extractions_attempt_non_negative CHECK (attempt >= 0);

CREATE INDEX idx_extractions_workspace_batch_status
    ON extractions (workspace_id, batch_job_id, status);
