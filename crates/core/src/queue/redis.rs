use async_trait::async_trait;
use struxio_common::WorkspaceId;
use uuid::Uuid;

use super::{ExtractionJob, QueueConsumer, QueueError, QueueProducer};

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
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let eid = extraction_id.to_string();
        let did = document_id.to_string();
        let tid = template_id.to_string();
        let wid = workspace_id.as_uuid().to_string();

        match batch_job_id {
            Some(bid) => {
                let bid_s = bid.to_string();
                ::redis::cmd("XADD")
                    .arg("extractions:queue")
                    .arg("*")
                    .arg("extraction_id")
                    .arg(&eid)
                    .arg("document_id")
                    .arg(&did)
                    .arg("template_id")
                    .arg(&tid)
                    .arg("workspace_id")
                    .arg(&wid)
                    .arg("batch_job_id")
                    .arg(&bid_s)
                    .query_async::<String>(&mut conn)
                    .await?;
            }
            None => {
                ::redis::cmd("XADD")
                    .arg("extractions:queue")
                    .arg("*")
                    .arg("extraction_id")
                    .arg(&eid)
                    .arg("document_id")
                    .arg(&did)
                    .arg("template_id")
                    .arg(&tid)
                    .arg("workspace_id")
                    .arg(&wid)
                    .query_async::<String>(&mut conn)
                    .await?;
            }
        }

        Ok(())
    }
}

// ── Consumer ─────────────────────────────────────────────────────────────────

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
}

#[async_trait]
impl QueueConsumer for RedisConsumer {
    async fn ensure_group(&self) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        if let Err(e) = ::redis::cmd("XGROUP")
            .arg("CREATE")
            .arg("extractions:queue")
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
        let result: Option<::redis::streams::StreamReadReply> = ::redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(&self.group)
            .arg(&self.consumer)
            .arg("COUNT")
            .arg(1)
            .arg("BLOCK")
            .arg(5000)
            .arg("STREAMS")
            .arg("extractions:queue")
            .arg(">")
            .query_async(&mut conn)
            .await?;

        let entry = match result {
            Some(reply) => reply
                .keys
                .first()
                .and_then(|k| k.ids.first())
                .cloned(),
            None => return Ok(None),
        };

        let entry = match entry {
            Some(e) => e,
            None => return Ok(None),
        };

        let stream_id = entry.id.clone();
        let extraction_id: Option<Uuid> = entry
            .get::<String>("extraction_id")
            .and_then(|s| Uuid::parse_str(&s).ok());
        let document_id: Option<Uuid> = entry
            .get::<String>("document_id")
            .and_then(|s| Uuid::parse_str(&s).ok());
        let template_id: Option<Uuid> = entry
            .get::<String>("template_id")
            .and_then(|s| Uuid::parse_str(&s).ok());
        let workspace_id: Option<WorkspaceId> = entry
            .get::<String>("workspace_id")
            .and_then(|s| Uuid::parse_str(&s).ok())
            .and_then(|id| WorkspaceId::new(id).ok());
        let batch_job_id: Option<Uuid> = entry
            .get::<String>("batch_job_id")
            .and_then(|s| Uuid::parse_str(&s).ok());

        let job = extraction_id.and_then(|eid| {
            document_id.and_then(|did| {
                template_id.and_then(|tid| {
                    workspace_id.map(|wid| ExtractionJob {
                        stream_id,
                        extraction_id: eid,
                        document_id: did,
                        template_id: tid,
                        workspace_id: wid,
                        batch_job_id,
                    })
                })
            })
        });

        Ok(job)
    }

    async fn ack(&self, stream_id: &str) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        ::redis::cmd("XACK")
            .arg("extractions:queue")
            .arg(&self.group)
            .arg(stream_id)
            .query_async::<()>(&mut conn)
            .await?;
        Ok(())
    }
}
