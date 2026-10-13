use crate::*;

#[async_trait]
impl HumanLoginFailureStore for PgControlPlaneStore {
    async fn record_human_login_failure(
        &self,
        write: &HumanLoginFailureWrite,
        prune_limit: u32,
    ) -> Result<(), StoreError> {
        if !(1..=1_000).contains(&prune_limit) {
            return Err(StoreError::Conflict);
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        // `occurred_at` is stamped by the database clock via the migration
        // DDL default; callers never supply a timestamp.
        sqlx::query(
            r#"
            INSERT INTO human_login_failures(
                id, client_key, reason, source_sha256
            )
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(write.failure_id.as_uuid())
        .bind(human_session_client(write.client_key))
        .bind(write.reason.as_str())
        .bind(write.source_sha256.map(|digest| digest.to_vec()))
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        sqlx::query(
            r#"
            DELETE FROM human_login_failures
             WHERE id IN (
                 SELECT id FROM human_login_failures
                 WHERE occurred_at <= statement_timestamp() - interval '30 days'
                 ORDER BY occurred_at, id
                 LIMIT $1
             )
            "#,
        )
        .bind(i64::from(prune_limit))
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)
    }

    async fn list_human_login_failures(
        &self,
        query: &HumanLoginFailureQuery,
    ) -> Result<Vec<HumanLoginFailureRecord>, StoreError> {
        if !(1..=1_000).contains(&query.limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, HumanLoginFailureRow>(
            r#"
            SELECT id, client_key, reason, occurred_at, source_sha256
              FROM human_login_failures
             WHERE ($1::text IS NULL OR reason = $1)
               AND ($2::text IS NULL OR client_key = $2)
               AND ($3::timestamptz IS NULL OR occurred_at >= $3)
             ORDER BY occurred_at DESC, id DESC
             LIMIT $4
            "#,
        )
        .bind(query.reason.map(HumanLoginFailureReason::as_str))
        .bind(query.client_key.map(human_session_client))
        .bind(query.occurred_since)
        .bind(i64::from(query.limit))
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.iter().map(human_login_failure_from_row).collect()
    }
}

#[derive(sqlx::FromRow)]
struct HumanLoginFailureRow {
    id: Uuid,
    client_key: String,
    reason: String,
    occurred_at: DateTime<Utc>,
    source_sha256: Option<Vec<u8>>,
}

fn human_login_failure_from_row(
    row: &HumanLoginFailureRow,
) -> Result<HumanLoginFailureRecord, StoreError> {
    let source_sha256 = match &row.source_sha256 {
        None => None,
        Some(digest) => {
            Some(<[u8; 32]>::try_from(digest.as_slice()).map_err(|_| StoreError::Unavailable)?)
        }
    };
    Ok(HumanLoginFailureRecord {
        failure_id: HumanLoginFailureId::from_uuid(row.id),
        client_key: human_session_client_from_str(&row.client_key)?,
        reason: human_login_failure_reason_from_str(&row.reason)?,
        occurred_at: row.occurred_at,
        source_sha256,
    })
}

fn human_login_failure_reason_from_str(value: &str) -> Result<HumanLoginFailureReason, StoreError> {
    HumanLoginFailureReason::parse(value).ok_or_else(|| {
        tracing::error!(
            reason = value,
            "unknown Human login failure reason persisted"
        );
        StoreError::Unavailable
    })
}
