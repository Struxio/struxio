-- SPDX-License-Identifier: AGPL-3.0-only

-- Rows already processing when lease support was deployed have no owner or
-- expiry. Expire only those legacy rows so a worker can reclaim them once.
-- Live synchronous and worker-owned executions always have a real future
-- expiry and are not touched.
UPDATE extractions
SET processing_lease_expires_at = now()
WHERE status = 'processing'
  AND processing_lease_expires_at IS NULL;
