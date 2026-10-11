use crate::ports::HumanSentDeliveryEvidence as StoredDeliverySummary;
use crate::{
    ControlPlaneStore, HumanSentListItemRecheck, HumanSentListQuery, HumanSentListRecheck,
    HumanSentQueryStore, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, MailSubmissionId,
    MailboxCapability, MailboxId, Permission,
};
use serde_json::json;
use uuid::Uuid;

const MIN_LIMIT: u16 = 1;
const MAX_LIMIT: u16 = 50;
const CURSOR_PREFIX: &str = "v1.";

#[derive(Debug, Clone)]
pub struct ListHumanSentSubmissionsCommand {
    pub context: AuthorizationContext,
    pub mailbox_id: MailboxId,
    pub limit: u16,
    pub cursor: Option<String>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Browser-safe, latest delivery counters for one accepted submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSentDeliverySummary {
    pub observed_at: DateTime<Utc>,
    pub terminal: bool,
    pub recipient_count: u16,
    pub delivered_count: u16,
    pub failed_count: u16,
    pub queued_count: u16,
    pub unknown_count: u16,
}

/// Content-free metadata for one accepted submission owned by the current
/// Human. It intentionally excludes Draft, provider, recipient, and SMTP
/// information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSentListItem {
    pub submission_id: MailSubmissionId,
    pub accepted_at: DateTime<Utc>,
    pub recipient_count: u16,
    pub delivery: Option<HumanSentDeliverySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSentPage {
    pub items: Vec<HumanSentListItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum HumanSentError {
    #[error("Human sent-directory access is forbidden")]
    Forbidden,
    #[error("the mailbox is unavailable or inaccessible")]
    NotFound,
    #[error("Human sent-directory page size must be between 1 and 50")]
    InvalidPageSize,
    #[error("the Human sent-directory cursor is invalid or no longer visible")]
    InvalidCursor,
    #[error("the sent-directory state or authorization changed")]
    Conflict,
    #[error("Human sent-directory state is temporarily unavailable")]
    Unavailable,
}

pub struct HumanSentService<S> {
    store: S,
}

impl<S> HumanSentService<S>
where
    S: HumanSentQueryStore + ControlPlaneStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Lists only durable, accepted Human submissions. The list is a local
    /// evidence directory; it deliberately performs no data-plane or remote
    /// mailbox I/O and remains visible after an otherwise valid data-plane
    /// rebinding.
    pub async fn list(
        &self,
        command: ListHumanSentSubmissionsCommand,
    ) -> Result<HumanSentPage, HumanSentError> {
        require_mailbox_use(&command.context)?;
        if !(MIN_LIMIT..=MAX_LIMIT).contains(&command.limit) {
            return Err(HumanSentError::InvalidPageSize);
        }
        let after_submission_id = command.cursor.as_deref().map(parse_cursor).transpose()?;
        let batch = self
            .store
            .list_human_sent_submissions(&HumanSentListQuery {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                mailbox_id: command.mailbox_id,
                after_submission_id,
                limit: command.limit,
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;

        if !batch.active_human {
            return Err(HumanSentError::Forbidden);
        }
        if after_submission_id.is_some() && !batch.cursor_valid {
            return Err(HumanSentError::InvalidCursor);
        }
        if batch.candidates.len() > usize::from(command.limit)
            || (batch.has_more && batch.candidates.is_empty())
        {
            return Err(HumanSentError::Unavailable);
        }
        let authorization = batch.authorization.ok_or(HumanSentError::NotFound)?;
        if !can_read_and_send(
            &authorization,
            command.context.organization_id,
            command.mailbox_id,
        ) {
            return Err(HumanSentError::NotFound);
        }

        let mut previous: Option<(DateTime<Utc>, MailSubmissionId)> = None;
        let mut items = Vec::with_capacity(batch.candidates.len());
        let mut expected_submissions = Vec::with_capacity(batch.candidates.len());
        for candidate in batch.candidates {
            if candidate.organization_id != command.context.organization_id
                || candidate.mailbox_id != command.mailbox_id
                || candidate.requested_by != command.context.principal_id
                || candidate.submission_id.as_uuid().is_nil()
                || !(1..=100).contains(&candidate.recipient_count)
                || previous.is_some_and(|(accepted_at, submission_id)| {
                    candidate.accepted_at > accepted_at
                        || (candidate.accepted_at == accepted_at
                            && candidate.submission_id >= submission_id)
                })
            {
                return Err(HumanSentError::Unavailable);
            }
            let delivery = candidate
                .delivery
                .as_ref()
                .map(|summary| {
                    validate_delivery_summary(
                        summary,
                        candidate.accepted_at,
                        candidate.recipient_count,
                    )
                })
                .transpose()?;
            previous = Some((candidate.accepted_at, candidate.submission_id));
            expected_submissions.push(HumanSentListItemRecheck {
                submission_id: candidate.submission_id,
                expected_accepted_at: candidate.accepted_at,
                expected_recipient_count: candidate.recipient_count,
                expected_delivery_snapshot_id: candidate
                    .delivery
                    .as_ref()
                    .map(|summary| summary.snapshot_id),
            });
            items.push(HumanSentListItem {
                submission_id: candidate.submission_id,
                accepted_at: candidate.accepted_at,
                recipient_count: candidate.recipient_count,
                delivery,
            });
        }
        let next_cursor = if batch.has_more {
            items.last().map(|item| format_cursor(item.submission_id))
        } else {
            None
        };
        let page = HumanSentPage { items, next_cursor };
        self.record_list_evidence(
            &command,
            &HumanSentListRecheck {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                mailbox_id: command.mailbox_id,
                expected_submissions,
            },
            &page,
        )
        .await?;
        Ok(page)
    }

    async fn record_list_evidence(
        &self,
        command: &ListHumanSentSubmissionsCommand,
        proof: &HumanSentListRecheck,
        page: &HumanSentPage,
    ) -> Result<(), HumanSentError> {
        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(|_| HumanSentError::Unavailable)?;
        if !transaction
            .human_sent_list_still_authorized(proof)
            .await
            .map_err(|_| HumanSentError::Unavailable)?
        {
            return Err(HumanSentError::Conflict);
        }
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail.submissions.queried",
            AuditResource {
                kind: "mailbox".into(),
                id: command.mailbox_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "page_size": command.limit,
                "result_count": page.items.len(),
                "has_more": page.next_cursor.is_some(),
            }),
            command.now,
        )
        .and_then(|event| event.correlate_human_session(command.context.authentication.as_ref()))
        .map_err(|_| HumanSentError::Unavailable)?;
        let event = PendingEvent::new(
            "mail.submissions.queried.v1",
            "mailbox",
            command.mailbox_id.to_string(),
            json!({
                "organization_id": command.context.organization_id,
                "mailbox_id": command.mailbox_id,
                "result_count": page.items.len(),
                "has_more": page.next_cursor.is_some(),
            }),
            command.now,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(|_| HumanSentError::Unavailable)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(|_| HumanSentError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| HumanSentError::Unavailable)
    }
}

fn require_mailbox_use(context: &AuthorizationContext) -> Result<(), HumanSentError> {
    if context.permits(context.organization_id, Permission::MailboxUse) {
        Ok(())
    } else {
        Err(HumanSentError::Forbidden)
    }
}

fn can_read_and_send(
    authorization: &copilotmail_domain::MailboxAccessSnapshot,
    organization_id: copilotmail_domain::OrganizationId,
    mailbox_id: MailboxId,
) -> bool {
    authorization
        .authorize(organization_id, mailbox_id, MailboxCapability::Read, None)
        .is_some()
        && authorization
            .authorize(organization_id, mailbox_id, MailboxCapability::Send, None)
            .is_some()
}

fn validate_delivery_summary(
    summary: &StoredDeliverySummary,
    accepted_at: DateTime<Utc>,
    expected_recipient_count: u16,
) -> Result<HumanSentDeliverySummary, HumanSentError> {
    let counts = [
        summary.delivered_count,
        summary.failed_count,
        summary.queued_count,
        summary.unknown_count,
    ];
    if summary.snapshot_id.is_nil()
        || summary.observed_at < accepted_at
        || !(1..=100).contains(&summary.recipient_count)
        || summary.recipient_count != expected_recipient_count
        || counts.iter().any(|count| *count > summary.recipient_count)
        || counts.iter().map(|count| u32::from(*count)).sum::<u32>()
            != u32::from(summary.recipient_count)
        || summary.terminal != (summary.queued_count == 0 && summary.unknown_count == 0)
    {
        return Err(HumanSentError::Unavailable);
    }
    Ok(HumanSentDeliverySummary {
        observed_at: summary.observed_at,
        terminal: summary.terminal,
        recipient_count: summary.recipient_count,
        delivered_count: summary.delivered_count,
        failed_count: summary.failed_count,
        queued_count: summary.queued_count,
        unknown_count: summary.unknown_count,
    })
}

fn parse_cursor(value: &str) -> Result<MailSubmissionId, HumanSentError> {
    let raw = value
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(HumanSentError::InvalidCursor)?;
    let parsed = Uuid::parse_str(raw).map_err(|_| HumanSentError::InvalidCursor)?;
    if parsed.is_nil() || raw != parsed.to_string() {
        return Err(HumanSentError::InvalidCursor);
    }
    Ok(MailSubmissionId::from_uuid(parsed))
}

fn format_cursor(submission_id: MailSubmissionId) -> String {
    format!("{CURSOR_PREFIX}{submission_id}")
}

const fn map_store_error(error: StoreError) -> HumanSentError {
    match error {
        StoreError::Conflict => HumanSentError::Conflict,
        StoreError::MissingReference => HumanSentError::NotFound,
        StoreError::AuthenticationInvalid => HumanSentError::Forbidden,
        StoreError::AgentMailAdmission(_) | StoreError::Unavailable => HumanSentError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ControlPlaneTransaction, ExistingOrganizationBootstrap, HumanSentDeliveryEvidence,
        HumanSentListBatch, HumanSentListCandidate,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{
        AuthenticationBinding, HumanSessionId, MailboxAccessSnapshot, MailboxMembershipRole,
        Organization, OrganizationId, OrganizationSlug, Principal, PrincipalId, Role,
        RoleAssignment,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct Evidence {
        audits: Vec<String>,
        events: Vec<String>,
        serialized: Vec<String>,
        session_ids: Vec<Option<String>>,
        commits: usize,
        rechecks: Vec<HumanSentListRecheck>,
    }

    #[derive(Clone)]
    struct Store {
        batch: HumanSentListBatch,
        evidence: Arc<Mutex<Evidence>>,
        query_calls: Arc<AtomicUsize>,
        final_authorized: bool,
        commit_fails: bool,
    }

    struct Transaction {
        evidence: Arc<Mutex<Evidence>>,
        final_authorized: bool,
        commit_fails: bool,
    }

    #[async_trait]
    impl HumanSentQueryStore for Store {
        async fn list_human_sent_submissions(
            &self,
            _: &HumanSentListQuery,
        ) -> Result<HumanSentListBatch, StoreError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.batch.clone())
        }
    }

    #[async_trait]
    impl ControlPlaneStore for Store {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                evidence: self.evidence.clone(),
                final_authorized: self.final_authorized,
                commit_fails: self.commit_fails,
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for Transaction {
        async fn bind_request_authentication(
            &mut self,
            _: &AuthorizationContext,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
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

        async fn human_sent_list_still_authorized(
            &mut self,
            proof: &HumanSentListRecheck,
        ) -> Result<bool, StoreError> {
            self.evidence.lock().unwrap().rechecks.push(proof.clone());
            Ok(self.final_authorized)
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            let mut evidence = self.evidence.lock().unwrap();
            evidence.audits.push(event.action.clone());
            evidence.serialized.push(event.details.to_string());
            evidence.session_ids.push(event.context.session_id.clone());
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            let mut evidence = self.evidence.lock().unwrap();
            evidence.events.push(event.event_type.clone());
            evidence.serialized.push(event.payload.to_string());
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            if self.commit_fails {
                return Err(StoreError::Unavailable);
            }
            self.evidence.lock().unwrap().commits += 1;
            Ok(())
        }
    }

    struct Fixture {
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        now: DateTime<Utc>,
        context: AuthorizationContext,
        authorization: MailboxAccessSnapshot,
    }

    fn fixture() -> Fixture {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let now = DateTime::parse_from_rfc3339("2026-08-22T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        Fixture {
            organization_id,
            principal_id,
            mailbox_id,
            now,
            context: AuthorizationContext {
                principal_id,
                organization_id,
                permissions: [Permission::MailboxUse].into_iter().collect(),
                authentication: None,
            },
            authorization: MailboxAccessSnapshot {
                organization_id,
                mailbox_id,
                membership: Some(MailboxMembershipRole::Member),
                delegations: Vec::new(),
            },
        }
    }

    fn empty_batch(fixture: &Fixture) -> HumanSentListBatch {
        HumanSentListBatch {
            active_human: true,
            cursor_valid: true,
            authorization: Some(fixture.authorization.clone()),
            candidates: Vec::new(),
            has_more: false,
        }
    }

    fn store(_: &Fixture, batch: HumanSentListBatch, evidence: Arc<Mutex<Evidence>>) -> Store {
        Store {
            batch,
            evidence,
            query_calls: Arc::new(AtomicUsize::new(0)),
            final_authorized: true,
            commit_fails: false,
        }
    }

    fn command(fixture: &Fixture) -> ListHumanSentSubmissionsCommand {
        ListHumanSentSubmissionsCommand {
            context: fixture.context.clone(),
            mailbox_id: fixture.mailbox_id,
            limit: 2,
            cursor: None,
            trace_id: "trace-human-sent".into(),
            now: fixture.now,
        }
    }

    fn candidate(
        fixture: &Fixture,
        submission_id: MailSubmissionId,
        accepted_at: DateTime<Utc>,
        delivery: Option<HumanSentDeliveryEvidence>,
    ) -> HumanSentListCandidate {
        HumanSentListCandidate {
            submission_id,
            organization_id: fixture.organization_id,
            mailbox_id: fixture.mailbox_id,
            requested_by: fixture.principal_id,
            accepted_at,
            recipient_count: 2,
            delivery,
        }
    }

    #[tokio::test]
    async fn lists_safe_acceptance_order_and_commits_minimal_evidence() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let newest = MailSubmissionId::from_uuid(Uuid::from_u128(0x30));
        let older = MailSubmissionId::from_uuid(Uuid::from_u128(0x20));
        let newest_accepted_at = fixture.now;
        let older_accepted_at = fixture.now - chrono::Duration::seconds(1);
        let mut batch = empty_batch(&fixture);
        batch.candidates = vec![
            candidate(
                &fixture,
                newest,
                newest_accepted_at,
                Some(HumanSentDeliveryEvidence {
                    snapshot_id: Uuid::from_u128(0x40),
                    observed_at: newest_accepted_at,
                    terminal: false,
                    recipient_count: 2,
                    delivered_count: 1,
                    failed_count: 0,
                    queued_count: 1,
                    unknown_count: 0,
                }),
            ),
            candidate(&fixture, older, older_accepted_at, None),
        ];
        batch.has_more = true;
        let page = HumanSentService::new(store(&fixture, batch, evidence.clone()))
            .list(command(&fixture))
            .await
            .unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.submission_id)
                .collect::<Vec<_>>(),
            vec![newest, older]
        );
        assert_eq!(page.next_cursor, Some(format_cursor(older)));
        assert_eq!(page.items[0].delivery.as_ref().unwrap().delivered_count, 1);
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.rechecks.len(), 1);
        assert_eq!(
            evidence.rechecks[0].expected_submissions[0].submission_id,
            newest
        );
        assert_eq!(evidence.audits, ["mail.submissions.queried"]);
        assert_eq!(evidence.session_ids, vec![None]);
        assert_eq!(evidence.events, ["mail.submissions.queried.v1"]);
        assert_eq!(evidence.commits, 1);
        let serialized = evidence.serialized.join(" ");
        for secret in ["recipient@example.test", "provider-secret", "smtp"] {
            assert!(!serialized.contains(secret));
        }
    }

    #[tokio::test]
    async fn human_browser_session_is_bound_onto_the_query_audit() {
        let mut fixture = fixture();
        let session_id = HumanSessionId::from_uuid(Uuid::from_u128(0x5155));
        fixture.context.authentication = Some(AuthenticationBinding::HumanSession { session_id });
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        HumanSentService::new(store(&fixture, empty_batch(&fixture), evidence.clone()))
            .list(command(&fixture))
            .await
            .unwrap();
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.session_ids, vec![Some(session_id.to_string())]);
        let serialized = evidence.serialized.join(" ");
        assert!(!serialized.contains(&session_id.to_string()));
        assert!(!serialized.contains("session_token"));
    }

    #[tokio::test]
    async fn rejects_missing_rbac_and_noncanonical_cursor_before_query() {
        let first_fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let first_store = store(&first_fixture, empty_batch(&first_fixture), evidence);
        let calls = first_store.query_calls.clone();
        let mut invalid_rbac = command(&first_fixture);
        invalid_rbac.context.permissions.clear();
        assert_eq!(
            HumanSentService::new(first_store).list(invalid_rbac).await,
            Err(HumanSentError::Forbidden)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let second_fixture = fixture();
        let second_evidence = Arc::new(Mutex::new(Evidence::default()));
        let second_store = store(
            &second_fixture,
            empty_batch(&second_fixture),
            second_evidence,
        );
        let calls = second_store.query_calls.clone();
        let mut invalid_cursor = command(&second_fixture);
        invalid_cursor.cursor = Some(format!(
            "v1.{}",
            Uuid::from_u128(0xabcdef).to_string().to_uppercase()
        ));
        assert_eq!(
            HumanSentService::new(second_store)
                .list(invalid_cursor)
                .await,
            Err(HumanSentError::InvalidCursor)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn malformed_delivery_or_final_recheck_failure_never_discloses_or_audits() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let submission_id = MailSubmissionId::from_uuid(Uuid::from_u128(0x31));
        let mut malformed_batch = empty_batch(&fixture);
        malformed_batch.candidates = vec![candidate(
            &fixture,
            submission_id,
            fixture.now,
            Some(HumanSentDeliveryEvidence {
                snapshot_id: Uuid::from_u128(0x41),
                observed_at: fixture.now,
                terminal: true,
                recipient_count: 2,
                delivered_count: 1,
                failed_count: 0,
                queued_count: 1,
                unknown_count: 0,
            }),
        )];
        assert_eq!(
            HumanSentService::new(store(&fixture, malformed_batch, evidence.clone()))
                .list(command(&fixture))
                .await,
            Err(HumanSentError::Unavailable)
        );
        assert!(evidence.lock().unwrap().audits.is_empty());

        let denied_evidence = Arc::new(Mutex::new(Evidence::default()));
        let mut denied_store = store(&fixture, empty_batch(&fixture), denied_evidence.clone());
        denied_store.final_authorized = false;
        assert_eq!(
            HumanSentService::new(denied_store)
                .list(command(&fixture))
                .await,
            Err(HumanSentError::Conflict)
        );
        let denied_evidence = denied_evidence.lock().unwrap();
        assert_eq!(denied_evidence.rechecks.len(), 1);
        assert!(denied_evidence.audits.is_empty());
        assert!(denied_evidence.events.is_empty());
        assert_eq!(denied_evidence.commits, 0);
    }
}
