use crate::*;

#[async_trait]
impl HumanLoginTransactionStore for PgControlPlaneStore {
    async fn admit_human_login_transaction(
        &self,
        write: &HumanLoginTransactionWrite,
        admission_limit: u32,
        cleanup_limit: u32,
    ) -> Result<HumanLoginAdmissionResult, StoreError> {
        if !(1..=100_000).contains(&admission_limit) || !(1..=1_000).contains(&cleanup_limit) {
            return Err(StoreError::Conflict);
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        let admitted_window = sqlx::query_scalar::<_, DateTime<Utc>>(
            r#"
            WITH database_window AS (
                SELECT date_trunc('minute', statement_timestamp()) AS window_started_at
            )
            INSERT INTO human_login_admission_windows(
                client_key, window_started_at, admitted_count
            )
            SELECT $1, database_window.window_started_at, 1
              FROM database_window
            ON CONFLICT (client_key, window_started_at) DO UPDATE
               SET admitted_count =
                   human_login_admission_windows.admitted_count + 1
             WHERE human_login_admission_windows.admitted_count < $2
            RETURNING window_started_at
            "#,
        )
        .bind(human_session_client(write.client_key))
        .bind(i64::from(admission_limit))
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if admitted_window.is_none() {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanLoginAdmissionResult::RateLimited {
                retry_after_seconds: 60,
            });
        }

        let cleanup_limit = i64::from(cleanup_limit);
        sqlx::query(
            r#"
            WITH stale AS (
                SELECT client_key, window_started_at
                  FROM human_login_admission_windows
                 WHERE window_started_at
                       < date_trunc('minute', statement_timestamp())
                 ORDER BY window_started_at, client_key
                 FOR UPDATE SKIP LOCKED
                 LIMIT $1
            )
            DELETE FROM human_login_admission_windows AS admission
             USING stale
             WHERE admission.client_key = stale.client_key
               AND admission.window_started_at = stale.window_started_at
            "#,
        )
        .bind(cleanup_limit)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;

        let expired_login_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT login.id
              FROM human_login_transactions AS login
             WHERE login.expires_at <= statement_timestamp()
               AND NOT EXISTS (
                   SELECT 1
                     FROM human_browser_sessions AS session
                    WHERE session.login_transaction_id = login.id
               )
             ORDER BY login.expires_at, login.id
             FOR UPDATE OF login SKIP LOCKED
             LIMIT $1
            "#,
        )
        .bind(cleanup_limit)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !expired_login_ids.is_empty() {
            sqlx::query(
                r#"
                DELETE FROM human_login_transaction_consumptions
                 WHERE login_transaction_id = ANY($1)
                "#,
            )
            .bind(expired_login_ids.as_slice())
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
            sqlx::query(
                r#"
                DELETE FROM human_login_transactions AS login
                 WHERE login.id = ANY($1)
                   AND login.expires_at <= statement_timestamp()
                   AND NOT EXISTS (
                       SELECT 1
                         FROM human_browser_sessions AS session
                        WHERE session.login_transaction_id = login.id
                   )
                "#,
            )
            .bind(expired_login_ids.as_slice())
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        }

        sqlx::query(
            r#"
            INSERT INTO human_login_transactions(
                id, client_key, return_to, state_sha256, browser_nonce_sha256,
                encrypted_pkce_verifier, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(write.login_transaction_id.as_uuid())
        .bind(human_session_client(write.client_key))
        .bind(&write.return_to)
        .bind(write.state_sha256.as_slice())
        .bind(write.browser_nonce_sha256.as_slice())
        .bind(&write.encrypted_pkce_verifier)
        .bind(write.created_at)
        .bind(write.expires_at)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanLoginAdmissionResult::Admitted)
    }

    async fn consume_human_login_transaction(
        &self,
        state_sha256: &[u8; 32],
        browser_nonce_sha256: &[u8; 32],
        consumed_at: DateTime<Utc>,
    ) -> Result<Option<HumanLoginTransactionRecord>, StoreError> {
        let row = sqlx::query_as::<_, HumanLoginTransactionRow>(
            r#"
            WITH candidate AS (
                SELECT login.id, login.client_key, login.return_to,
                       login.encrypted_pkce_verifier,
                       login.created_at, login.expires_at
                  FROM human_login_transactions AS login
                 WHERE login.state_sha256 = $1
                   AND login.browser_nonce_sha256 = $2
                   AND login.created_at <= $3
                   AND login.expires_at > $3
                   AND NOT EXISTS (
                       SELECT 1
                         FROM human_login_transaction_consumptions AS consumed
                        WHERE consumed.login_transaction_id = login.id
                   )
                   FOR UPDATE
            ),
            consumed AS (
                INSERT INTO human_login_transaction_consumptions(
                    login_transaction_id, consumed_at
                )
                SELECT id, $3 FROM candidate
                ON CONFLICT (login_transaction_id) DO NOTHING
                RETURNING login_transaction_id
            )
            SELECT candidate.id, candidate.client_key, candidate.return_to,
                   candidate.encrypted_pkce_verifier,
                   candidate.created_at, candidate.expires_at
              FROM candidate
              JOIN consumed
                ON consumed.login_transaction_id = candidate.id
            "#,
        )
        .bind(state_sha256.as_slice())
        .bind(browser_nonce_sha256.as_slice())
        .bind(consumed_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(human_login_transaction_from_row).transpose()
    }
}
