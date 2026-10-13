use crate::{
    HumanLoginFailureQuery, HumanLoginFailureReason, HumanLoginFailureRecord,
    HumanLoginFailureStore, HumanLoginFailureWrite, HumanSessionClient, StoreError,
};
use copilotmail_domain::HumanLoginFailureId;

/// Maximum expired ledger rows pruned by one append.
const LOGIN_FAILURE_PRUNE_LIMIT: u32 = 500;

/// Maximum rows returned by one platform-operations read.
const LOGIN_FAILURE_QUERY_LIMIT: u32 = 500;

#[derive(Debug, Clone)]
pub struct RecordHumanLoginFailureCommand {
    pub client_key: HumanSessionClient,
    pub reason: HumanLoginFailureReason,
    /// One-way, deployment-salted digest of the request source (remote
    /// address and/or user agent). Raw client metadata never crosses this
    /// boundary; `None` records an unattributed failure.
    pub source_sha256: Option<[u8; 32]>,
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum HumanLoginFailureError {
    #[error("the login failure ledger request is invalid")]
    InvalidRequest,
    #[error("the login failure ledger is temporarily unavailable")]
    Unavailable,
}

pub struct HumanLoginFailureService<S> {
    store: S,
}

impl<S> HumanLoginFailureService<S>
where
    S: HumanLoginFailureStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Appends one anonymous failure entry. Callers must treat this as
    /// best-effort evidence: the original rejection is always authoritative
    /// and a ledger outage must never turn a failed login into a different
    /// response.
    pub async fn record(
        &self,
        command: RecordHumanLoginFailureCommand,
    ) -> Result<(), HumanLoginFailureError> {
        self.store
            .record_human_login_failure(
                &HumanLoginFailureWrite {
                    failure_id: HumanLoginFailureId::new(),
                    client_key: command.client_key,
                    reason: command.reason,
                    source_sha256: command.source_sha256,
                },
                LOGIN_FAILURE_PRUNE_LIMIT,
            )
            .await
            .map_err(map_store_error)
    }

    /// Tenant-free read reserved for platform operations. The command is
    /// bounded; results carry only the sanitized ledger fields.
    pub async fn list(
        &self,
        query: HumanLoginFailureQuery,
    ) -> Result<Vec<HumanLoginFailureRecord>, HumanLoginFailureError> {
        if !(1..=LOGIN_FAILURE_QUERY_LIMIT).contains(&query.limit) {
            return Err(HumanLoginFailureError::InvalidRequest);
        }
        self.store
            .list_human_login_failures(&query)
            .await
            .map_err(map_store_error)
    }
}

const fn map_store_error(error: StoreError) -> HumanLoginFailureError {
    match error {
        StoreError::Conflict | StoreError::MissingReference | StoreError::AgentMailAdmission(_) => {
            HumanLoginFailureError::InvalidRequest
        }
        StoreError::AuthenticationInvalid | StoreError::Unavailable => {
            HumanLoginFailureError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::DateTime;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemoryStore {
        state: Arc<Mutex<MemoryState>>,
    }

    #[derive(Default)]
    struct MemoryState {
        rows: Vec<HumanLoginFailureRecord>,
        prune_calls: Vec<u32>,
        next_stamp: i64,
    }

    #[async_trait]
    impl HumanLoginFailureStore for MemoryStore {
        async fn record_human_login_failure(
            &self,
            write: &HumanLoginFailureWrite,
            cleanup_limit: u32,
        ) -> Result<(), StoreError> {
            if !(1..=1_000).contains(&cleanup_limit) {
                return Err(StoreError::Conflict);
            }
            let mut state = self.state.lock().unwrap();
            state.prune_calls.push(cleanup_limit);
            state.next_stamp += 1;
            let occurred_at = DateTime::from_timestamp_millis(state.next_stamp).unwrap();
            let horizon = occurred_at - chrono::Duration::days(30);
            state.rows.retain(|row| row.occurred_at > horizon);
            state.rows.push(HumanLoginFailureRecord {
                failure_id: write.failure_id,
                client_key: write.client_key,
                reason: write.reason,
                occurred_at,
                source_sha256: write.source_sha256,
            });
            Ok(())
        }

        async fn list_human_login_failures(
            &self,
            query: &HumanLoginFailureQuery,
        ) -> Result<Vec<HumanLoginFailureRecord>, StoreError> {
            let state = self.state.lock().unwrap();
            let mut rows = state
                .rows
                .iter()
                .filter(|row| {
                    query.reason.is_none_or(|reason| row.reason == reason)
                        && query
                            .client_key
                            .is_none_or(|client| row.client_key == client)
                        && query
                            .occurred_since
                            .is_none_or(|since| row.occurred_at >= since)
                })
                .cloned()
                .collect::<Vec<_>>();
            rows.sort_by(|left, right| {
                right
                    .occurred_at
                    .cmp(&left.occurred_at)
                    .then(right.failure_id.cmp(&left.failure_id))
            });
            rows.truncate(usize::try_from(query.limit).unwrap_or(usize::MAX));
            Ok(rows)
        }
    }

    fn record_command(
        client_key: HumanSessionClient,
        reason: HumanLoginFailureReason,
    ) -> RecordHumanLoginFailureCommand {
        RecordHumanLoginFailureCommand {
            client_key,
            reason,
            source_sha256: None,
        }
    }

    #[tokio::test]
    async fn ledger_records_only_reason_codes_database_time_and_salted_source() {
        let store = MemoryStore::default();
        let service = HumanLoginFailureService::new(store.clone());
        service
            .record(RecordHumanLoginFailureCommand {
                client_key: HumanSessionClient::Admin,
                reason: HumanLoginFailureReason::OidcCallbackRejected,
                source_sha256: Some([0x5a; 32]),
            })
            .await
            .unwrap();
        service
            .record(record_command(
                HumanSessionClient::Mail,
                HumanLoginFailureReason::PrincipalRejected,
            ))
            .await
            .unwrap();

        let rows = service
            .list(HumanLoginFailureQuery {
                reason: None,
                client_key: None,
                occurred_since: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].reason, HumanLoginFailureReason::PrincipalRejected);
        assert_eq!(rows[0].source_sha256, None);
        assert_eq!(
            rows[1].reason,
            HumanLoginFailureReason::OidcCallbackRejected
        );
        assert_eq!(rows[1].source_sha256, Some([0x5a; 32]));
        // The write carries no caller timestamp: occurred_at is stamped by
        // the store boundary, mirroring the database clock.
        assert!(rows[0].occurred_at > rows[1].occurred_at);
        assert_eq!(store.state.lock().unwrap().prune_calls, [500, 500]);
    }

    #[tokio::test]
    async fn ledger_queries_stay_bounded_and_filterable() {
        let store = MemoryStore::default();
        let service = HumanLoginFailureService::new(store.clone());
        for _ in 0..3 {
            service
                .record(record_command(
                    HumanSessionClient::Admin,
                    HumanLoginFailureReason::LoginTransactionRejected,
                ))
                .await
                .unwrap();
        }
        service
            .record(record_command(
                HumanSessionClient::Mail,
                HumanLoginFailureReason::CallbackStateMissing,
            ))
            .await
            .unwrap();

        assert_eq!(
            service
                .list(HumanLoginFailureQuery {
                    reason: None,
                    client_key: None,
                    occurred_since: None,
                    limit: 0,
                })
                .await,
            Err(HumanLoginFailureError::InvalidRequest)
        );
        assert_eq!(
            service
                .list(HumanLoginFailureQuery {
                    reason: None,
                    client_key: None,
                    occurred_since: None,
                    limit: LOGIN_FAILURE_QUERY_LIMIT + 1,
                })
                .await,
            Err(HumanLoginFailureError::InvalidRequest)
        );
        let page = service
            .list(HumanLoginFailureQuery {
                reason: Some(HumanLoginFailureReason::LoginTransactionRejected),
                client_key: Some(HumanSessionClient::Admin),
                occurred_since: None,
                limit: 2,
            })
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        assert!(
            page.iter()
                .all(|row| row.reason == HumanLoginFailureReason::LoginTransactionRejected)
        );
        let since = page[0].occurred_at;
        let recent = service
            .list(HumanLoginFailureQuery {
                reason: None,
                client_key: None,
                occurred_since: Some(since),
                limit: 10,
            })
            .await
            .unwrap();
        // `occurred_since` is inclusive: the newest Admin row and the later
        // Mail row qualify, the two older Admin rows do not.
        assert_eq!(recent.len(), 2);
    }

    #[tokio::test]
    async fn ledger_write_prunes_expired_rows_without_touching_live_ones() {
        let store = MemoryStore::default();
        {
            let mut state = store.state.lock().unwrap();
            state.next_stamp = 0;
            state.rows.push(HumanLoginFailureRecord {
                failure_id: HumanLoginFailureId::new(),
                client_key: HumanSessionClient::Admin,
                reason: HumanLoginFailureReason::SessionEstablishFailed,
                occurred_at: DateTime::from_timestamp_millis(0).unwrap()
                    - chrono::Duration::days(31),
                source_sha256: None,
            });
        }
        let service = HumanLoginFailureService::new(store.clone());
        service
            .record(record_command(
                HumanSessionClient::Admin,
                HumanLoginFailureReason::LoginAdmissionLimited,
            ))
            .await
            .unwrap();
        let rows = service
            .list(HumanLoginFailureQuery {
                reason: None,
                client_key: None,
                occurred_since: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].reason,
            HumanLoginFailureReason::LoginAdmissionLimited
        );
    }

    #[test]
    fn reason_codes_are_closed_stable_and_round_trip() {
        let reasons = [
            HumanLoginFailureReason::LoginAdmissionLimited,
            HumanLoginFailureReason::LoginBeginRejected,
            HumanLoginFailureReason::CallbackStateMissing,
            HumanLoginFailureReason::LoginCookieRejected,
            HumanLoginFailureReason::LoginTransactionRejected,
            HumanLoginFailureReason::CallbackClientMismatch,
            HumanLoginFailureReason::LoginMaterialRejected,
            HumanLoginFailureReason::OidcCallbackRejected,
            HumanLoginFailureReason::PrincipalRejected,
            HumanLoginFailureReason::AuthorizationUnavailable,
            HumanLoginFailureReason::SessionEstablishFailed,
        ];
        for reason in reasons {
            assert_eq!(
                HumanLoginFailureReason::parse(reason.as_str()),
                Some(reason)
            );
        }
        assert_eq!(HumanLoginFailureReason::parse("AccessDenied"), None);
        assert_eq!(
            HumanLoginFailureReason::parse("login_transaction_rejected; DROP TABLE"),
            None
        );
    }
}
