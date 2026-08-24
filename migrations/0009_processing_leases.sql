-- SPDX-License-Identifier: AGPL-3.0-only

-- A processing lease prevents duplicate workers from executing the same
-- extraction while still allowing recovery after a crashed worker.
ALTER TABLE extractions
    ADD COLUMN processing_lease_expires_at TIMESTAMPTZ;

CREATE INDEX idx_extractions_processing_lease
    ON extractions (processing_lease_expires_at)
    WHERE status = 'processing';
