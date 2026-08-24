use std::time::Duration;
use struxio_core::jobs::{JobEnvelope, DELAYED_ZSET, DLQ_STREAM, READY_STREAM};
use struxio_core::queue::redis::RedisConsumer;
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
}
