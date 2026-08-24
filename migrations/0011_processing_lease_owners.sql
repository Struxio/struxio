-- SPDX-License-Identifier: AGPL-3.0-only

-- Lease ownership fences heartbeat and terminal writes from stale workers.
ALTER TABLE extractions
    ADD COLUMN processing_lease_token UUID;

CREATE INDEX idx_extractions_processing_lease_token
    ON extractions (workspace_id, id, processing_lease_token)
    WHERE status = 'processing';
