use crate::mail_ports::{
    MailDeliverySnapshot, MailEngineError, MailSubmissionAdapter, RecipientDeliveryState,
};
use crate::{
    ControlPlaneStore, MailDataPlaneFingerprintScheme, MailDeliveryQueryStore,
    MailDeliveryRecipientState, MailDeliveryRecipientWrite, MailDeliverySnapshotInsert,
    MailDeliverySnapshotWrite, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::MailSubmissionId;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReconcileMailDeliveryCommand {
    pub submission_id: MailSubmissionId,
    /// Scheduler time retained for command compatibility. It is deliberately
    /// not persisted as delivery observation evidence; PostgreSQL assigns that
    /// causal fact from its authoritative snapshot clock.
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailDeliveryReconciliationResult {
    Pending { inserted: bool },
    Terminal { inserted: bool },
}

#[derive(Debug, thiserror::Error)]
pub enum MailDeliveryReconciliationError {
    #[error("accepted mail submission is not available for delivery reconciliation")]
    NotEligible,
    #[error("mail delivery source is temporarily unavailable")]
    Unavailable,
}

pub struct MailDeliveryReconciliationService<S> {
    store: S,
    adapter: Arc<dyn MailSubmissionAdapter>,
    fingerprint_scheme: MailDataPlaneFingerprintScheme,
}

impl<S> MailDeliveryReconciliationService<S>
where
    S: ControlPlaneStore + MailDeliveryQueryStore,
{
    #[must_use]
    pub fn new(
        store: S,
        adapter: Arc<dyn MailSubmissionAdapter>,
        fingerprint_scheme: MailDataPlaneFingerprintScheme,
    ) -> Self {
        Self {
            store,
            adapter,
            fingerprint_scheme,
        }
    }

    pub async fn reconcile(
        &self,
        command: ReconcileMailDeliveryCommand,
    ) -> Result<MailDeliveryReconciliationResult, MailDeliveryReconciliationError> {
        let data_plane_id = self.adapter.data_plane_id();
        let target = self
            .store
            .get_mail_delivery_reconciliation_target(command.submission_id, data_plane_id)
            .await
            .map_err(map_store_error)?
            .filter(|target| target.data_plane_id == data_plane_id)
            .ok_or(MailDeliveryReconciliationError::NotEligible)?;
        let snapshot = self
            .adapter
            .get_delivery_snapshot(
                &target.engine_account_ref,
                &target.engine_email_ref,
                &target.engine_identity_ref,
                &target.engine_submission_ref,
            )
            .await
            .map_err(map_engine_error)?;
        if snapshot.submission_ref != target.engine_submission_ref {
            return Err(MailDeliveryReconciliationError::Unavailable);
        }
        let write = normalize_snapshot(&target, self.fingerprint_scheme, snapshot)?;
        let terminal = write.recipients.iter().all(|recipient| {
            matches!(
                recipient.state,
                MailDeliveryRecipientState::Delivered
                    | MailDeliveryRecipientState::PermanentFailure
            )
        });
        let counts = delivery_counts(&write.recipients);
        let mut transaction = self.store.begin().await.map_err(map_store_error)?;
        let insert = transaction
            .append_mail_delivery_snapshot(&write)
            .await
            .map_err(map_store_error)?;
        let inserted_at = match insert {
            MailDeliverySnapshotInsert::Inserted { observed_at } => Some(observed_at),
            MailDeliverySnapshotInsert::Duplicate => None,
        };
        if let Some(observed_at) = inserted_at {
            transaction
                .enqueue_event(&PendingEvent::for_mail_data_plane(
                    target.data_plane_id,
                    "message.delivery_status_changed.v1",
                    "mail.submission",
                    target.submission_id.to_string(),
                    json!({
                        "organization_id": target.organization_id,
                        "mailbox_id": target.mailbox_id,
                        "submission_id": target.submission_id,
                        "recipient_count": write.recipients.len(),
                        "delivered_count": counts.delivered,
                        "failed_count": counts.failed,
                        "queued_count": counts.queued,
                        "unknown_count": counts.unknown,
                        "terminal": terminal,
                    }),
                    observed_at,
                ))
                .await
                .map_err(map_store_error)?;
        }
        // audit-coverage: operational-source-evidence
        // This immutable snapshot is remote machine evidence. It is committed
        // with the matching Outbox event and is not attributed to a Human.
        transaction.commit().await.map_err(map_store_error)?;
        let inserted = inserted_at.is_some();
        Ok(if terminal {
            MailDeliveryReconciliationResult::Terminal { inserted }
        } else {
            MailDeliveryReconciliationResult::Pending { inserted }
        })
    }
}

fn normalize_snapshot(
    target: &crate::MailDeliveryReconciliationTarget,
    fingerprint_scheme: MailDataPlaneFingerprintScheme,
    snapshot: MailDeliverySnapshot,
) -> Result<MailDeliverySnapshotWrite, MailDeliveryReconciliationError> {
    if snapshot.source_state.is_empty()
        || snapshot.source_state.len() > 512
        || snapshot.source_state.trim() != snapshot.source_state
        || snapshot.source_state.chars().any(char::is_control)
        || snapshot.recipients.is_empty()
        || snapshot.recipients.len() > 100
    {
        return Err(MailDeliveryReconciliationError::Unavailable);
    }
    let recipients = snapshot
        .recipients
        .into_iter()
        .map(|recipient| {
            let mut digest = Sha256::new();
            match fingerprint_scheme {
                MailDataPlaneFingerprintScheme::LegacyV1 => {
                    digest.update(b"copilotmail.mail.delivery.recipient.v1");
                }
                MailDataPlaneFingerprintScheme::DataPlaneV2 => {
                    digest.update(b"copilotmail.mail.delivery.recipient.v2");
                    digest.update(target.data_plane_id.as_uuid().as_bytes());
                }
            }
            digest.update(target.submission_id.as_uuid().as_bytes());
            digest.update(recipient.recipient.as_str().as_bytes());
            MailDeliveryRecipientWrite {
                recipient_ref: digest.finalize().into(),
                state: match recipient.state {
                    RecipientDeliveryState::Queued => MailDeliveryRecipientState::Queued,
                    RecipientDeliveryState::Delivered => MailDeliveryRecipientState::Delivered,
                    RecipientDeliveryState::PermanentFailure => {
                        MailDeliveryRecipientState::PermanentFailure
                    }
                    RecipientDeliveryState::Unknown => MailDeliveryRecipientState::Unknown,
                },
                smtp_reply: recipient.smtp_reply,
            }
        })
        .collect::<Vec<_>>();
    let mut fingerprint = Sha256::new();
    match fingerprint_scheme {
        MailDataPlaneFingerprintScheme::LegacyV1 => {
            fingerprint.update(b"copilotmail.mail.delivery.snapshot.v1");
        }
        MailDataPlaneFingerprintScheme::DataPlaneV2 => {
            fingerprint.update(b"copilotmail.mail.delivery.snapshot.v2");
            fingerprint.update(target.data_plane_id.as_uuid().as_bytes());
        }
    }
    fingerprint.update(target.submission_id.as_uuid().as_bytes());
    fingerprint.update(target.engine_submission_ref.as_bytes());
    for recipient in &recipients {
        fingerprint.update(recipient.recipient_ref);
        fingerprint.update([delivery_state_tag(recipient.state)]);
        fingerprint.update((recipient.smtp_reply.len() as u32).to_be_bytes());
        fingerprint.update(recipient.smtp_reply.as_bytes());
    }
    Ok(MailDeliverySnapshotWrite {
        snapshot_id: Uuid::now_v7(),
        organization_id: target.organization_id,
        submission_id: target.submission_id,
        data_plane_id: target.data_plane_id,
        source_state: snapshot.source_state,
        source_sha256: fingerprint.finalize().into(),
        recipients,
    })
}

#[derive(Default)]
struct DeliveryCounts {
    delivered: usize,
    failed: usize,
    queued: usize,
    unknown: usize,
}

fn delivery_counts(recipients: &[MailDeliveryRecipientWrite]) -> DeliveryCounts {
    let mut counts = DeliveryCounts::default();
    for recipient in recipients {
        match recipient.state {
            MailDeliveryRecipientState::Delivered => counts.delivered += 1,
            MailDeliveryRecipientState::PermanentFailure => counts.failed += 1,
            MailDeliveryRecipientState::Queued => counts.queued += 1,
            MailDeliveryRecipientState::Unknown => counts.unknown += 1,
        }
    }
    counts
}

const fn delivery_state_tag(state: MailDeliveryRecipientState) -> u8 {
    match state {
        MailDeliveryRecipientState::Queued => 0,
        MailDeliveryRecipientState::Delivered => 1,
        MailDeliveryRecipientState::PermanentFailure => 2,
        MailDeliveryRecipientState::Unknown => 3,
    }
}

fn map_store_error(_error: StoreError) -> MailDeliveryReconciliationError {
    MailDeliveryReconciliationError::Unavailable
}

fn map_engine_error(_error: MailEngineError) -> MailDeliveryReconciliationError {
    MailDeliveryReconciliationError::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail_ports::{
        CreatedMailSubmission, MailSubmissionReconciliation, RecipientDeliveryEvidence,
    };
    use crate::{
        ControlPlaneTransaction, MailDeliveryReconciliationTarget, MailDeliverySnapshotRecord,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{
        AuditEvent, MailboxAddress, MailboxId, Organization, OrganizationId, OrganizationSlug,
        Principal, PrincipalId, Role, RoleAssignment,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct State {
        target: Option<MailDeliveryReconciliationTarget>,
        snapshot_observed_at: Option<DateTime<Utc>>,
        writes: Vec<MailDeliverySnapshotWrite>,
        events: Vec<PendingEvent>,
        commits: usize,
    }

    #[derive(Clone)]
    struct Store {
        state: Arc<Mutex<State>>,
    }

    struct Transaction {
        shared: Arc<Mutex<State>>,
        writes: Vec<MailDeliverySnapshotWrite>,
        events: Vec<PendingEvent>,
    }

    #[async_trait]
    impl ControlPlaneStore for Store {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                shared: Arc::clone(&self.state),
                writes: Vec::new(),
                events: Vec::new(),
            }))
        }
    }

    #[async_trait]
    impl MailDeliveryQueryStore for Store {
        async fn get_mail_delivery_reconciliation_target(
            &self,
            _: MailSubmissionId,
            _: copilotmail_domain::MailDataPlaneId,
        ) -> Result<Option<MailDeliveryReconciliationTarget>, StoreError> {
            Ok(self.state.lock().unwrap().target.clone())
        }

        async fn get_latest_mail_delivery_snapshot(
            &self,
            _: OrganizationId,
            _: PrincipalId,
            _: MailSubmissionId,
        ) -> Result<Option<MailDeliverySnapshotRecord>, StoreError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for Transaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_organization(&mut self, _: &Organization) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_principal(&mut self, _: &Principal) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role(&mut self, _: &Role) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role_assignment(&mut self, _: &RoleAssignment) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn record_organization_bootstrap(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn append_mail_delivery_snapshot(
            &mut self,
            write: &MailDeliverySnapshotWrite,
        ) -> Result<MailDeliverySnapshotInsert, StoreError> {
            let observed_at = self
                .shared
                .lock()
                .unwrap()
                .snapshot_observed_at
                .ok_or(StoreError::Unavailable)?;
            self.writes.push(write.clone());
            Ok(MailDeliverySnapshotInsert::Inserted { observed_at })
        }

        async fn append_audit_event(&mut self, _: &AuditEvent) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.events.push(event.clone());
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            let mut state = self.shared.lock().unwrap();
            state.writes.extend(self.writes);
            state.events.extend(self.events);
            state.commits += 1;
            Ok(())
        }
    }

    struct Adapter {
        data_plane_id: copilotmail_domain::MailDataPlaneId,
        snapshot: MailDeliverySnapshot,
        snapshot_calls: Arc<Mutex<usize>>,
    }

    impl crate::mail_ports::MailDataPlanePort for Adapter {
        fn data_plane_id(&self) -> copilotmail_domain::MailDataPlaneId {
            self.data_plane_id
        }
    }

    #[async_trait]
    impl MailSubmissionAdapter for Adapter {
        async fn probe_submissions(&self) -> Result<(), MailEngineError> {
            Ok(())
        }

        async fn reconcile_submission(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<MailSubmissionReconciliation, MailEngineError> {
            Err(MailEngineError::Unavailable)
        }

        async fn create_submission(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<CreatedMailSubmission, MailEngineError> {
            Err(MailEngineError::Unavailable)
        }

        async fn get_delivery_snapshot(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<MailDeliverySnapshot, MailEngineError> {
            *self.snapshot_calls.lock().unwrap() += 1;
            Ok(self.snapshot.clone())
        }
    }

    fn fixture(
        recipients: Vec<RecipientDeliveryEvidence>,
    ) -> (Store, MailSubmissionId, Arc<dyn MailSubmissionAdapter>) {
        let submission_id = MailSubmissionId::new();
        let snapshot_observed_at = Utc::now() + chrono::Duration::seconds(5);
        let target = MailDeliveryReconciliationTarget {
            submission_id,
            organization_id: OrganizationId::new(),
            requested_by: PrincipalId::new(),
            mailbox_id: MailboxId::new(),
            data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
            engine_account_ref: "account-42".into(),
            engine_email_ref: "email-9".into(),
            engine_identity_ref: "identity-3".into(),
            engine_submission_ref: "submission-7".into(),
        };
        (
            Store {
                state: Arc::new(Mutex::new(State {
                    target: Some(target),
                    snapshot_observed_at: Some(snapshot_observed_at),
                    ..State::default()
                })),
            },
            submission_id,
            Arc::new(Adapter {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                snapshot: MailDeliverySnapshot {
                    submission_ref: "submission-7".into(),
                    source_state: "state-8".into(),
                    recipients,
                },
                snapshot_calls: Arc::new(Mutex::new(0)),
            }),
        )
    }

    fn recipient(address: &str, state: RecipientDeliveryState) -> RecipientDeliveryEvidence {
        RecipientDeliveryEvidence {
            recipient: MailboxAddress::parse(address).unwrap(),
            state,
            smtp_reply: match state {
                RecipientDeliveryState::Delivered => "250 2.0.0 Accepted",
                RecipientDeliveryState::PermanentFailure => "550 5.1.1 Unknown",
                RecipientDeliveryState::Queued | RecipientDeliveryState::Unknown => {
                    "250 2.1.5 Queued"
                }
            }
            .into(),
        }
    }

    fn hex_lower(bytes: &[u8]) -> String {
        use std::fmt::Write as _;

        bytes.iter().fold(String::new(), |mut output, byte| {
            write!(&mut output, "{byte:02x}").unwrap();
            output
        })
    }

    #[test]
    fn legacy_delivery_fingerprints_match_historical_fixed_vectors() {
        let submission_id = MailSubmissionId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-0000000000aa").unwrap(),
        );
        let target = MailDeliveryReconciliationTarget {
            submission_id,
            organization_id: OrganizationId::new(),
            requested_by: PrincipalId::new(),
            mailbox_id: MailboxId::new(),
            data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
            engine_account_ref: "account-42".into(),
            engine_email_ref: "email-9".into(),
            engine_identity_ref: "identity-3".into(),
            engine_submission_ref: "submission-7".into(),
        };
        let snapshot = MailDeliverySnapshot {
            submission_ref: "submission-7".into(),
            source_state: "state-8".into(),
            recipients: vec![recipient(
                "person@example.test",
                RecipientDeliveryState::Delivered,
            )],
        };
        let write = normalize_snapshot(
            &target,
            MailDataPlaneFingerprintScheme::LegacyV1,
            snapshot.clone(),
        )
        .unwrap();
        assert_eq!(
            hex_lower(&write.recipients[0].recipient_ref),
            "eea8facc9a3bbc20658911f5e9b9882813aaafd393c08fc4acea0274149eeb66"
        );
        assert_eq!(
            hex_lower(&write.source_sha256),
            "b24eca879c917cfed1099523a8f61829e6463cfcb8b57ce13c742e1f62285739"
        );
        let replay =
            normalize_snapshot(&target, MailDataPlaneFingerprintScheme::LegacyV1, snapshot)
                .unwrap();
        assert_eq!(replay.source_sha256, write.source_sha256);
        assert_eq!(replay.recipients, write.recipients);
    }

    #[test]
    fn v2_delivery_fingerprints_are_scoped_to_the_data_plane() {
        let recipients = vec![recipient(
            "person@example.test",
            RecipientDeliveryState::Delivered,
        )];
        let (store, _, _) = fixture(recipients.clone());
        let first_target = store.state.lock().unwrap().target.clone().unwrap();
        let mut second_target = first_target.clone();
        second_target.data_plane_id = crate::mail_ports::other_test_mail_data_plane_id();
        let snapshot = MailDeliverySnapshot {
            submission_ref: "submission-7".into(),
            source_state: "state-8".into(),
            recipients,
        };
        let first = normalize_snapshot(
            &first_target,
            MailDataPlaneFingerprintScheme::DataPlaneV2,
            snapshot.clone(),
        )
        .unwrap();
        let second = normalize_snapshot(
            &second_target,
            MailDataPlaneFingerprintScheme::DataPlaneV2,
            snapshot,
        )
        .unwrap();
        assert_ne!(
            first.recipients[0].recipient_ref,
            second.recipients[0].recipient_ref
        );
        assert_ne!(first.source_sha256, second.source_sha256);
    }

    #[tokio::test]
    async fn mismatched_target_data_plane_never_reads_remote_delivery_state() {
        let submission_id = MailSubmissionId::new();
        let store = Store {
            state: Arc::new(Mutex::new(State {
                target: Some(MailDeliveryReconciliationTarget {
                    submission_id,
                    organization_id: OrganizationId::new(),
                    requested_by: PrincipalId::new(),
                    mailbox_id: MailboxId::new(),
                    data_plane_id: crate::mail_ports::other_test_mail_data_plane_id(),
                    engine_account_ref: "account-42".into(),
                    engine_email_ref: "email-9".into(),
                    engine_identity_ref: "identity-3".into(),
                    engine_submission_ref: "submission-7".into(),
                }),
                ..State::default()
            })),
        };
        let snapshot_calls = Arc::new(Mutex::new(0));
        let adapter = Arc::new(Adapter {
            data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
            snapshot: MailDeliverySnapshot {
                submission_ref: "submission-7".into(),
                source_state: "state-8".into(),
                recipients: vec![recipient(
                    "person@example.test",
                    RecipientDeliveryState::Delivered,
                )],
            },
            snapshot_calls: Arc::clone(&snapshot_calls),
        });

        let result = MailDeliveryReconciliationService::new(
            store,
            adapter,
            MailDataPlaneFingerprintScheme::LegacyV1,
        )
        .reconcile(ReconcileMailDeliveryCommand {
            submission_id,
            now: Utc::now(),
        })
        .await;

        assert!(matches!(
            result,
            Err(MailDeliveryReconciliationError::NotEligible)
        ));
        assert_eq!(*snapshot_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn pending_snapshot_is_minimized_and_committed_with_outbox_evidence() {
        let (store, submission_id, adapter) = fixture(vec![
            recipient("DONE@example.test", RecipientDeliveryState::Delivered),
            recipient("later@example.test", RecipientDeliveryState::Queued),
        ]);
        let result = MailDeliveryReconciliationService::new(
            store.clone(),
            adapter,
            MailDataPlaneFingerprintScheme::LegacyV1,
        )
        .reconcile(ReconcileMailDeliveryCommand {
            submission_id,
            now: Utc::now(),
        })
        .await
        .unwrap();
        assert_eq!(
            result,
            MailDeliveryReconciliationResult::Pending { inserted: true }
        );
        let state = store.state.lock().unwrap();
        assert_eq!(state.commits, 1);
        assert_eq!(state.writes.len(), 1);
        assert_eq!(state.events.len(), 1);
        assert_eq!(
            state.events[0].occurred_at,
            state.snapshot_observed_at.unwrap()
        );
        assert_eq!(
            state.events[0].event_type,
            "message.delivery_status_changed.v1"
        );
        assert_eq!(
            state.events[0].data_plane_id,
            Some(crate::mail_ports::test_mail_data_plane_id())
        );
        assert_eq!(state.events[0].payload["delivered_count"], 1);
        assert_eq!(state.events[0].payload["queued_count"], 1);
        assert!(state.events[0].payload.to_string().find('@').is_none());
        assert_eq!(state.writes[0].recipients[0].recipient_ref.len(), 32);
    }

    #[test]
    fn recipient_references_are_scoped_to_one_submission() {
        let recipients = vec![recipient(
            "person@example.test",
            RecipientDeliveryState::Delivered,
        )];
        let (store, _, _) = fixture(recipients.clone());
        let first_target = store.state.lock().unwrap().target.clone().unwrap();
        let snapshot = MailDeliverySnapshot {
            submission_ref: "submission-7".into(),
            source_state: "state-8".into(),
            recipients,
        };
        let first = normalize_snapshot(
            &first_target,
            MailDataPlaneFingerprintScheme::LegacyV1,
            snapshot.clone(),
        )
        .unwrap();
        let mut second_target = first_target;
        second_target.submission_id = MailSubmissionId::new();
        let second = normalize_snapshot(
            &second_target,
            MailDataPlaneFingerprintScheme::LegacyV1,
            snapshot,
        )
        .unwrap();
        assert_ne!(
            first.recipients[0].recipient_ref,
            second.recipients[0].recipient_ref
        );
    }
}
