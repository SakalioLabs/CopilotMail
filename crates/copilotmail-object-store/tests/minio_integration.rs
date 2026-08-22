use chrono::{Duration, Utc};
use copilotmail_object_store::{
    S3WormArchive, S3WormArchiveConfig, WormObjectSource, WormRetentionMode,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires COPILOTMAIL_TEST_S3_* and an Object Lock enabled bucket"]
async fn uploads_and_replays_exact_locked_version() {
    let endpoint =
        std::env::var("COPILOTMAIL_TEST_S3_ENDPOINT").expect("test S3 endpoint is required");
    let bucket = std::env::var("COPILOTMAIL_TEST_S3_BUCKET").expect("test S3 bucket is required");
    let region = std::env::var("COPILOTMAIL_TEST_S3_REGION").unwrap_or_else(|_| "us-east-1".into());
    let archive = S3WormArchive::connect(S3WormArchiveConfig {
        endpoint_url: Some(endpoint),
        region,
        bucket,
        force_path_style: true,
        allow_insecure_http: true,
    })
    .await
    .expect("connect to test archive");
    archive
        .verify_bucket_is_immutable()
        .await
        .expect("bucket must confirm versioning and Object Lock");

    let id = Uuid::now_v7();
    let path = std::env::temp_dir().join(format!("copilotmail-worm-{id}.json"));
    let body = format!("{{\"archive_id\":\"{id}\"}}\n");
    tokio::fs::write(&path, body.as_bytes())
        .await
        .expect("write test source");
    let source = WormObjectSource {
        key: format!("integration/{id}/evidence.json"),
        content_type: "application/json".into(),
        path: path.clone(),
        sha256_hex: format!("{:x}", Sha256::digest(body.as_bytes())),
        metadata: BTreeMap::from([("archive-id".into(), id.to_string())]),
    };
    let now = Utc::now();
    let retain_until = now + Duration::hours(1);
    let first = archive
        .put_immutable(&source, WormRetentionMode::Governance, retain_until, now)
        .await
        .expect("upload locked version");
    let replay = archive
        .put_immutable(&source, WormRetentionMode::Governance, retain_until, now)
        .await
        .expect("reuse exact locked version");
    assert_eq!(first, replay);
    assert!(first.retain_until >= retain_until);
    let held = archive
        .set_legal_hold(&first.key, &first.version_id, true)
        .await
        .expect("apply and verify exact-version legal hold");
    let held_replay = archive
        .set_legal_hold(&first.key, &first.version_id, true)
        .await
        .expect("idempotently verify existing legal hold");
    assert!(held.enabled);
    assert_eq!(held.key, held_replay.key);
    assert_eq!(held.version_id, held_replay.version_id);
    let released = archive
        .set_legal_hold(&first.key, &first.version_id, false)
        .await
        .expect("release and verify exact-version legal hold");
    assert!(!released.enabled);
    tokio::fs::remove_file(path)
        .await
        .expect("remove local test source");
}
