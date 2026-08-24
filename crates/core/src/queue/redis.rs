use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use redis::FromRedisValue;
use struxio_common::WorkspaceId;
use uuid::Uuid;

use super::{
    ExtractionJob, MalformedMessage, QueueConsumer, QueueError, QueueProducer, StreamDelivery,
};
use crate::jobs::{JobEnvelope, CONSUMER_GROUP, DELAYED_ZSET, DLQ_STREAM, READY_STREAM};

const PROMOTE_DELAYED_LUA: &str = r#"
local payloads = redis.call(
    'ZRANGEBYSCORE', KEYS[1], '-inf', ARGV[1], 'LIMIT', 0, ARGV[2]
)
local promoted = 0

for _, payload in ipairs(payloads) do
    local ok, job = pcall(cjson.decode, payload)
    if ok
        and type(job) == 'table'
        and type(job.extraction_id) == 'string'
        and type(job.document_id) == 'string'
        and type(job.template_id) == 'string'
        and type(job.workspace_id) == 'string'
    then
        local fields = {
            'extraction_id', job.extraction_id,
            'document_id', job.document_id,
            'template_id', job.template_id,
            'workspace_id', job.workspace_id
        }
        if type(job.batch_job_id) == 'string' then
            table.insert(fields, 'batch_job_id')
            table.insert(fields, job.batch_job_id)
        end
        redis.call('XADD', KEYS[2], '*', unpack(fields))
    else
        redis.call(
            'XADD', KEYS[3], '*',
            'original_id', '0-0',
            'payload', payload,
            'error', 'invalid delayed job payload',
            'dead_lettered_at', ARGV[3]
        )
    end
    redis.call('ZREM', KEYS[1], payload)
    promoted = promoted + 1
end

return promoted
"#;

// ── Producer ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RedisProducer {
    client: ::redis::Client,
}

impl RedisProducer {
    pub fn new(client: ::redis::Client) -> Self {
        Self { client }
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
        let envelope = JobEnvelope {
            extraction_id,
            document_id,
            template_id,
            workspace_id: workspace_id.as_uuid(),
            batch_job_id,
        };
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        xadd_envelope(&mut conn, READY_STREAM, &envelope).await?;
        Ok(())
    }
}

// ── Consumer ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RedisConsumer {
    client: ::redis::Client,
    group: String,
    consumer: String,
}

impl RedisConsumer {
    pub fn new(client: ::redis::Client, group: String, consumer: String) -> Self {
        Self {
            client,
            group,
            consumer,
        }
    }

    pub fn for_workers(client: ::redis::Client, consumer: String) -> Self {
        Self::new(client, CONSUMER_GROUP.to_string(), consumer)
    }

    /// Atomically move due delayed jobs onto the ready stream.
    pub async fn promote_delayed(&self, limit: usize) -> Result<usize, QueueError> {
        if limit == 0 {
            return Ok(0);
        }
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let now_ms = Utc::now().timestamp_millis();
        let promoted: usize = ::redis::Script::new(PROMOTE_DELAYED_LUA)
            .key(DELAYED_ZSET)
            .key(READY_STREAM)
            .key(DLQ_STREAM)
            .arg(now_ms)
            .arg(limit)
            .arg(Utc::now().to_rfc3339())
            .invoke_async(&mut conn)
            .await?;
        Ok(promoted)
    }

    /// Read newly assigned jobs (`>`), blocking up to `block`.
    pub async fn read_new(
        &self,
        count: usize,
        block: Duration,
    ) -> Result<Vec<StreamDelivery>, QueueError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let result: Option<::redis::streams::StreamReadReply> = ::redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(&self.group)
            .arg(&self.consumer)
            .arg("COUNT")
            .arg(count)
            .arg("BLOCK")
            .arg(block.as_millis() as usize)
            .arg("STREAMS")
            .arg(READY_STREAM)
            .arg(">")
            .query_async(&mut conn)
            .await?;

        Ok(result
            .map(|reply| {
                reply
                    .keys
                    .into_iter()
                    .flat_map(|key| key.ids.into_iter().map(parse_stream_id))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Reclaim entries idle in the PEL longer than `min_idle`.
    pub async fn reclaim_stale(
        &self,
        min_idle: Duration,
        count: usize,
    ) -> Result<Vec<StreamDelivery>, QueueError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let reply: ::redis::streams::StreamAutoClaimReply = ::redis::cmd("XAUTOCLAIM")
            .arg(READY_STREAM)
            .arg(&self.group)
            .arg(&self.consumer)
            .arg(min_idle.as_millis() as usize)
            .arg("0-0")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut conn)
            .await?;
        Ok(reply.claimed.into_iter().map(parse_stream_id).collect())
    }

    pub async fn schedule_retry(
        &self,
        envelope: &JobEnvelope,
        delay: Duration,
    ) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let payload = envelope
            .to_delayed_payload()
            .map_err(|e| QueueError::Other(e.to_string()))?;
        let score = Utc::now().timestamp_millis() + delay.as_millis() as i64;
        let _: i32 = ::redis::cmd("ZADD")
            .arg(DELAYED_ZSET)
            .arg(score)
            .arg(payload)
            .query_async(&mut conn)
            .await?;
        Ok(())
    }

    pub async fn dead_letter_job(
        &self,
        job: &ExtractionJob,
        reason: &str,
    ) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        xadd_dlq_raw(
            &mut conn,
            &job.stream_id,
            &job.envelope().to_fields(),
            reason,
        )
        .await
    }

    pub async fn dead_letter_malformed(
        &self,
        message: &MalformedMessage,
    ) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        xadd_dlq_raw(
            &mut conn,
            &message.stream_id,
            &message.fields,
            &message.reason,
        )
        .await
    }
}

#[async_trait]
impl QueueConsumer for RedisConsumer {
    async fn ensure_group(&self) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        if let Err(e) = ::redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(READY_STREAM)
            .arg(&self.group)
            .arg("0")
            .arg("MKSTREAM")
            .query_async::<()>(&mut conn)
            .await
        {
            if !e.to_string().contains("BUSYGROUP") {
                return Err(e.into());
            }
        }
        Ok(())
    }

    async fn next_job(&self) -> Result<Option<ExtractionJob>, QueueError> {
        let deliveries = self.read_new(1, Duration::from_secs(5)).await?;
        match deliveries.into_iter().next() {
            Some(StreamDelivery::Job(job)) => Ok(Some(job)),
            Some(StreamDelivery::Malformed(message)) => {
                self.dead_letter_malformed(&message).await?;
                self.ack(&message.stream_id).await?;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    async fn ack(&self, stream_id: &str) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        ::redis::cmd("XACK")
            .arg(READY_STREAM)
            .arg(&self.group)
            .arg(stream_id)
            .query_async::<()>(&mut conn)
            .await?;
        Ok(())
    }
}

fn parse_stream_id(entry: ::redis::streams::StreamId) -> StreamDelivery {
    let mut fields = HashMap::new();
    let mut pairs = Vec::new();
    for (key, value) in &entry.map {
        if let Ok(text) = String::from_redis_value(value) {
            fields.insert(key.clone(), text.clone());
            pairs.push((key.clone(), text));
        }
    }
    match JobEnvelope::from_fields(&fields) {
        Ok(envelope) => match envelope.workspace() {
            Ok(workspace_id) => StreamDelivery::Job(ExtractionJob {
                stream_id: entry.id,
                extraction_id: envelope.extraction_id,
                document_id: envelope.document_id,
                template_id: envelope.template_id,
                workspace_id,
                batch_job_id: envelope.batch_job_id,
            }),
            Err(err) => StreamDelivery::Malformed(MalformedMessage {
                stream_id: entry.id,
                reason: err.message().to_string(),
                fields: pairs,
            }),
        },
        Err(err) => StreamDelivery::Malformed(MalformedMessage {
            stream_id: entry.id,
            reason: err.message().to_string(),
            fields: pairs,
        }),
    }
}

async fn xadd_envelope(
    conn: &mut redis::aio::MultiplexedConnection,
    stream: &str,
    envelope: &JobEnvelope,
) -> Result<(), QueueError> {
    let mut cmd = ::redis::cmd("XADD");
    cmd.arg(stream).arg("*");
    for (key, value) in envelope.to_fields() {
        cmd.arg(key).arg(value);
    }
    cmd.query_async::<String>(conn).await?;
    Ok(())
}

async fn xadd_dlq_raw(
    conn: &mut redis::aio::MultiplexedConnection,
    original_id: &str,
    fields: &[(String, String)],
    reason: &str,
) -> Result<(), QueueError> {
    let mut cmd = ::redis::cmd("XADD");
    cmd.arg(DLQ_STREAM)
        .arg("*")
        .arg("original_id")
        .arg(original_id)
        .arg("error")
        .arg(reason)
        .arg("dead_lettered_at")
        .arg(Utc::now().to_rfc3339());
    for (key, value) in fields {
        cmd.arg(key).arg(value);
    }
    cmd.query_async::<String>(conn).await?;
    Ok(())
}
