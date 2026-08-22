use crate::{
    ControlPlaneStore, ControlPlaneTransaction, MailDomainDnsObservation, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext,
    DnsVerificationEvidence, DomainError, MailDataPlaneId, MailDomain, MailDomainId,
    MailDomainName, MailDomainRoute, MailDomainRouteMode, MailDomainStatus, OrganizationId,
    Permission, PrincipalId,
};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct RegisterMailDomainCommand {
    pub organization_id: OrganizationId,
    pub name: String,
    pub route_mode: MailDomainRouteMode,
    pub ownership_challenge_sha256: [u8; 32],
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterMailDomainResult {
    pub mail_domain_id: MailDomainId,
    pub name: String,
    pub status: MailDomainStatus,
    pub ownership_challenge_version: i64,
    pub route_version: i64,
}

#[derive(Debug, Clone)]
pub struct RotateMailDomainChallengeCommand {
    pub organization_id: OrganizationId,
    pub mail_domain_id: MailDomainId,
    pub expected_version: i64,
    pub ownership_challenge_sha256: [u8; 32],
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotateMailDomainChallengeResult {
    pub mail_domain_id: MailDomainId,
    pub name: String,
    pub status: MailDomainStatus,
    pub ownership_challenge_version: i64,
}

#[derive(Debug, Clone)]
pub struct RecordMailDomainDnsCommand {
    pub organization_id: OrganizationId,
    pub mail_domain_id: MailDomainId,
    pub observed_domain: MailDomainName,
    pub evidence: DnsVerificationEvidence,
    pub raw_answers: Value,
    pub failure_codes: Vec<String>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ActivateMailDomainCommand {
    pub organization_id: OrganizationId,
    pub mail_domain_id: MailDomainId,
    /// Server-selected data plane. API callers must never supply this value.
    pub data_plane_id: MailDataPlaneId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SuspendMailDomainCommand {
    pub organization_id: OrganizationId,
    pub mail_domain_id: MailDomainId,
    pub reason_code: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpdateMailDomainRouteCommand {
    pub organization_id: OrganizationId,
    pub mail_domain_id: MailDomainId,
    pub expected_version: i64,
    pub inbound_enabled: bool,
    pub outbound_enabled: bool,
    pub smart_host_ref: Option<String>,
    pub dkim_key_ref: Option<String>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateMailDomainRouteResult {
    pub mail_domain_id: MailDomainId,
    pub version: i64,
    pub inbound_enabled: bool,
    pub outbound_enabled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MailDomainGovernanceError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("mail domain operation is forbidden")]
    Forbidden,
    #[error("mail domain does not exist")]
    NotFound,
    #[error("mail domain state changed concurrently")]
    Conflict,
    #[error("fresh, complete DNS evidence is required")]
    DnsEvidenceRequired,
    #[error("ownership challenge rotation requires a non-active, non-retired domain")]
    ChallengeRotationNotAllowed,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct MailDomainGovernanceService<S> {
    store: S,
}

impl<S: ControlPlaneStore> MailDomainGovernanceService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn register(
        &self,
        context: &AuthorizationContext,
        command: RegisterMailDomainCommand,
    ) -> Result<RegisterMailDomainResult, MailDomainGovernanceError> {
        let mut transaction = self.store.begin().await?;
        authorize(&mut *transaction, context, command.organization_id).await?;
        let name = MailDomainName::parse(&command.name)?;
        let mut domain = MailDomain::new(
            command.organization_id,
            name,
            command.route_mode,
            command.ownership_challenge_sha256,
            command.now,
        )?;
        domain.begin_dns_verification()?;
        let route = MailDomainRoute::disabled(domain.id, command.organization_id, command.now);
        let audit = audit(
            command.organization_id,
            context.principal_id,
            "mail_domain.registered",
            domain.id,
            command.trace_id,
            json!({
                "name": domain.name,
                "route_mode": domain.route_mode,
                "status": domain.status,
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "mail_domain.registered.v1",
            "mail_domain",
            domain.id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mail_domain_id": domain.id,
                "name": domain.name,
                "route_mode": domain.route_mode,
                "status": domain.status,
            }),
            command.now,
        );
        transaction
            .insert_mail_domain(&domain, context.principal_id)
            .await?;
        transaction
            .insert_mail_domain_route(&route, context.principal_id)
            .await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(RegisterMailDomainResult {
            mail_domain_id: domain.id,
            name: domain.name.to_string(),
            status: domain.status,
            ownership_challenge_version: domain.ownership_challenge_version,
            route_version: route.version,
        })
    }

    pub async fn rotate_ownership_challenge(
        &self,
        context: &AuthorizationContext,
        command: RotateMailDomainChallengeCommand,
    ) -> Result<RotateMailDomainChallengeResult, MailDomainGovernanceError> {
        let mut transaction = self.store.begin().await?;
        authorize(&mut *transaction, context, command.organization_id).await?;
        let mut domain = transaction
            .find_mail_domain_for_update(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::NotFound)?;
        if matches!(
            domain.status,
            MailDomainStatus::Active | MailDomainStatus::Retired
        ) {
            return Err(MailDomainGovernanceError::ChallengeRotationNotAllowed);
        }
        if domain.ownership_challenge_version != command.expected_version {
            return Err(MailDomainGovernanceError::Conflict);
        }
        let previous_version = domain.ownership_challenge_version;
        domain.rotate_ownership_challenge(command.ownership_challenge_sha256)?;
        let audit = audit(
            command.organization_id,
            context.principal_id,
            "mail_domain.challenge_rotated",
            command.mail_domain_id,
            command.trace_id,
            json!({
                "previous_challenge_version": previous_version,
                "ownership_challenge_version": domain.ownership_challenge_version,
                "status": domain.status,
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "mail_domain.challenge_rotated.v1",
            "mail_domain",
            command.mail_domain_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mail_domain_id": command.mail_domain_id,
                "ownership_challenge_version": domain.ownership_challenge_version,
                "status": domain.status,
            }),
            command.now,
        );
        if !transaction.update_mail_domain(&domain, command.now).await? {
            return Err(MailDomainGovernanceError::Conflict);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(RotateMailDomainChallengeResult {
            mail_domain_id: domain.id,
            name: domain.name.to_string(),
            status: domain.status,
            ownership_challenge_version: domain.ownership_challenge_version,
        })
    }

    pub async fn record_dns(
        &self,
        context: &AuthorizationContext,
        command: RecordMailDomainDnsCommand,
    ) -> Result<MailDomainStatus, MailDomainGovernanceError> {
        validate_observation_payload(&command)?;
        let mut transaction = self.store.begin().await?;
        authorize(&mut *transaction, context, command.organization_id).await?;
        let mut domain = transaction
            .find_mail_domain_for_update(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::NotFound)?;
        if domain.name != command.observed_domain
            || domain.ownership_challenge_sha256 != command.evidence.ownership_challenge_sha256
        {
            return Err(DomainError::InvalidDnsVerificationEvidence.into());
        }
        let previous_status = domain.status;
        let transitioned_to_verified = match domain.status {
            MailDomainStatus::PendingDns => {
                domain.record_dns_evidence(&command.evidence, command.now)?
            }
            MailDomainStatus::Suspended => {
                domain.begin_dns_verification()?;
                domain.record_dns_evidence(&command.evidence, command.now)?
            }
            MailDomainStatus::Verified | MailDomainStatus::Active => false,
            MailDomainStatus::Draft | MailDomainStatus::Retired => {
                return Err(DomainError::InvalidMailDomainTransition.into());
            }
        };
        let observation = MailDomainDnsObservation {
            organization_id: command.organization_id,
            mail_domain_id: command.mail_domain_id,
            evidence: command.evidence.clone(),
            raw_answers: command.raw_answers,
            failure_codes: command.failure_codes.clone(),
            created_at: command.now,
        };
        let action = if transitioned_to_verified {
            "mail_domain.verified"
        } else {
            "mail_domain.dns_observed"
        };
        let audit = audit(
            command.organization_id,
            context.principal_id,
            action,
            command.mail_domain_id,
            command.trace_id,
            json!({
                "previous_status": previous_status,
                "status": domain.status,
                "activation_ready": command.evidence.activation_ready(command.now),
                "valid_until": command.evidence.valid_until,
                "failure_codes": command.failure_codes,
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            if transitioned_to_verified {
                "mail_domain.verified.v1"
            } else {
                "mail_domain.dns_observed.v1"
            },
            "mail_domain",
            command.mail_domain_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mail_domain_id": command.mail_domain_id,
                "status": domain.status,
                "activation_ready": command.evidence.activation_ready(command.now),
                "valid_until": command.evidence.valid_until,
                "failure_codes": command.failure_codes,
            }),
            command.now,
        );
        transaction
            .insert_mail_domain_dns_observation(&observation)
            .await?;
        if domain.status != previous_status
            && !transaction.update_mail_domain(&domain, command.now).await?
        {
            return Err(MailDomainGovernanceError::Conflict);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(domain.status)
    }

    pub async fn activate(
        &self,
        context: &AuthorizationContext,
        command: ActivateMailDomainCommand,
    ) -> Result<(), MailDomainGovernanceError> {
        let mut transaction = self.store.begin().await?;
        authorize(&mut *transaction, context, command.organization_id).await?;
        let mut domain = transaction
            .find_mail_domain_for_update(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::NotFound)?;
        if !transaction
            .latest_mail_domain_dns_is_activation_ready(
                command.organization_id,
                command.mail_domain_id,
                &domain.ownership_challenge_sha256,
                command.now,
            )
            .await?
        {
            return Err(MailDomainGovernanceError::DnsEvidenceRequired);
        }
        domain.activate(command.now)?;
        let audit = audit(
            command.organization_id,
            context.principal_id,
            "mail_domain.activated",
            command.mail_domain_id,
            command.trace_id,
            json!({"status": domain.status, "activated_at": domain.activated_at}),
            command.now,
        )?;
        let event = PendingEvent::for_mail_data_plane(
            command.data_plane_id,
            "mail_domain.activated.v1",
            "mail_domain",
            command.mail_domain_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mail_domain_id": command.mail_domain_id,
                "status": domain.status,
            }),
            command.now,
        );
        if !transaction
            .assign_mail_domain_data_plane(
                command.organization_id,
                command.mail_domain_id,
                command.data_plane_id,
                command.now,
            )
            .await?
            || !transaction.update_mail_domain(&domain, command.now).await?
        {
            return Err(MailDomainGovernanceError::Conflict);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn suspend(
        &self,
        context: &AuthorizationContext,
        command: SuspendMailDomainCommand,
    ) -> Result<(), MailDomainGovernanceError> {
        validate_reason_code(&command.reason_code)?;
        let mut transaction = self.store.begin().await?;
        authorize(&mut *transaction, context, command.organization_id).await?;
        let mut domain = transaction
            .find_mail_domain_for_update(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::NotFound)?;
        let data_plane_id = transaction
            .assigned_mail_domain_data_plane(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::Conflict)?;
        let mut route = transaction
            .find_mail_domain_route_for_update(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::NotFound)?;
        let expected_route_version = route.version;
        domain.suspend(command.now)?;
        route.update(
            domain.status,
            domain.route_mode,
            false,
            false,
            route.smart_host_ref.clone(),
            route.dkim_key_ref.clone(),
            command.now,
        )?;
        let audit = audit(
            command.organization_id,
            context.principal_id,
            "mail_domain.suspended",
            command.mail_domain_id,
            command.trace_id,
            json!({
                "status": domain.status,
                "reason_code": command.reason_code,
                "route_version": route.version,
                "routes_disabled": true,
            }),
            command.now,
        )?;
        let event = PendingEvent::for_mail_data_plane(
            data_plane_id,
            "mail_domain.suspended.v1",
            "mail_domain",
            command.mail_domain_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mail_domain_id": command.mail_domain_id,
                "status": domain.status,
                "reason_code": command.reason_code,
                "route_version": route.version,
            }),
            command.now,
        );
        if !transaction.update_mail_domain(&domain, command.now).await?
            || !transaction
                .update_mail_domain_route(&route, expected_route_version, context.principal_id)
                .await?
        {
            return Err(MailDomainGovernanceError::Conflict);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn update_route(
        &self,
        context: &AuthorizationContext,
        command: UpdateMailDomainRouteCommand,
    ) -> Result<UpdateMailDomainRouteResult, MailDomainGovernanceError> {
        let mut transaction = self.store.begin().await?;
        authorize(&mut *transaction, context, command.organization_id).await?;
        let domain = transaction
            .find_mail_domain_for_update(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::NotFound)?;
        let mut route = transaction
            .find_mail_domain_route_for_update(command.organization_id, command.mail_domain_id)
            .await?
            .ok_or(MailDomainGovernanceError::NotFound)?;
        if route.version != command.expected_version {
            return Err(MailDomainGovernanceError::Conflict);
        }
        route.update(
            domain.status,
            domain.route_mode,
            command.inbound_enabled,
            command.outbound_enabled,
            command.smart_host_ref,
            command.dkim_key_ref,
            command.now,
        )?;
        let audit = audit(
            command.organization_id,
            context.principal_id,
            "mail_domain.route_updated",
            command.mail_domain_id,
            command.trace_id,
            json!({
                "previous_version": command.expected_version,
                "version": route.version,
                "inbound_enabled": route.inbound_enabled,
                "outbound_enabled": route.outbound_enabled,
                "route_mode": domain.route_mode,
                "smart_host_configured": route.smart_host_ref.is_some(),
                "dkim_key_configured": route.dkim_key_ref.is_some(),
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "mail_domain.route_updated.v1",
            "mail_domain",
            command.mail_domain_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mail_domain_id": command.mail_domain_id,
                "version": route.version,
                "inbound_enabled": route.inbound_enabled,
                "outbound_enabled": route.outbound_enabled,
                "route_mode": domain.route_mode,
            }),
            command.now,
        );
        if !transaction
            .update_mail_domain_route(&route, command.expected_version, context.principal_id)
            .await?
        {
            return Err(MailDomainGovernanceError::Conflict);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(UpdateMailDomainRouteResult {
            mail_domain_id: command.mail_domain_id,
            version: route.version,
            inbound_enabled: route.inbound_enabled,
            outbound_enabled: route.outbound_enabled,
        })
    }
}

async fn authorize(
    transaction: &mut dyn ControlPlaneTransaction,
    context: &AuthorizationContext,
    organization_id: OrganizationId,
) -> Result<(), MailDomainGovernanceError> {
    if !context.permits(organization_id, Permission::MailAdmin)
        || !transaction
            .active_principal_exists(organization_id, context.principal_id)
            .await?
    {
        return Err(MailDomainGovernanceError::Forbidden);
    }
    Ok(())
}

fn validate_observation_payload(
    command: &RecordMailDomainDnsCommand,
) -> Result<(), MailDomainGovernanceError> {
    command.evidence.validate()?;
    if command.now < command.evidence.observed_at
        || !command.raw_answers.is_object()
        || serde_json::to_vec(&command.raw_answers).map_or(true, |value| value.len() > 64 * 1024)
        || command.failure_codes.len() > 32
        || command.failure_codes.iter().any(|code| {
            code.is_empty()
                || code.len() > 100
                || !code.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
                })
        })
    {
        return Err(DomainError::InvalidDnsVerificationEvidence.into());
    }
    Ok(())
}

fn validate_reason_code(value: &str) -> Result<(), MailDomainGovernanceError> {
    if value.is_empty()
        || value.len() > 100
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
    {
        return Err(DomainError::InvalidMailDomainTransition.into());
    }
    Ok(())
}

fn audit(
    organization_id: OrganizationId,
    actor: PrincipalId,
    action: &'static str,
    mail_domain_id: MailDomainId,
    trace_id: String,
    details: Value,
    now: DateTime<Utc>,
) -> Result<AuditEvent, DomainError> {
    AuditEvent::new(
        organization_id,
        AuditActor {
            principal_id: actor,
            on_behalf_of: None,
            credential_id: None,
        },
        action,
        AuditResource {
            kind: "mail_domain".into(),
            id: mail_domain_id.to_string(),
        },
        AuditOutcome::Success,
        trace_id,
        details,
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use copilotmail_domain::{
        DnsVerificationChecks, Organization, OrganizationSlug, Role, RoleAssignment,
    };
    use std::{
        collections::BTreeSet,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct MemoryStore {
        state: Arc<Mutex<MemoryState>>,
    }

    #[derive(Clone, Default)]
    struct MemoryState {
        domain: Option<MailDomain>,
        route: Option<MailDomainRoute>,
        assigned_data_plane_id: Option<MailDataPlaneId>,
        activation_ready: bool,
        actions: Vec<String>,
        event_routes: Vec<Option<MailDataPlaneId>>,
    }

    struct MemoryTransaction {
        shared: Arc<Mutex<MemoryState>>,
        working: MemoryState,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            let working = self.state.lock().unwrap().clone();
            Ok(Box::new(MemoryTransaction {
                shared: self.state.clone(),
                working,
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for MemoryTransaction {
        async fn find_organization_bootstrap(
            &mut self,
            _slug: &OrganizationSlug,
        ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
            Ok(None)
        }

        async fn insert_organization(
            &mut self,
            _organization: &Organization,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_principal(
            &mut self,
            _principal: &copilotmail_domain::Principal,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role(&mut self, _role: &Role) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role_assignment(
            &mut self,
            _assignment: &RoleAssignment,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn record_organization_bootstrap(
            &mut self,
            _organization_id: OrganizationId,
            _root_principal_id: PrincipalId,
            _created_at: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn active_principal_exists(
            &mut self,
            _organization_id: OrganizationId,
            _principal_id: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(true)
        }

        async fn insert_mail_domain(
            &mut self,
            domain: &MailDomain,
            _created_by: PrincipalId,
        ) -> Result<(), StoreError> {
            self.working.domain = Some(domain.clone());
            self.working.actions.push("domain".into());
            Ok(())
        }

        async fn insert_mail_domain_route(
            &mut self,
            route: &MailDomainRoute,
            _updated_by: PrincipalId,
        ) -> Result<(), StoreError> {
            self.working.route = Some(route.clone());
            self.working.actions.push("route".into());
            Ok(())
        }

        async fn find_mail_domain_for_update(
            &mut self,
            _organization_id: OrganizationId,
            _mail_domain_id: MailDomainId,
        ) -> Result<Option<MailDomain>, StoreError> {
            Ok(self.working.domain.clone())
        }

        async fn update_mail_domain(
            &mut self,
            domain: &MailDomain,
            _updated_at: DateTime<Utc>,
        ) -> Result<bool, StoreError> {
            self.working.domain = Some(domain.clone());
            self.working.actions.push("domain_updated".into());
            Ok(true)
        }

        async fn insert_mail_domain_dns_observation(
            &mut self,
            _observation: &MailDomainDnsObservation,
        ) -> Result<(), StoreError> {
            self.working.actions.push("dns".into());
            Ok(())
        }

        async fn latest_mail_domain_dns_is_activation_ready(
            &mut self,
            _organization_id: OrganizationId,
            _mail_domain_id: MailDomainId,
            _ownership_challenge_sha256: &[u8; 32],
            _now: DateTime<Utc>,
        ) -> Result<bool, StoreError> {
            Ok(self.working.activation_ready)
        }

        async fn assign_mail_domain_data_plane(
            &mut self,
            _organization_id: OrganizationId,
            _mail_domain_id: MailDomainId,
            data_plane_id: MailDataPlaneId,
            _assigned_at: DateTime<Utc>,
        ) -> Result<bool, StoreError> {
            match self.working.assigned_data_plane_id {
                Some(existing) => Ok(existing == data_plane_id),
                None => {
                    self.working.assigned_data_plane_id = Some(data_plane_id);
                    self.working.actions.push("data_plane_assigned".into());
                    Ok(true)
                }
            }
        }

        async fn assigned_mail_domain_data_plane(
            &mut self,
            _organization_id: OrganizationId,
            _mail_domain_id: MailDomainId,
        ) -> Result<Option<MailDataPlaneId>, StoreError> {
            Ok(self.working.assigned_data_plane_id)
        }

        async fn find_mail_domain_route_for_update(
            &mut self,
            _organization_id: OrganizationId,
            _mail_domain_id: MailDomainId,
        ) -> Result<Option<MailDomainRoute>, StoreError> {
            Ok(self.working.route.clone())
        }

        async fn update_mail_domain_route(
            &mut self,
            route: &MailDomainRoute,
            expected_version: i64,
            _updated_by: PrincipalId,
        ) -> Result<bool, StoreError> {
            if self
                .working
                .route
                .as_ref()
                .is_none_or(|current| current.version != expected_version)
            {
                return Ok(false);
            }
            self.working.route = Some(route.clone());
            self.working.actions.push("route_updated".into());
            Ok(true)
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            self.working.actions.push(format!("audit:{}", event.action));
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.working.event_routes.push(event.data_plane_id);
            self.working
                .actions
                .push(format!("outbox:{}", event.event_type));
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            let mut working = self.working;
            working.actions.push("commit".into());
            *self.shared.lock().unwrap() = working;
            Ok(())
        }
    }

    fn context(organization_id: OrganizationId, principal_id: PrincipalId) -> AuthorizationContext {
        AuthorizationContext {
            principal_id,
            organization_id,
            permissions: BTreeSet::from([Permission::MailAdmin]),
        }
    }

    fn evidence(now: DateTime<Utc>, ready: bool) -> DnsVerificationEvidence {
        DnsVerificationEvidence::new(
            [7; 32],
            now,
            now + chrono::Duration::minutes(10),
            DnsVerificationChecks {
                ownership_txt: ready,
                mx_present: ready,
                mx_hosts_public: ready,
                spf_present: ready,
                dkim_present: ready,
                dmarc_present: ready,
            },
        )
        .unwrap()
    }

    fn active_state(now: DateTime<Utc>) -> MemoryState {
        let organization_id = OrganizationId::new();
        let mut domain = MailDomain::new(
            organization_id,
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        domain
            .record_dns_evidence(&evidence(now, true), now)
            .unwrap();
        domain.activate(now).unwrap();
        let mut route = MailDomainRoute::disabled(domain.id, organization_id, now);
        route
            .update(
                domain.status,
                domain.route_mode,
                true,
                true,
                None,
                Some("kms://dkim/example".into()),
                now,
            )
            .unwrap();
        MemoryState {
            domain: Some(domain),
            route: Some(route),
            assigned_data_plane_id: Some(crate::mail_ports::test_mail_data_plane_id()),
            activation_ready: true,
            actions: vec![],
            event_routes: vec![],
        }
    }

    #[tokio::test]
    async fn register_is_atomic_and_routes_start_disabled() {
        let store = MemoryStore::default();
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let result = MailDomainGovernanceService::new(store.clone())
            .register(
                &context(organization_id, principal_id),
                RegisterMailDomainCommand {
                    organization_id,
                    name: "例子.example".into(),
                    route_mode: MailDomainRouteMode::DirectMx,
                    ownership_challenge_sha256: [7; 32],
                    trace_id: "trace-register".into(),
                    now,
                },
            )
            .await
            .unwrap();

        let state = store.state.lock().unwrap();
        assert_eq!(result.status, MailDomainStatus::PendingDns);
        assert!(!state.route.as_ref().unwrap().inbound_enabled);
        assert!(!state.route.as_ref().unwrap().outbound_enabled);
        assert_eq!(
            state.actions,
            [
                "domain",
                "route",
                "audit:mail_domain.registered",
                "outbox:mail_domain.registered.v1",
                "commit",
            ]
        );
        assert_eq!(state.event_routes, [None]);
    }

    #[tokio::test]
    async fn activation_requires_latest_fresh_complete_evidence() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mut domain = MailDomain::new(
            organization_id,
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        domain
            .record_dns_evidence(&evidence(now, true), now)
            .unwrap();
        let store = MemoryStore::default();
        store.state.lock().unwrap().domain = Some(domain.clone());

        let error = MailDomainGovernanceService::new(store.clone())
            .activate(
                &context(organization_id, principal_id),
                ActivateMailDomainCommand {
                    organization_id,
                    mail_domain_id: domain.id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    trace_id: "trace-activate".into(),
                    now,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            MailDomainGovernanceError::DnsEvidenceRequired
        ));
        assert!(store.state.lock().unwrap().actions.is_empty());
    }

    #[tokio::test]
    async fn activation_assigns_and_routes_the_domain_in_the_same_commit() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let data_plane_id = crate::mail_ports::test_mail_data_plane_id();
        let mut domain = MailDomain::new(
            organization_id,
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        domain
            .record_dns_evidence(&evidence(now, true), now)
            .unwrap();
        let store = MemoryStore::default();
        {
            let mut state = store.state.lock().unwrap();
            state.domain = Some(domain.clone());
            state.activation_ready = true;
        }

        MailDomainGovernanceService::new(store.clone())
            .activate(
                &context(organization_id, principal_id),
                ActivateMailDomainCommand {
                    organization_id,
                    mail_domain_id: domain.id,
                    data_plane_id,
                    trace_id: "trace-activate-routed".into(),
                    now,
                },
            )
            .await
            .unwrap();

        let state = store.state.lock().unwrap();
        assert_eq!(state.assigned_data_plane_id, Some(data_plane_id));
        assert_eq!(
            state.domain.as_ref().map(|domain| domain.status),
            Some(MailDomainStatus::Active)
        );
        assert_eq!(state.event_routes, [Some(data_plane_id)]);
        assert_eq!(
            state.actions,
            [
                "data_plane_assigned",
                "domain_updated",
                "audit:mail_domain.activated",
                "outbox:mail_domain.activated.v1",
                "commit",
            ]
        );
    }

    #[tokio::test]
    async fn challenge_rotation_is_atomic_and_invalidates_the_previous_digest() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mut domain = MailDomain::new(
            organization_id,
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        let store = MemoryStore::default();
        store.state.lock().unwrap().domain = Some(domain.clone());

        let result = MailDomainGovernanceService::new(store.clone())
            .rotate_ownership_challenge(
                &context(organization_id, principal_id),
                RotateMailDomainChallengeCommand {
                    organization_id,
                    mail_domain_id: domain.id,
                    expected_version: 1,
                    ownership_challenge_sha256: [8; 32],
                    trace_id: "trace-rotate-challenge".into(),
                    now,
                },
            )
            .await
            .unwrap();

        let state = store.state.lock().unwrap();
        let persisted = state.domain.as_ref().unwrap();
        assert_eq!(result.ownership_challenge_version, 2);
        assert_eq!(persisted.ownership_challenge_sha256, [8; 32]);
        assert_eq!(persisted.ownership_challenge_version, 2);
        assert_eq!(
            state.actions,
            [
                "domain_updated",
                "audit:mail_domain.challenge_rotated",
                "outbox:mail_domain.challenge_rotated.v1",
                "commit",
            ]
        );
    }

    #[tokio::test]
    async fn active_domain_challenge_rotation_is_rejected_without_side_effects() {
        let now = Utc::now();
        let state = active_state(now);
        let domain = state.domain.clone().unwrap();
        let store = MemoryStore {
            state: Arc::new(Mutex::new(state)),
        };

        let error = MailDomainGovernanceService::new(store.clone())
            .rotate_ownership_challenge(
                &context(domain.organization_id, PrincipalId::new()),
                RotateMailDomainChallengeCommand {
                    organization_id: domain.organization_id,
                    mail_domain_id: domain.id,
                    expected_version: 1,
                    ownership_challenge_sha256: [8; 32],
                    trace_id: "trace-active-rotate".into(),
                    now,
                },
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            MailDomainGovernanceError::ChallengeRotationNotAllowed
        ));
        assert!(store.state.lock().unwrap().actions.is_empty());
    }

    #[tokio::test]
    async fn dns_evidence_is_bound_to_the_registered_domain_name() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mut domain = MailDomain::new(
            organization_id,
            MailDomainName::parse("target.example").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        let store = MemoryStore::default();
        store.state.lock().unwrap().domain = Some(domain.clone());

        let error = MailDomainGovernanceService::new(store.clone())
            .record_dns(
                &context(organization_id, principal_id),
                RecordMailDomainDnsCommand {
                    organization_id,
                    mail_domain_id: domain.id,
                    observed_domain: MailDomainName::parse("other.example").unwrap(),
                    evidence: evidence(now, true),
                    raw_answers: json!({}),
                    failure_codes: vec![],
                    trace_id: "trace-wrong-domain".into(),
                    now,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, MailDomainGovernanceError::Domain(_)));
        assert!(store.state.lock().unwrap().actions.is_empty());
    }

    #[tokio::test]
    async fn stale_route_version_cannot_overwrite_current_configuration() {
        let now = Utc::now();
        let state = active_state(now);
        let domain = state.domain.clone().unwrap();
        let current_version = state.route.as_ref().unwrap().version;
        let store = MemoryStore {
            state: Arc::new(Mutex::new(state)),
        };
        let error = MailDomainGovernanceService::new(store.clone())
            .update_route(
                &context(domain.organization_id, PrincipalId::new()),
                UpdateMailDomainRouteCommand {
                    organization_id: domain.organization_id,
                    mail_domain_id: domain.id,
                    expected_version: current_version - 1,
                    inbound_enabled: false,
                    outbound_enabled: false,
                    smart_host_ref: None,
                    dkim_key_ref: None,
                    trace_id: "trace-stale".into(),
                    now,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, MailDomainGovernanceError::Conflict));
        assert!(store.state.lock().unwrap().actions.is_empty());
    }

    #[tokio::test]
    async fn suspension_disables_both_routes_in_the_same_commit() {
        let now = Utc::now();
        let state = active_state(now);
        let domain = state.domain.clone().unwrap();
        let store = MemoryStore {
            state: Arc::new(Mutex::new(state)),
        };
        MailDomainGovernanceService::new(store.clone())
            .suspend(
                &context(domain.organization_id, PrincipalId::new()),
                SuspendMailDomainCommand {
                    organization_id: domain.organization_id,
                    mail_domain_id: domain.id,
                    reason_code: "security_incident".into(),
                    trace_id: "trace-suspend".into(),
                    now,
                },
            )
            .await
            .unwrap();

        let state = store.state.lock().unwrap();
        assert_eq!(
            state.domain.as_ref().unwrap().status,
            MailDomainStatus::Suspended
        );
        assert!(!state.route.as_ref().unwrap().inbound_enabled);
        assert!(!state.route.as_ref().unwrap().outbound_enabled);
        assert_eq!(state.actions.last().map(String::as_str), Some("commit"));
        assert!(state.actions.contains(&"domain_updated".into()));
        assert!(state.actions.contains(&"route_updated".into()));
        assert_eq!(
            state.event_routes,
            [Some(crate::mail_ports::test_mail_data_plane_id())]
        );
    }

    #[tokio::test]
    async fn suspension_without_a_durable_data_plane_assignment_fails_closed() {
        let now = Utc::now();
        let mut state = active_state(now);
        state.assigned_data_plane_id = None;
        let domain = state.domain.clone().unwrap();
        let store = MemoryStore {
            state: Arc::new(Mutex::new(state)),
        };

        let error = MailDomainGovernanceService::new(store.clone())
            .suspend(
                &context(domain.organization_id, PrincipalId::new()),
                SuspendMailDomainCommand {
                    organization_id: domain.organization_id,
                    mail_domain_id: domain.id,
                    reason_code: "security_incident".into(),
                    trace_id: "trace-unassigned-suspend".into(),
                    now,
                },
            )
            .await
            .unwrap_err();

        assert!(matches!(error, MailDomainGovernanceError::Conflict));
        let state = store.state.lock().unwrap();
        assert_eq!(
            state.domain.as_ref().unwrap().status,
            MailDomainStatus::Active
        );
        assert!(state.actions.is_empty());
        assert!(state.event_routes.is_empty());
    }
}
