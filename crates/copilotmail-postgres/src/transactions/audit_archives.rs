use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_audit_checkpoint(
        &mut self,
        checkpoint: &SignedAuditCheckpoint,
        archive_object_key: &str,
    ) -> Result<bool, StoreError> {
        let payload = &checkpoint.payload;
        let signed_payload = String::from_utf8(payload.canonical_bytes().map_err(|error| {
            tracing::error!(%error, "invalid signed audit checkpoint payload");
            StoreError::Unavailable
        })?)
        .map_err(|_| StoreError::Unavailable)?;
        let root_hash = decode_sha256(&payload.root_hash).map_err(|error| {
            tracing::error!(%error, "invalid audit checkpoint root hash");
            StoreError::Unavailable
        })?;
        let signature = checkpoint.signature_bytes().map_err(|error| {
            tracing::error!(%error, "invalid audit checkpoint signature encoding");
            StoreError::Unavailable
        })?;
        let inserted = sqlx::query_scalar::<_, i32>(
            r#"
            INSERT INTO audit_checkpoints(
                id, organization_id, period_start, period_end, root_hash,
                signature, key_id, object_key, created_at,
                chain_sequence_start, chain_sequence_end, event_count,
                first_event_id, last_event_id, hash_version,
                signature_algorithm, signed_payload, public_key_fingerprint
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9,
                $10, $11, $12, $13, $14, $15, $16, $17, $18
            )
            ON CONFLICT DO NOTHING
            RETURNING 1
            "#,
        )
        .bind(payload.checkpoint_id.as_uuid())
        .bind(payload.organization_id.as_uuid())
        .bind(payload.period_start)
        .bind(payload.period_end)
        .bind(root_hash)
        .bind(signature)
        .bind(&payload.key_id)
        .bind(archive_object_key)
        .bind(payload.created_at)
        .bind(payload.chain_sequence_start)
        .bind(payload.chain_sequence_end)
        .bind(i64::try_from(payload.event_count).map_err(|_| StoreError::Conflict)?)
        .bind(payload.first_event_id)
        .bind(payload.last_event_id)
        .bind(payload.hash_version)
        .bind(&checkpoint.signature_algorithm)
        .bind(signed_payload)
        .bind(&payload.public_key_sha256)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(inserted.is_some())
    }

    pub(crate) async fn tx_find_audit_checkpoint(
        &mut self,
        organization_id: OrganizationId,
        checkpoint_id: AuditCheckpointId,
    ) -> Result<Option<ExistingAuditCheckpoint>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            signed_payload: String,
            signature_algorithm: String,
            signature: Vec<u8>,
            object_key: String,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT signed_payload, signature_algorithm, signature, object_key
              FROM audit_checkpoints
             WHERE organization_id = $1
               AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(checkpoint_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let checkpoint = SignedAuditCheckpoint::from_storage(
                &row.signed_payload,
                row.signature_algorithm,
                &row.signature,
            )
            .map_err(|error| {
                tracing::error!(%error, "stored audit checkpoint is invalid");
                StoreError::Unavailable
            })?;
            Ok(ExistingAuditCheckpoint {
                checkpoint,
                archive_object_key: row.object_key,
            })
        })
        .transpose()
    }

    pub(crate) async fn tx_insert_audit_archive(
        &mut self,
        archive: &AuditArchiveRecord,
    ) -> Result<bool, StoreError> {
        let events_size =
            i64::try_from(archive.events.size_bytes).map_err(|_| StoreError::Conflict)?;
        let manifest_size =
            i64::try_from(archive.manifest.size_bytes).map_err(|_| StoreError::Conflict)?;
        let inserted = sqlx::query_scalar::<_, i32>(
            r#"
            INSERT INTO audit_archives(
                id, organization_id, checkpoint_id, provider, bucket,
                events_object_key, events_version_id, events_sha256,
                events_size_bytes, manifest_object_key, manifest_version_id,
                manifest_sha256, manifest_size_bytes, retention_mode,
                retain_until, verified_at, created_by, created_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9,
                $10, $11, $12, $13, $14, $15, $16, $17, $18
            )
            ON CONFLICT DO NOTHING
            RETURNING 1
            "#,
        )
        .bind(archive.archive_id.as_uuid())
        .bind(archive.organization_id.as_uuid())
        .bind(archive.checkpoint_id.as_uuid())
        .bind(&archive.provider)
        .bind(&archive.bucket)
        .bind(&archive.events.key)
        .bind(&archive.events.version_id)
        .bind(&archive.events.sha256_hex)
        .bind(events_size)
        .bind(&archive.manifest.key)
        .bind(&archive.manifest.version_id)
        .bind(&archive.manifest.sha256_hex)
        .bind(manifest_size)
        .bind(&archive.retention_mode)
        .bind(archive.retain_until)
        .bind(archive.verified_at)
        .bind(archive.created_by.as_uuid())
        .bind(archive.created_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(inserted.is_some())
    }

    pub(crate) async fn tx_find_audit_archive(
        &mut self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            checkpoint_id: Uuid,
            provider: String,
            bucket: String,
            events_object_key: String,
            events_version_id: String,
            events_sha256: String,
            events_size_bytes: i64,
            manifest_object_key: String,
            manifest_version_id: String,
            manifest_sha256: String,
            manifest_size_bytes: i64,
            retention_mode: String,
            retain_until: DateTime<Utc>,
            verified_at: DateTime<Utc>,
            created_by: Uuid,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, checkpoint_id, provider, bucket,
                   events_object_key, events_version_id, events_sha256,
                   events_size_bytes, manifest_object_key, manifest_version_id,
                   manifest_sha256, manifest_size_bytes, retention_mode,
                   retain_until, verified_at, created_by, created_at
              FROM audit_archives
             WHERE organization_id = $1
               AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(archive_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(AuditArchiveRecord {
                archive_id: AuditArchiveId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                checkpoint_id: AuditCheckpointId::from_uuid(row.checkpoint_id),
                provider: row.provider,
                bucket: row.bucket,
                events: ArchivedObjectRecord {
                    key: row.events_object_key,
                    version_id: row.events_version_id,
                    sha256_hex: row.events_sha256,
                    size_bytes: u64::try_from(row.events_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                manifest: ArchivedObjectRecord {
                    key: row.manifest_object_key,
                    version_id: row.manifest_version_id,
                    sha256_hex: row.manifest_sha256,
                    size_bytes: u64::try_from(row.manifest_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                retention_mode: row.retention_mode,
                retain_until: row.retain_until,
                verified_at: row.verified_at,
                created_by: PrincipalId::from_uuid(row.created_by),
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    pub(crate) async fn tx_authorize_audit_export_grant(
        &mut self,
        grant: &AuditExportGrant,
    ) -> Result<AuditExportGrant, StoreError> {
        sqlx::query(
            r#"
            INSERT INTO audit_export_grants(
                id, organization_id, archive_id, approval_request_id,
                requested_by, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (approval_request_id) DO NOTHING
            "#,
        )
        .bind(grant.id().as_uuid())
        .bind(grant.organization_id().as_uuid())
        .bind(grant.archive_id().as_uuid())
        .bind(grant.approval_request_id().as_uuid())
        .bind(grant.requested_by().as_uuid())
        .bind(grant.created_at())
        .bind(grant.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let row = sqlx::query_as::<_, AuditExportGrantRow>(
            r#"
            SELECT id, organization_id, archive_id, approval_request_id,
                   requested_by, created_at, expires_at
              FROM audit_export_grants
             WHERE organization_id = $1 AND approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(grant.organization_id().as_uuid())
        .bind(grant.approval_request_id().as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;
        audit_export_grant_from_row(row)
    }

    pub(crate) async fn tx_record_audit_export_download(
        &mut self,
        download: &AuditExportDownloadRecord,
    ) -> Result<AuditExportDownloadRecord, StoreError> {
        let size_bytes =
            i64::try_from(download.object.size_bytes).map_err(|_| StoreError::Conflict)?;
        let downloaded_at = sqlx::query_scalar::<_, DateTime<Utc>>(
            r#"
            INSERT INTO audit_export_downloads(
                id, organization_id, grant_id, archive_id, downloaded_by,
                artifact, object_key, object_version_id, sha256, size_bytes,
                downloaded_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING downloaded_at
            "#,
        )
        .bind(download.id.as_uuid())
        .bind(download.organization_id.as_uuid())
        .bind(download.grant_id.as_uuid())
        .bind(download.archive_id.as_uuid())
        .bind(download.downloaded_by.as_uuid())
        .bind(download.artifact.as_str())
        .bind(&download.object.key)
        .bind(&download.object.version_id)
        .bind(&download.object.sha256_hex)
        .bind(size_bytes)
        .bind(download.downloaded_at)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(AuditExportDownloadRecord {
            downloaded_at,
            ..download.clone()
        })
    }
}
