use crate::{
    ApprovalProof, ApprovalQueryStore, ControlPlaneStore, PendingEvent, RoleAssignmentGrantRecord,
    RoleAssignmentListQuery, RoleAssignmentRevocationResult, RoleAssignmentSummary,
    RoleGovernanceQueryStore, RoleSummary, StoreError,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use copilotmail_domain::{
    ApprovalAction, AuditActor, AuditContext, AuditEvent, AuditOutcome, AuditResource,
    AuthorizationContext, DomainError, OrganizationId, Permission, PrincipalId, RoleAssignment,
    RoleAssignmentId, RoleId,
};
use serde_json::json;

const MAX_ASSIGNMENT_LIFETIME: Duration = Duration::days(365);

#[derive(Debug, Clone)]
pub struct GrantRoleAssignmentCommand {
    pub context: AuthorizationContext,
    pub principal_id: PrincipalId,
    pub role_id: RoleId,
    pub valid_until: DateTime<Utc>,
    pub approval: ApprovalProof,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RevokeRoleAssignmentCommand {
    pub context: AuthorizationContext,
    pub assignment_id: RoleAssignmentId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ListRolesCommand {
    pub context: AuthorizationContext,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ListRoleAssignmentsCommand {
    pub context: AuthorizationContext,
    pub principal_id: Option<PrincipalId>,
    pub include_inactive: bool,
    pub after_assignment_id: Option<RoleAssignmentId>,
    pub limit: i64,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssignmentPage {
    pub items: Vec<RoleAssignmentSummary>,
    pub has_more: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum RoleGovernanceError {
    #[error("role governance access is forbidden")]
    Forbidden,
    #[error("role or assignment was not found")]
    NotFound,
    #[error("role assignment conflicts with current state")]
    Conflict,
    #[error("the last effective organization owner cannot be revoked")]
    LastActiveOwner,
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct RoleGovernanceService<S> {
    store: S,
}

impl<S> RoleGovernanceService<S>
where
    S: ControlPlaneStore + ApprovalQueryStore + RoleGovernanceQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn grant(
        &self,
        command: GrantRoleAssignmentCommand,
    ) -> Result<RoleAssignmentGrantRecord, RoleGovernanceError> {
        ensure_authorization_manager(&command.context)?;
        validate_grant_window(command.now, command.valid_until)?;
        let expected_resource_id = role_assignment_approval_resource_id(
            command.principal_id,
            command.role_id,
            command.valid_until,
        )?;
        if command.approval.organization_id != command.context.organization_id
            || command.approval.requested_by != command.context.principal_id
            || command.approval.action != ApprovalAction::PrivilegeElevation
            || command.approval.resource_kind != "authorization.role_assignment"
            || command.approval.resource_id != expected_resource_id
            || command.approval.now != command.now
        {
            return Err(RoleGovernanceError::Forbidden);
        }
        let approval = self
            .store
            .approved_request_evidence(&command.approval)
            .await?
            .ok_or(RoleGovernanceError::Forbidden)?;
        if approval.required_approvals < 2
            || approval.approval_count < approval.required_approvals
            || approval.expires_at <= command.now
        {
            return Err(RoleGovernanceError::Forbidden);
        }

        let role = self
            .store
            .list_roles(command.context.organization_id)
            .await?
            .into_iter()
            .find(|role| role.role_id == command.role_id)
            .ok_or(RoleGovernanceError::NotFound)?;

        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_active_human(
            &mut transaction,
            command.context.organization_id,
            command.context.principal_id,
        )
        .await?;
        if !transaction
            .active_principal_exists(command.context.organization_id, command.principal_id)
            .await?
        {
            return Err(RoleGovernanceError::NotFound);
        }
        if let Some(existing) = transaction
            .find_role_assignment_grant_by_approval(
                command.context.organization_id,
                command.approval.request_id,
            )
            .await?
        {
            if existing.principal_id == command.principal_id
                && existing.role_id == command.role_id
                && existing.granted_by == command.context.principal_id
                && existing.valid_until == command.valid_until
            {
                return Ok(existing);
            }
            return Err(RoleGovernanceError::Conflict);
        }

        let assignment = RoleAssignment::new(
            command.context.organization_id,
            command.principal_id,
            command.role_id,
            command.context.principal_id,
            command.now,
            Some(command.valid_until),
            command.now,
        )?;
        transaction.insert_role_assignment(&assignment).await?;
        transaction
            .insert_role_assignment_grant(&assignment, command.approval.request_id)
            .await?;
        let record = RoleAssignmentGrantRecord {
            assignment_id: assignment.id(),
            organization_id: assignment.organization_id(),
            principal_id: assignment.principal_id(),
            role_id: assignment.role_id(),
            approval_request_id: command.approval.request_id,
            granted_by: assignment.created_by(),
            valid_from: assignment.valid_from(),
            valid_until: command.valid_until,
        };
        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.subject_principal_id = Some(command.principal_id);
        audit_context.approval_request_id = Some(command.approval.request_id);
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.role_assignment.granted",
            AuditResource {
                kind: "authorization.role_assignment".into(),
                id: assignment.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "principal_id": command.principal_id,
                "role_id": command.role_id,
                "system_role": role.system_role,
                "valid_until": command.valid_until.to_rfc3339_opts(
                    SecondsFormat::Millis,
                    true
                ),
            }),
            command.now,
        )?
        .with_context(audit_context)?;
        transaction.append_audit_event(&audit).await?;
        transaction
            .enqueue_event(&PendingEvent::new(
                "authorization.role_assignment.granted.v1",
                "authorization.role_assignment",
                assignment.id().to_string(),
                json!({
                    "organization_id": command.context.organization_id,
                    "assignment_id": assignment.id(),
                    "principal_id": command.principal_id,
                    "role_id": command.role_id,
                }),
                command.now,
            ))
            .await?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn revoke(
        &self,
        command: RevokeRoleAssignmentCommand,
    ) -> Result<RoleAssignmentSummary, RoleGovernanceError> {
        ensure_authorization_manager(&command.context)?;
        let assignment = self
            .store
            .get_role_assignment(command.context.organization_id, command.assignment_id)
            .await?
            .ok_or(RoleGovernanceError::NotFound)?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_active_human(
            &mut transaction,
            command.context.organization_id,
            command.context.principal_id,
        )
        .await?;
        match transaction
            .revoke_governed_role_assignment(
                command.context.organization_id,
                command.assignment_id,
                command.now,
            )
            .await?
        {
            RoleAssignmentRevocationResult::AlreadyRevoked => return Ok(assignment),
            RoleAssignmentRevocationResult::LastActiveOwner => {
                return Err(RoleGovernanceError::LastActiveOwner);
            }
            RoleAssignmentRevocationResult::NotFound => return Err(RoleGovernanceError::NotFound),
            RoleAssignmentRevocationResult::Revoked => {}
        }
        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.subject_principal_id = Some(assignment.principal_id);
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.role_assignment.revoked",
            AuditResource {
                kind: "authorization.role_assignment".into(),
                id: command.assignment_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "principal_id": assignment.principal_id,
                "role_id": assignment.role_id,
                "system_role": assignment.system_role,
            }),
            command.now,
        )?
        .with_context(audit_context)?;
        transaction.append_audit_event(&audit).await?;
        transaction
            .enqueue_event(&PendingEvent::new(
                "authorization.role_assignment.revoked.v1",
                "authorization.role_assignment",
                command.assignment_id.to_string(),
                json!({
                    "organization_id": command.context.organization_id,
                    "assignment_id": command.assignment_id,
                    "principal_id": assignment.principal_id,
                    "role_id": assignment.role_id,
                }),
                command.now,
            ))
            .await?;
        transaction.commit().await?;

        Ok(RoleAssignmentSummary {
            revoked_at: Some(command.now),
            ..assignment
        })
    }

    pub async fn list_roles(
        &self,
        command: ListRolesCommand,
    ) -> Result<Vec<RoleSummary>, RoleGovernanceError> {
        ensure_authorization_manager(&command.context)?;
        let roles = self
            .store
            .list_roles(command.context.organization_id)
            .await?;
        append_read_evidence(
            &self.store,
            &command.context,
            json!({"operation": "list_roles", "result_count": roles.len()}),
            &command.trace_id,
            command.now,
        )
        .await?;
        Ok(roles)
    }

    pub async fn list_assignments(
        &self,
        command: ListRoleAssignmentsCommand,
    ) -> Result<RoleAssignmentPage, RoleGovernanceError> {
        ensure_authorization_manager(&command.context)?;
        if !(1..=100).contains(&command.limit) {
            return Err(RoleGovernanceError::Domain(
                DomainError::InvalidRoleAssignmentWindow,
            ));
        }
        let mut items = self
            .store
            .list_role_assignments(RoleAssignmentListQuery {
                organization_id: command.context.organization_id,
                principal_id: command.principal_id,
                include_inactive: command.include_inactive,
                after_assignment_id: command.after_assignment_id,
                limit: command.limit + 1,
                now: command.now,
            })
            .await?;
        let has_more = items.len() > command.limit as usize;
        if has_more {
            items.truncate(command.limit as usize);
        }
        append_read_evidence(
            &self.store,
            &command.context,
            json!({
                "operation": "list_assignments",
                "principal_filter": command.principal_id,
                "include_inactive": command.include_inactive,
                "cursor_present": command.after_assignment_id.is_some(),
                "result_count": items.len(),
                "has_more": has_more,
            }),
            &command.trace_id,
            command.now,
        )
        .await?;
        Ok(RoleAssignmentPage { items, has_more })
    }
}

pub fn role_assignment_approval_resource_id(
    principal_id: PrincipalId,
    role_id: RoleId,
    valid_until: DateTime<Utc>,
) -> Result<String, DomainError> {
    if !valid_until
        .timestamp_subsec_nanos()
        .is_multiple_of(1_000_000)
    {
        return Err(DomainError::InvalidRoleAssignmentWindow);
    }
    Ok(format!(
        "v1:{principal_id}:{role_id}:{}",
        valid_until.timestamp_millis()
    ))
}

fn validate_grant_window(
    now: DateTime<Utc>,
    valid_until: DateTime<Utc>,
) -> Result<(), DomainError> {
    if valid_until <= now
        || valid_until > now + MAX_ASSIGNMENT_LIFETIME
        || !valid_until
            .timestamp_subsec_nanos()
            .is_multiple_of(1_000_000)
    {
        return Err(DomainError::InvalidRoleAssignmentWindow);
    }
    Ok(())
}

fn ensure_authorization_manager(context: &AuthorizationContext) -> Result<(), RoleGovernanceError> {
    if !context.permits(context.organization_id, Permission::AuthorizationManage) {
        return Err(RoleGovernanceError::Forbidden);
    }
    Ok(())
}

async fn ensure_active_human(
    transaction: &mut Box<dyn crate::ControlPlaneTransaction>,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
) -> Result<(), RoleGovernanceError> {
    if !transaction
        .authorize_human_permission(
            organization_id,
            principal_id,
            Permission::AuthorizationManage,
        )
        .await?
    {
        return Err(RoleGovernanceError::Forbidden);
    }
    Ok(())
}

async fn append_read_evidence<S>(
    store: &S,
    context: &AuthorizationContext,
    details: serde_json::Value,
    trace_id: &str,
    now: DateTime<Utc>,
) -> Result<(), RoleGovernanceError>
where
    S: ControlPlaneStore,
{
    let mut transaction = store.begin_authorized(context).await?;
    ensure_active_human(
        &mut transaction,
        context.organization_id,
        context.principal_id,
    )
    .await?;
    let mut audit_context = AuditContext::for_request(trace_id.to_owned())?;
    audit_context.subject_principal_id = Some(context.principal_id);
    let audit = AuditEvent::new(
        context.organization_id,
        AuditActor {
            principal_id: context.principal_id,
            on_behalf_of: None,
            credential_id: None,
        },
        "authorization.role_assignment.queried",
        AuditResource {
            kind: "authorization.role_assignment_query".into(),
            id: context.organization_id.to_string(),
        },
        AuditOutcome::Success,
        trace_id.to_owned(),
        details,
        now,
    )?
    .with_context(audit_context)?;
    transaction.append_audit_event(&audit).await?;
    transaction
        .enqueue_event(&PendingEvent::new(
            "authorization.role_assignment.queried.v1",
            "authorization.role_assignment_query",
            context.organization_id.to_string(),
            json!({"organization_id": context.organization_id}),
            now,
        ))
        .await?;
    transaction.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn approval_resource_binds_target_role_and_exact_millisecond_expiry() {
        let principal_id = PrincipalId::new();
        let role_id = RoleId::new();
        let valid_until = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).single().unwrap()
            + Duration::milliseconds(123);
        assert_eq!(
            role_assignment_approval_resource_id(principal_id, role_id, valid_until).unwrap(),
            format!(
                "v1:{principal_id}:{role_id}:{}",
                valid_until.timestamp_millis()
            )
        );
        assert_ne!(
            role_assignment_approval_resource_id(PrincipalId::new(), role_id, valid_until).unwrap(),
            role_assignment_approval_resource_id(principal_id, role_id, valid_until).unwrap()
        );
        assert_ne!(
            role_assignment_approval_resource_id(principal_id, RoleId::new(), valid_until).unwrap(),
            role_assignment_approval_resource_id(principal_id, role_id, valid_until).unwrap()
        );
    }

    #[test]
    fn grant_window_is_bounded_and_rejects_sub_millisecond_instants() {
        let now = Utc.with_ymd_and_hms(2026, 7, 26, 0, 0, 0).single().unwrap();
        assert!(validate_grant_window(now, now + Duration::days(365)).is_ok());
        assert!(validate_grant_window(now, now).is_err());
        assert!(
            validate_grant_window(now, now + Duration::days(365) + Duration::milliseconds(1))
                .is_err()
        );
        assert!(
            role_assignment_approval_resource_id(
                PrincipalId::new(),
                RoleId::new(),
                now + Duration::nanoseconds(1),
            )
            .is_err()
        );
    }
}
