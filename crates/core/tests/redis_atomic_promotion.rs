use std::time::Duration;
use struxio_common::WorkspaceId;
use struxio_core::jobs::{JobEnvelope, DELAYED_ZSET, DLQ_STREAM, READY_STREAM};
use struxio_core::queue::redis::{RedisConsumer, RedisProducer};
use struxio_core::queue::{QueueConsumer, QueueProducer};
use uuid::Uuid;

#[tokio::test]
async fn concurrent_promoters_emit_one_ready_entry() {
    let Ok(redis_url) = std::env::var("REDIS_URL") else {
        return;
    };
    let client = redis::Client::open(redis_url).expect("redis client");
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("redis connection");
    redis::cmd("DEL")
        .arg(DELAYED_ZSET)
        .arg(READY_STREAM)
        .arg(DLQ_STREAM)
        .query_async::<()>(&mut connection)
        .await
        .expect("clear queue keys");

    let consumer = RedisConsumer::for_workers(client, format!("atomic-test-{}", Uuid::new_v4()));
    let envelope = JobEnvelope {
        extraction_id: Uuid::new_v4(),
        document_id: Uuid::new_v4(),
        template_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        batch_job_id: None,
    };
    consumer
        .schedule_retry(&envelope, Duration::ZERO)
        .await
        .expect("schedule retry");

    let first = consumer.promote_delayed(10);
    let second = consumer.promote_delayed(10);
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.unwrap() + second.unwrap(), 1);

    let ready: usize = redis::cmd("XLEN")
        .arg(READY_STREAM)
        .query_async(&mut connection)
        .await
        .expect("ready length");
    let delayed: usize = redis::cmd("ZCARD")
        .arg(DELAYED_ZSET)
        .query_async(&mut connection)
        .await
        .expect("delayed length");
    assert_eq!(ready, 1);
    assert_eq!(delayed, 0);

    redis::cmd("DEL")
        .arg(DELAYED_ZSET)
        .arg(READY_STREAM)
        .arg(DLQ_STREAM)
        .query_async::<()>(&mut connection)
        .await
        .expect("reset queue keys");
    let owner = RedisConsumer::for_workers(
        client.clone(),
        format!("heartbeat-owner-{}", Uuid::new_v4()),
    );
    owner.ensure_group().await.expect("consumer group");
    RedisProducer::new(client.clone())
        .enqueue_extraction(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            WorkspaceId::new(Uuid::new_v4()).unwrap(),
            None,
        )
        .await
        .expect("ready job");
    let delivery = owner
        .read_new(1, Duration::from_millis(10))
        .await
        .expect("read job");
    let stream_id = match &delivery[0] {
        struxio_core::queue::StreamDelivery::Job(job) => job.stream_id.clone(),
        other => panic!("expected job, got {other:?}"),
    };
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(owner.touch(&stream_id).await.expect("touch delivery"));

    let competitor =
        RedisConsumer::for_workers(client, format!("heartbeat-competitor-{}", Uuid::new_v4()));
    let reclaimed = competitor
        .reclaim_stale(Duration::from_millis(200), 1)
        .await
        .expect("reclaim check");
    assert!(
        reclaimed.is_empty(),
        "heartbeat must keep a live delivery from being reclaimed"
    );
}
