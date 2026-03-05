ALTER TABLE extractions ADD COLUMN batch_job_id UUID REFERENCES batch_jobs(id) ON DELETE SET NULL;
CREATE INDEX idx_extractions_batch_job_id ON extractions(batch_job_id);
