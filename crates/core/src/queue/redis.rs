use async_trait::async_trait;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use struxio_common::WorkspaceId;
use uuid::Uuid;

use super::{
    ExtractionJob, QueueConsumer, QueueError, QueueProducer, RetryRequest, DEFAULT_MAX_ATTEMPTS,
};

const EXTRACTION_STREAM: &str = "extractions:queue";
const RETRY_SET: &str = "extractions:retry";
const DLQ_STREAM: &str = "extractions:dlq";

// ── Producer ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RedisProducer {
    client: ::redis::Client,
    max_attempts: u32,
}

impl RedisProducer {
    pub fn new(client: ::redis::Client) -> Self {
        Self {
            client,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }

    pub fn with_max_attempts(client: ::redis::Client, max_attempts: u32) -> Self {
        Self {
            client,
            max_attempts: max_attempts.max(1),
        }
    }
}

#[async_trait]
impl QueueProducer for RedisProducer {
    async fn enqueue_extraction(
        &self,
        extraction_id: Uuid,
        document_id: Uuid,
        template_id: Uuid,
        workspace_id: WorkspaceId,
        batch_job_id: Option<Uuid>,
    ) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let job = ExtractionJob::new(
            extraction_id,
            document_id,
            template_id,
            workspace_id,
            batch_job_id,
            self.max_attempts,
        );
        let batch_id = job
            .batch_job_id
            .map(|id| id.to_string())
            .unwrap_or_default();
        ::redis::cmd("XADD")
            .arg(EXTRACTION_STREAM)
            .arg("*")
            .arg("job_id")
            .arg(job.job_id.to_string())
            .arg("extraction_id")
            .arg(job.extraction_id.to_string())
            .arg("document_id")
            .arg(job.document_id.to_string())
            .arg("template_id")
            .arg(job.template_id.to_string())
            .arg("workspace_id")
            .arg(job.workspace_id.as_uuid().to_string())
            .arg("batch_job_id")
            .arg(batch_id)
            .arg("attempt")
            .arg(job.attempt)
            .arg("max_attempts")
            .arg(job.max_attempts)
            .query_async::<String>(&mut conn)
            .await?;

        Ok(())
    }
}

// ── Consumer ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RedisConsumerConfig {
    pub visibility_timeout: Duration,
    pub max_attempts: u32,
}

impl Default for RedisConsumerConfig {
    fn default() -> Self {
        Self {
            visibility_timeout: Duration::from_secs(300),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }
}

pub struct RedisConsumer {
    client: ::redis::Client,
    group: String,
    consumer: String,
    config: RedisConsumerConfig,
    reclaim_cursor: Mutex<String>,
}

impl RedisConsumer {
    pub fn new(client: ::redis::Client, group: String, consumer: String) -> Self {
        Self::with_config(client, group, consumer, RedisConsumerConfig::default())
    }

    pub fn with_config(
        client: ::redis::Client,
        group: String,
        consumer: String,
        config: RedisConsumerConfig,
    ) -> Self {
        Self {
            client,
            group,
            consumer,
            config: RedisConsumerConfig {
                max_attempts: config.max_attempts.max(1),
                ..config
            },
            reclaim_cursor: Mutex::new("0-0".to_string()),
        }
    }

    async fn promote_due_retries(
        &self,
        conn: &mut ::redis::aio::MultiplexedConnection,
    ) -> Result<(), QueueError> {
        // The Lua script makes ZREM + XADD one durable operation. A process
        // crash cannot remove a retry from the schedule without publishing it.
        const PROMOTE: &str = r#"
            local members = redis.call('ZRANGEBYSCORE', KEYS[1], '-inf', ARGV[1],
                                       'LIMIT', 0, ARGV[2])
            for _, member in ipairs(members) do
                if redis.call('ZREM', KEYS[1], member) == 1 then
                    local job = cjson.decode(member)
                    redis.call('XADD', KEYS[2], '*',
                        'job_id', job.job_id,
                        'extraction_id', job.extraction_id,
                        'document_id', job.document_id,
                        'template_id', job.template_id,
                        'workspace_id', job.workspace_id,
                        'batch_job_id', job.batch_job_id or '',
                        'attempt', job.attempt,
                        'max_attempts', job.max_attempts)
                end
            end
            return #members
        "#;

        let now_ms = chrono::Utc::now().timestamp_millis();
        ::redis::Script::new(PROMOTE)
            .key(RETRY_SET)
            .key(EXTRACTION_STREAM)
            .arg(now_ms)
            .arg(32)
            .invoke_async::<i32>(conn)
            .await?;
        Ok(())
    }

    async fn read_entry(
        &self,
        entry: ::redis::streams::StreamId,
    ) -> Result<ExtractionJob, QueueError> {
        let stream_id = entry.id.clone();
        let parse_uuid = |name: &str| {
            entry
                .get::<String>(name)
                .and_then(|value| Uuid::parse_str(&value).ok())
        };
        let extraction_id =
            parse_uuid("extraction_id").ok_or_else(|| QueueError::InvalidMessage {
                stream_id: stream_id.clone(),
                reason: "missing or invalid extraction_id".to_string(),
            })?;
        let document_id = parse_uuid("document_id").ok_or_else(|| QueueError::InvalidMessage {
            stream_id: stream_id.clone(),
            reason: "missing or invalid document_id".to_string(),
        })?;
        let template_id = parse_uuid("template_id").ok_or_else(|| QueueError::InvalidMessage {
            stream_id: stream_id.clone(),
            reason: "missing or invalid template_id".to_string(),
        })?;
        let workspace_id = parse_uuid("workspace_id")
            .and_then(|id| WorkspaceId::new(id).ok())
            .ok_or_else(|| QueueError::InvalidMessage {
                stream_id: stream_id.clone(),
                reason: "missing or invalid workspace_id".to_string(),
            })?;

        let job_id = parse_uuid("job_id").unwrap_or(extraction_id);
        let attempt = entry
            .get::<String>("attempt")
            .and_then(|value| value.parse().ok())
            .unwrap_or(1);
        let max_attempts = entry
            .get::<String>("max_attempts")
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.config.max_attempts)
            .max(1);

        Ok(ExtractionJob {
            stream_id,
            job_id,
            extraction_id,
            document_id,
            template_id,
            workspace_id,
            batch_job_id: parse_uuid("batch_job_id"),
            attempt: attempt.max(1),
            max_attempts,
        })
    }
}

#[async_trait]
impl QueueConsumer for RedisConsumer {
    async fn ensure_group(&self) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        if let Err(e) = ::redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(EXTRACTION_STREAM)
            .arg(&self.group)
            .arg("0")
            .arg("MKSTREAM")
            .query_async::<()>(&mut conn)
            .await
        {
            if !e.to_string().contains("BUSYGROUP") {
                return Err(e.into());
            }
            // BUSYGROUP = group already exists, fine
        }
        Ok(())
    }

    async fn next_job(&self) -> Result<Option<ExtractionJob>, QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        self.promote_due_retries(&mut conn).await?;

        let reclaim_start = self
            .reclaim_cursor
            .lock()
            .map_err(|_| QueueError::Other("reclaim cursor lock poisoned".to_string()))?
            .clone();
        let reclaimed: ::redis::streams::StreamAutoClaimReply = ::redis::cmd("XAUTOCLAIM")
            .arg(EXTRACTION_STREAM)
            .arg(&self.group)
            .arg(&self.consumer)
            .arg(self.config.visibility_timeout.as_millis() as u64)
            .arg(reclaim_start)
            .arg("COUNT")
            .arg(1)
            .query_async(&mut conn)
            .await?;
        *self
            .reclaim_cursor
            .lock()
            .map_err(|_| QueueError::Other("reclaim cursor lock poisoned".to_string()))? =
            reclaimed.next_stream_id.clone();
        if let Some(entry) = reclaimed.claimed.into_iter().next() {
            return self.read_entry(entry).await.map(Some);
        }

        let result: Option<::redis::streams::StreamReadReply> = ::redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(&self.group)
            .arg(&self.consumer)
            .arg("COUNT")
            .arg(1)
            .arg("BLOCK")
            .arg(5000)
            .arg("STREAMS")
            .arg(EXTRACTION_STREAM)
            .arg(">")
            .query_async(&mut conn)
            .await?;

        let Some(entry) = result
            .and_then(|reply| reply.keys.into_iter().next())
            .and_then(|key| key.ids.into_iter().next())
        else {
            return Ok(None);
        };
        self.read_entry(entry).await.map(Some)
    }

    async fn ack(&self, stream_id: &str) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        ::redis::cmd("XACK")
            .arg(EXTRACTION_STREAM)
            .arg(&self.group)
            .arg(stream_id)
            .query_async::<()>(&mut conn)
            .await?;
        Ok(())
    }

    async fn retry(&self, job: &ExtractionJob, request: RetryRequest) -> Result<(), QueueError> {
        #[derive(Serialize)]
        struct RetryEnvelope<'a> {
            #[serde(flatten)]
            job: &'a ExtractionJob,
            reason: &'a str,
            error_class: &'static str,
        }

        let next_attempt = job.attempt.saturating_add(1);
        let next_job = ExtractionJob {
            stream_id: String::new(),
            attempt: next_attempt,
            ..job.clone()
        };
        let payload = serde_json::to_string(&RetryEnvelope {
            job: &next_job,
            reason: &request.reason,
            error_class: match request.class {
                super::RetryClass::Transient => "transient",
                super::RetryClass::Permanent => "permanent",
            },
        })
        .map_err(|e| QueueError::Other(format!("serialize retry payload: {e}")))?;
        let due_at = chrono::Utc::now()
            .checked_add_signed(
                chrono::Duration::from_std(request.delay)
                    .map_err(|e| QueueError::Other(format!("retry delay: {e}")))?,
            )
            .ok_or_else(|| QueueError::Other("retry delay overflow".to_string()))?
            .timestamp_millis();

        let mut conn = self.client.get_multiplexed_async_connection().await?;
        // ZADD is the durable retry decision; XACK is deliberately in the
        // same transaction so a worker crash cannot lose the current entry.
        ::redis::pipe()
            .atomic()
            .cmd("ZADD")
            .arg(RETRY_SET)
            .arg(due_at)
            .arg(payload)
            .ignore()
            .cmd("XACK")
            .arg(EXTRACTION_STREAM)
            .arg(&self.group)
            .arg(&job.stream_id)
            .ignore()
            .query_async::<()>(&mut conn)
            .await?;
        Ok(())
    }

    async fn dead_letter(&self, job: &ExtractionJob, reason: &str) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        ::redis::pipe()
            .atomic()
            .cmd("XADD")
            .arg(DLQ_STREAM)
            .arg("*")
            .arg("job_id")
            .arg(job.job_id.to_string())
            .arg("extraction_id")
            .arg(job.extraction_id.to_string())
            .arg("document_id")
            .arg(job.document_id.to_string())
            .arg("template_id")
            .arg(job.template_id.to_string())
            .arg("workspace_id")
            .arg(job.workspace_id.as_uuid().to_string())
            .arg("batch_job_id")
            .arg(
                job.batch_job_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            )
            .arg("attempt")
            .arg(job.attempt)
            .arg("max_attempts")
            .arg(job.max_attempts)
            .arg("error")
            .arg(reason)
            .ignore()
            .cmd("XACK")
            .arg(EXTRACTION_STREAM)
            .arg(&self.group)
            .arg(&job.stream_id)
            .ignore()
            .query_async::<()>(&mut conn)
            .await?;
        Ok(())
    }
}
