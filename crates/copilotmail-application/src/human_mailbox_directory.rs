use crate::{HumanMailboxDirectoryQuery, HumanMailboxDirectoryStore, StoreError};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuthorizationContext, MailboxAddress, MailboxCapability, MailboxId, MailboxKind, Permission,
};
use uuid::Uuid;

const MIN_LIMIT: u16 = 1;
const MAX_LIMIT: u16 = 50;
const CURSOR_PREFIX: &str = "v1.";

#[derive(Debug, Clone)]
pub struct ListHumanMailboxDirectoryCommand {
    pub context: AuthorizationContext,
    pub limit: u16,
    pub cursor: Option<String>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailboxDirectoryItem {
    pub mailbox_id: MailboxId,
    pub kind: MailboxKind,
    pub primary_address: MailboxAddress,
    pub can_send: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailboxDirectoryPage {
    pub items: Vec<HumanMailboxDirectoryItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HumanMailboxDirectoryError {
    #[error("Human mailbox directory access is forbidden")]
    Forbidden,
    #[error("mailbox directory page size must be between 1 and 50")]
    InvalidPageSize,
    #[error("the mailbox directory cursor is invalid or no longer visible")]
    InvalidCursor,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct HumanMailboxDirectoryService<S> {
    store: S,
}

impl<S: HumanMailboxDirectoryStore> HumanMailboxDirectoryService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn list(
        &self,
        command: ListHumanMailboxDirectoryCommand,
    ) -> Result<HumanMailboxDirectoryPage, HumanMailboxDirectoryError> {
        if !command
            .context
            .permits(command.context.organization_id, Permission::MailboxUse)
        {
            return Err(HumanMailboxDirectoryError::Forbidden);
        }
        if !(MIN_LIMIT..=MAX_LIMIT).contains(&command.limit) {
            return Err(HumanMailboxDirectoryError::InvalidPageSize);
        }
        let after_mailbox_id = command.cursor.as_deref().map(parse_cursor).transpose()?;
        let batch = self
            .store
            .list_human_mailbox_directory(&HumanMailboxDirectoryQuery {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                after_mailbox_id,
                limit: command.limit,
                now: command.now,
            })
            .await?;
        if !batch.active_human {
            return Err(HumanMailboxDirectoryError::Forbidden);
        }
        if after_mailbox_id.is_some() && !batch.cursor_valid {
            return Err(HumanMailboxDirectoryError::InvalidCursor);
        }
        if batch.candidates.len() > usize::from(command.limit)
            || (batch.has_more && batch.candidates.is_empty())
        {
            return Err(StoreError::Unavailable.into());
        }

        let mut previous = after_mailbox_id;
        let mut items = Vec::with_capacity(batch.candidates.len());
        for candidate in batch.candidates {
            let mailbox = candidate.mailbox;
            if mailbox.organization_id != command.context.organization_id
                || candidate.authorization.organization_id != command.context.organization_id
                || candidate.authorization.mailbox_id != mailbox.mailbox_id
                || candidate.data_plane_id.as_uuid().is_nil()
                || previous.is_some_and(|cursor| mailbox.mailbox_id <= cursor)
                || candidate
                    .authorization
                    .authorize(
                        command.context.organization_id,
                        mailbox.mailbox_id,
                        MailboxCapability::Read,
                        None,
                    )
                    .is_none()
            {
                return Err(StoreError::Unavailable.into());
            }
            let can_send = candidate
                .authorization
                .authorize(
                    command.context.organization_id,
                    mailbox.mailbox_id,
                    MailboxCapability::Send,
                    None,
                )
                .is_some();
            previous = Some(mailbox.mailbox_id);
            items.push(HumanMailboxDirectoryItem {
                mailbox_id: mailbox.mailbox_id,
                kind: mailbox.kind,
                primary_address: mailbox.primary_address,
                can_send,
            });
        }
        let next_cursor = if batch.has_more {
            items.last().map(|item| format_cursor(item.mailbox_id))
        } else {
            None
        };
        Ok(HumanMailboxDirectoryPage { items, next_cursor })
    }
}

fn parse_cursor(value: &str) -> Result<MailboxId, HumanMailboxDirectoryError> {
    let raw = value
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(HumanMailboxDirectoryError::InvalidCursor)?;
    let parsed = Uuid::parse_str(raw).map_err(|_| HumanMailboxDirectoryError::InvalidCursor)?;
    if raw != parsed.to_string() {
        return Err(HumanMailboxDirectoryError::InvalidCursor);
    }
    Ok(MailboxId::from_uuid(parsed))
}

fn format_cursor(mailbox_id: MailboxId) -> String {
    format!("{CURSOR_PREFIX}{mailbox_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        HumanMailboxDirectoryBatch, HumanMailboxDirectoryCandidate, HumanMailboxDirectoryStore,
        MailboxSummary,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{
        DelegationId, MailDataPlaneId, MailboxAccessSnapshot, MailboxMembershipRole,
        OrganizationId, PrincipalId, ResolvedMailboxDelegation,
    };
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct Store {
        batch: HumanMailboxDirectoryBatch,
    }

    #[async_trait]
    impl HumanMailboxDirectoryStore for Store {
        async fn list_human_mailbox_directory(
            &self,
            _query: &HumanMailboxDirectoryQuery,
        ) -> Result<HumanMailboxDirectoryBatch, StoreError> {
            Ok(self.batch.clone())
        }
    }

    #[derive(Default)]
    struct CountingStore {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl HumanMailboxDirectoryStore for &CountingStore {
        async fn list_human_mailbox_directory(
            &self,
            _query: &HumanMailboxDirectoryQuery,
        ) -> Result<HumanMailboxDirectoryBatch, StoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: true,
                candidates: Vec::new(),
                has_more: false,
            })
        }
    }

    fn context(organization_id: OrganizationId, principal_id: PrincipalId) -> AuthorizationContext {
        AuthorizationContext {
            organization_id,
            principal_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
            authentication: None,
        }
    }

    fn candidate(
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        membership: Option<MailboxMembershipRole>,
        capabilities: BTreeSet<MailboxCapability>,
    ) -> HumanMailboxDirectoryCandidate {
        HumanMailboxDirectoryCandidate {
            mailbox: MailboxSummary {
                mailbox_id,
                organization_id,
                kind: MailboxKind::Shared,
                primary_address: MailboxAddress::parse(format!("{mailbox_id}@example.test"))
                    .unwrap(),
                created_at: Utc::now(),
            },
            data_plane_id: MailDataPlaneId::new(),
            authorization: MailboxAccessSnapshot {
                organization_id,
                mailbox_id,
                membership,
                delegations: if capabilities.is_empty() {
                    Vec::new()
                } else {
                    vec![ResolvedMailboxDelegation {
                        delegation_id: DelegationId::new(),
                        grantor_principal_id: PrincipalId::new(),
                        capabilities,
                    }]
                },
            },
        }
    }

    #[tokio::test]
    async fn derives_send_from_the_same_snapshot_and_not_from_send_as() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let first = MailboxId::from_uuid(Uuid::from_u128(1));
        let second = MailboxId::from_uuid(Uuid::from_u128(2));
        let service = HumanMailboxDirectoryService::new(Store {
            batch: HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: true,
                candidates: vec![
                    candidate(
                        organization_id,
                        first,
                        None,
                        [MailboxCapability::Read, MailboxCapability::SendAs]
                            .into_iter()
                            .collect(),
                    ),
                    candidate(
                        organization_id,
                        second,
                        Some(MailboxMembershipRole::Member),
                        BTreeSet::new(),
                    ),
                ],
                has_more: true,
            },
        });
        let page = service
            .list(ListHumanMailboxDirectoryCommand {
                context: context(organization_id, principal_id),
                limit: 2,
                cursor: None,
                now: Utc::now(),
            })
            .await
            .unwrap();
        assert!(!page.items[0].can_send);
        assert!(page.items[1].can_send);
        assert_eq!(page.next_cursor, Some(format_cursor(second)));
    }

    #[tokio::test]
    async fn rejects_noncanonical_and_invisible_cursors_and_store_auth_drift() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let base = ListHumanMailboxDirectoryCommand {
            context: context(organization_id, principal_id),
            limit: 25,
            cursor: Some("v1.NOT-A-UUID".into()),
            now: Utc::now(),
        };
        let service = HumanMailboxDirectoryService::new(Store {
            batch: HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: true,
                candidates: Vec::new(),
                has_more: false,
            },
        });
        assert!(matches!(
            service.list(base).await,
            Err(HumanMailboxDirectoryError::InvalidCursor)
        ));

        let cursor_id = MailboxId::new();
        let service = HumanMailboxDirectoryService::new(Store {
            batch: HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            },
        });
        assert!(matches!(
            service
                .list(ListHumanMailboxDirectoryCommand {
                    context: context(organization_id, principal_id),
                    limit: 25,
                    cursor: Some(format_cursor(cursor_id)),
                    now: Utc::now(),
                })
                .await,
            Err(HumanMailboxDirectoryError::InvalidCursor)
        ));

        let service = HumanMailboxDirectoryService::new(Store {
            batch: HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: true,
                candidates: vec![candidate(
                    organization_id,
                    MailboxId::new(),
                    None,
                    [MailboxCapability::SendAs].into_iter().collect(),
                )],
                has_more: false,
            },
        });
        assert!(matches!(
            service
                .list(ListHumanMailboxDirectoryCommand {
                    context: context(organization_id, principal_id),
                    limit: 25,
                    cursor: None,
                    now: Utc::now(),
                })
                .await,
            Err(HumanMailboxDirectoryError::Store(StoreError::Unavailable))
        ));

        let service = HumanMailboxDirectoryService::new(Store {
            batch: HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: true,
                candidates: vec![candidate(
                    OrganizationId::new(),
                    MailboxId::new(),
                    Some(MailboxMembershipRole::Owner),
                    BTreeSet::new(),
                )],
                has_more: false,
            },
        });
        assert!(matches!(
            service
                .list(ListHumanMailboxDirectoryCommand {
                    context: context(organization_id, principal_id),
                    limit: 25,
                    cursor: None,
                    now: Utc::now(),
                })
                .await,
            Err(HumanMailboxDirectoryError::Store(StoreError::Unavailable))
        ));
    }

    #[tokio::test]
    async fn rejects_missing_rbac_inactive_humans_and_invalid_limits() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let counting = CountingStore::default();
        let no_rbac = HumanMailboxDirectoryService::new(&counting);
        assert!(matches!(
            no_rbac
                .list(ListHumanMailboxDirectoryCommand {
                    context: AuthorizationContext {
                        organization_id,
                        principal_id,
                        permissions: BTreeSet::new(),
                        authentication: None,
                    },
                    limit: 25,
                    cursor: None,
                    now: Utc::now(),
                })
                .await,
            Err(HumanMailboxDirectoryError::Forbidden)
        ));
        assert_eq!(counting.calls.load(Ordering::SeqCst), 0);

        let inactive = HumanMailboxDirectoryService::new(Store {
            batch: HumanMailboxDirectoryBatch {
                active_human: false,
                cursor_valid: true,
                candidates: Vec::new(),
                has_more: false,
            },
        });
        assert!(matches!(
            inactive
                .list(ListHumanMailboxDirectoryCommand {
                    context: context(organization_id, principal_id),
                    limit: 25,
                    cursor: None,
                    now: Utc::now(),
                })
                .await,
            Err(HumanMailboxDirectoryError::Forbidden)
        ));
        let service = HumanMailboxDirectoryService::new(Store {
            batch: HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: true,
                candidates: Vec::new(),
                has_more: false,
            },
        });
        for limit in [0, 51] {
            assert!(matches!(
                service
                    .list(ListHumanMailboxDirectoryCommand {
                        context: context(organization_id, principal_id),
                        limit,
                        cursor: None,
                        now: Utc::now(),
                    })
                    .await,
                Err(HumanMailboxDirectoryError::InvalidPageSize)
            ));
        }
    }
}
