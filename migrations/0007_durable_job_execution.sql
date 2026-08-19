-- Durable worker bookkeeping. The queue carries the same stable job identity
-- (the extraction id) across all Redis deliveries; these columns make attempt
-- and retry state visible and recoverable from Postgres as well.
ALTER TABLE extractions
    ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN last_attempt_at TIMESTAMPTZ,
    ADD COLUMN last_error_class TEXT,
    ADD COLUMN next_retry_at TIMESTAMPTZ;

ALTER TABLE extractions
    ADD CONSTRAINT extractions_attempt_count_nonnegative CHECK (attempt_count >= 0);

CREATE INDEX idx_extractions_processing_lease
    ON extractions (status, last_attempt_at)
    WHERE status IN ('pending', 'processing');
