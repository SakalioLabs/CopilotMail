use chrono::{Duration, Utc};
use copilotmail_object_store::{
    ArchivedObjectEvidence, S3WormArchive, S3WormArchiveConfig, WormArchiveError, WormObjectSource,
    WormRetentionMode,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

fn test_archive_config() -> S3WormArchiveConfig {
    let endpoint =
        std::env::var("COPILOTMAIL_TEST_S3_ENDPOINT").expect("test S3 endpoint is required");
    let bucket = std::env::var("COPILOTMAIL_TEST_S3_BUCKET").expect("test S3 bucket is required");
    let region = std::env::var("COPILOTMAIL_TEST_S3_REGION").unwrap_or_else(|_| "us-east-1".into());
    S3WormArchiveConfig {
        endpoint_url: Some(endpoint),
        region,
        bucket,
        force_path_style: true,
        allow_insecure_http: true,
    }
}

async fn upload_test_version(
    artifact: &str,
) -> (S3WormArchive, ArchivedObjectEvidence, PathBuf, Vec<u8>) {
    let archive = S3WormArchive::connect(test_archive_config())
        .await
        .expect("connect to test archive");
    archive
        .verify_bucket_is_immutable()
        .await
        .expect("bucket must confirm versioning and Object Lock");

    let id = Uuid::now_v7();
    let path = std::env::temp_dir().join(format!("copilotmail-worm-{id}-{artifact}.json"));
    let bytes = format!("{{\"archive_id\":\"{id}\",\"artifact\":\"{artifact}\"}}\n").into_bytes();
    tokio::fs::write(&path, &bytes)
        .await
        .expect("write test source");
    let source = WormObjectSource {
        key: format!("integration/{id}/{artifact}.json"),
        content_type: "application/json".into(),
        path: path.clone(),
        sha256_hex: format!("{:x}", Sha256::digest(&bytes)),
        metadata: BTreeMap::from([("archive-id".into(), id.to_string())]),
    };
    let now = Utc::now();
    let evidence = archive
        .put_immutable(
            &source,
            WormRetentionMode::Governance,
            now + Duration::hours(1),
            now,
        )
        .await
        .expect("upload locked version");
    (archive, evidence, path, bytes)
}

fn distinct_sha256(sha256_hex: &str) -> String {
    let replacement = if sha256_hex.starts_with('0') {
        '1'
    } else {
        '0'
    };
    format!("{replacement}{}", &sha256_hex[1..])
}

#[tokio::test]
#[ignore = "requires COPILOTMAIL_TEST_S3_* and an Object Lock enabled bucket"]
async fn uploads_and_replays_exact_locked_version() {
    let archive = S3WormArchive::connect(test_archive_config())
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

#[tokio::test]
#[ignore = "requires COPILOTMAIL_TEST_S3_* and an Object Lock enabled bucket"]
async fn downloads_and_verifies_the_exact_archived_version() {
    let (archive, evidence, path, bytes) = upload_test_version("verified-download").await;

    let retrieved = archive
        .get_verified_version(
            &evidence.key,
            &evidence.version_id,
            &evidence.sha256_hex,
            evidence.size_bytes,
            evidence.size_bytes,
        )
        .await
        .expect("download and verify exact archived version");

    assert_eq!(retrieved.key, evidence.key);
    assert_eq!(retrieved.version_id, evidence.version_id);
    assert_eq!(retrieved.sha256_hex, evidence.sha256_hex);
    assert_eq!(retrieved.size_bytes, evidence.size_bytes);
    assert_eq!(retrieved.content_type, "application/json");
    assert_eq!(retrieved.bytes, bytes);
    tokio::fs::remove_file(path)
        .await
        .expect("remove local test source");
}

#[tokio::test]
#[ignore = "requires COPILOTMAIL_TEST_S3_* and an Object Lock enabled bucket"]
async fn rejects_download_when_expected_sha256_does_not_match() {
    let (archive, evidence, path, _) = upload_test_version("sha256-mismatch").await;
    let error = archive
        .get_verified_version(
            &evidence.key,
            &evidence.version_id,
            &distinct_sha256(&evidence.sha256_hex),
            evidence.size_bytes,
            evidence.size_bytes,
        )
        .await
        .expect_err("a mismatched SHA-256 must reject the downloaded object");

    assert!(matches!(error, WormArchiveError::Verification));
    tokio::fs::remove_file(path)
        .await
        .expect("remove local test source");
}

#[tokio::test]
#[ignore = "requires COPILOTMAIL_TEST_S3_* and an Object Lock enabled bucket"]
async fn rejects_download_when_expected_length_does_not_match() {
    let (archive, evidence, path, _) = upload_test_version("length-mismatch").await;
    let expected_size = evidence.size_bytes + 1;
    let error = archive
        .get_verified_version(
            &evidence.key,
            &evidence.version_id,
            &evidence.sha256_hex,
            expected_size,
            expected_size,
        )
        .await
        .expect_err("a mismatched length must reject the downloaded object");

    assert!(matches!(error, WormArchiveError::Verification));
    tokio::fs::remove_file(path)
        .await
        .expect("remove local test source");
}
