use crate::{
    AgentMailPolicyVersionRecord, ApprovalProof, ApprovalQueryStore, ControlPlaneStore,
    PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AgentMailPolicyActivation, AgentMailPolicyVersion, ApprovalAction, AuditActor, AuditContext,
    AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, DomainError, Permission,
    PrincipalId,
};
use serde_json::json;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ActivateAgentMailPolicyCommand {
    pub context: AuthorizationContext,
    pub agent_id: PrincipalId,
    pub policy_version_id: Uuid,
    pub approval: ApprovalProof,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateAgentMailPolicyVersionCommand {
    pub context: AuthorizationContext,
    pub agent_id: PrincipalId,
    pub allowed_recipient_domains: BTreeSet<String>,
    pub max_recipients_per_message: u16,
    pub max_messages_per_hour: u32,
    pub max_messages_per_recipient_per_day: u16,
    pub require_first_contact_approval: bool,
    pub max_auto_replies_per_thread_per_day: u8,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct GetAgentMailPolicyVersionCommand {
    pub context: AuthorizationContext,
    pub agent_id: PrincipalId,
    pub policy_version_id: Uuid,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentMailPolicyError {
    #[error("operation is forbidden")]
    Forbidden,
    #[error("policy version is unavailable")]
    NotFound,
    #[error("policy activation conflicts with current state")]
    Conflict,
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct AgentMailPolicyService<S> {
    store: S,
}

impl<S> AgentMailPolicyService<S>
where
    S: ControlPlaneStore + ApprovalQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create_version(
        &self,
        command: CreateAgentMailPolicyVersionCommand,
    ) -> Result<AgentMailPolicyVersionRecord, AgentMailPolicyError> {
        if !command.context.permits(
            command.context.organization_id,
            Permission::AuthorizationManage,
        ) {
            return Err(AgentMailPolicyError::Forbidden);
        }
        let mut transaction = self.store.begin().await?;
        if !transaction
            .active_human_principal_exists(
                command.context.organization_id,
                command.context.principal_id,
            )
            .await?
        {
            return Err(AgentMailPolicyError::Forbidden);
        }
        let version = transaction
            .next_agent_mail_policy_version(command.context.organization_id, command.agent_id)
            .await?;
        let policy = AgentMailPolicyVersion::new(
            command.context.organization_id,
            command.agent_id,
            version,
            command.allowed_recipient_domains,
            command.max_recipients_per_message,
            command.max_messages_per_hour,
            command.max_messages_per_recipient_per_day,
            command.require_first_contact_approval,
            command.max_auto_replies_per_thread_per_day,
        )?;
        let record = AgentMailPolicyVersionRecord {
            id: Uuid::now_v7(),
            policy,
            created_by: command.context.principal_id,
            created_at: command.now,
        };
        transaction
            .insert_agent_mail_policy_version(&record)
            .await?;
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.mail_policy.versioned",
            AuditResource {
                kind: "agent.mail_policy".into(),
                id: command.agent_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "agent_id": command.agent_id,
                "policy_version_id": record.id,
                "version": record.policy.version(),
                "allowed_domain_count": record.policy.allowed_recipient_domains().len(),
                "max_recipients_per_message": record.policy.max_recipients_per_message(),
                "max_messages_per_hour": record.policy.max_messages_per_hour(),
                "max_messages_per_recipient_per_day": record.policy.max_messages_per_recipient_per_day(),
                "require_first_contact_approval": record.policy.require_first_contact_approval(),
                "max_auto_replies_per_thread_per_day": record.policy.max_auto_replies_per_thread_per_day(),
            }),
            command.now,
        )?;
        transaction.append_audit_event(&audit).await?;
        transaction
            .enqueue_event(&PendingEvent::new(
                "agent.mail_policy.versioned.v1",
                "agent.mail_policy",
                command.agent_id.to_string(),
                json!({
                    "organization_id": command.context.organization_id,
                    "agent_id": command.agent_id,
                    "policy_version_id": record.id,
                    "version": record.policy.version(),
                }),
                command.now,
            ))
            .await?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn get_version(
        &self,
        command: GetAgentMailPolicyVersionCommand,
    ) -> Result<AgentMailPolicyVersionRecord, AgentMailPolicyError> {
        if !command.context.permits(
            command.context.organization_id,
            Permission::AuthorizationManage,
        ) {
            return Err(AgentMailPolicyError::Forbidden);
        }
        let mut transaction = self.store.begin().await?;
        if !transaction
            .active_human_principal_exists(
                command.context.organization_id,
                command.context.principal_id,
            )
            .await?
        {
            return Err(AgentMailPolicyError::Forbidden);
        }
        let record = transaction
            .find_agent_mail_policy_version(
                command.context.organization_id,
                command.agent_id,
                command.policy_version_id,
            )
            .await?
            .ok_or(AgentMailPolicyError::NotFound)?;
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.mail_policy.queried",
            AuditResource {
                kind: "agent.mail_policy_query".into(),
                id: command.policy_version_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "agent_id": command.agent_id,
                "policy_version_id": record.id,
                "version": record.policy.version(),
            }),
            command.now,
        )?;
        transaction.append_audit_event(&audit).await?;
        transaction
            .enqueue_event(&PendingEvent::new(
                "agent.mail_policy.queried.v1",
                "agent.mail_policy_query",
                command.policy_version_id.to_string(),
                json!({
                    "organization_id": command.context.organization_id,
                    "agent_id": command.agent_id,
                    "policy_version_id": record.id,
                }),
                command.now,
            ))
            .await?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn activate(
        &self,
        command: ActivateAgentMailPolicyCommand,
    ) -> Result<AgentMailPolicyActivation, AgentMailPolicyError> {
        if !command.context.permits(
            command.context.organization_id,
            Permission::AuthorizationManage,
        ) || command.approval.organization_id != command.context.organization_id
            || command.approval.requested_by != command.context.principal_id
            || command.approval.action != ApprovalAction::ProductionAgentMail
            || command.approval.resource_kind != "agent.mail_policy_version"
            || command.approval.resource_id != command.policy_version_id.to_string()
            || command.approval.now != command.now
        {
            return Err(AgentMailPolicyError::Forbidden);
        }
        let evidence = self
            .store
            .approved_request_evidence(&command.approval)
            .await?
            .ok_or(AgentMailPolicyError::Forbidden)?;
        if evidence.required_approvals < 2
            || evidence.approval_count < evidence.required_approvals
            || evidence.expires_at <= command.now
        {
            return Err(AgentMailPolicyError::Forbidden);
        }

        let mut transaction = self.store.begin().await?;
        if !transaction
            .active_human_principal_exists(
                command.context.organization_id,
                command.context.principal_id,
            )
            .await?
        {
            return Err(AgentMailPolicyError::Forbidden);
        }
        let version = transaction
            .find_agent_mail_policy_version(
                command.context.organization_id,
                command.agent_id,
                command.policy_version_id,
            )
            .await?
            .ok_or(AgentMailPolicyError::NotFound)?;
        if let Some(existing) = transaction
            .find_agent_mail_policy_activation_by_approval(
                command.context.organization_id,
                command.approval.request_id,
            )
            .await?
        {
            if existing.agent_id() == command.agent_id
                && existing.policy_version() == version.policy.version()
                && existing.activated_by() == command.context.principal_id
            {
                return Ok(existing);
            }
            return Err(AgentMailPolicyError::Conflict);
        }

        let activation = AgentMailPolicyActivation::new(
            command.context.organization_id,
            command.agent_id,
            version.policy.version(),
            command.approval.request_id,
            command.context.principal_id,
            command.now,
        )?;
        transaction
            .insert_agent_mail_policy_activation(&activation, version.id)
            .await?;

        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.subject_principal_id = Some(command.agent_id);
        audit_context.approval_request_id = Some(command.approval.request_id);
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.mail_policy.activated",
            AuditResource {
                kind: "agent.mail_policy".into(),
                id: command.agent_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "agent_id": command.agent_id,
                "policy_version_id": version.id,
                "version": version.policy.version(),
            }),
            command.now,
        )?
        .with_context(audit_context)?;
        transaction.append_audit_event(&audit).await?;
        transaction
            .enqueue_event(&PendingEvent::new(
                "agent.mail_policy.activated.v1",
                "agent.mail_policy",
                command.agent_id.to_string(),
                json!({
                    "organization_id": command.context.organization_id,
                    "agent_id": command.agent_id,
                    "policy_version_id": version.id,
                    "version": version.policy.version(),
                }),
                command.now,
            ))
            .await?;
        transaction.commit().await?;
        Ok(activation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ControlPlaneTransaction;
    use async_trait::async_trait;
    use copilotmail_domain::{ApprovalRequestId, OrganizationId};
    use std::collections::BTreeSet;

    struct NeverStore;

    #[async_trait]
    impl ControlPlaneStore for NeverStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            panic!("authorization failure must precede transaction access")
        }
    }

    #[async_trait]
    impl ApprovalQueryStore for NeverStore {
        async fn approval_permits(&self, _: &ApprovalProof) -> Result<bool, StoreError> {
            panic!("authorization failure must precede approval access")
        }
    }

    #[tokio::test]
    async fn missing_authorization_manage_fails_before_approval_or_storage() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let now = Utc::now();
        let policy_version_id = Uuid::now_v7();
        let create_result = AgentMailPolicyService::new(NeverStore)
            .create_version(CreateAgentMailPolicyVersionCommand {
                context: AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: BTreeSet::new(),
                },
                agent_id: PrincipalId::new(),
                allowed_recipient_domains: BTreeSet::from(["internal.example".into()]),
                max_recipients_per_message: 5,
                max_messages_per_hour: 100,
                max_messages_per_recipient_per_day: 10,
                require_first_contact_approval: true,
                max_auto_replies_per_thread_per_day: 2,
                trace_id: "agent-mail-policy-create-denied".into(),
                now,
            })
            .await;
        assert!(matches!(
            create_result,
            Err(AgentMailPolicyError::Forbidden)
        ));
        let get_result = AgentMailPolicyService::new(NeverStore)
            .get_version(GetAgentMailPolicyVersionCommand {
                context: AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: BTreeSet::new(),
                },
                agent_id: PrincipalId::new(),
                policy_version_id,
                trace_id: "agent-mail-policy-get-denied".into(),
                now,
            })
            .await;
        assert!(matches!(get_result, Err(AgentMailPolicyError::Forbidden)));
        let result = AgentMailPolicyService::new(NeverStore)
            .activate(ActivateAgentMailPolicyCommand {
                context: AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: BTreeSet::new(),
                },
                agent_id: PrincipalId::new(),
                policy_version_id,
                approval: ApprovalProof {
                    organization_id,
                    request_id: ApprovalRequestId::new(),
                    requested_by: principal_id,
                    action: ApprovalAction::ProductionAgentMail,
                    resource_kind: "agent.mail_policy_version".into(),
                    resource_id: policy_version_id.to_string(),
                    now,
                },
                trace_id: "agent-mail-policy-denied".into(),
                now,
            })
            .await;
        assert!(matches!(result, Err(AgentMailPolicyError::Forbidden)));
    }
}
