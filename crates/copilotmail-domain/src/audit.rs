use crate::{
    ApprovalRequestId, AuditEventId, DomainError, OrganizationId, PolicyId, PolicyVersionId,
    PrincipalId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditRisk {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEventDefinition {
    pub action: &'static str,
    pub resource_kind: &'static str,
    pub risk: AuditRisk,
    pub privileged: bool,
}

macro_rules! audit_events {
    ($(($action:literal, $resource:literal, $risk:ident, $privileged:literal)),+ $(,)?) => {
        pub const AUDIT_EVENT_CATALOG: &[AuditEventDefinition] = &[
            $(AuditEventDefinition {
                action: $action,
                resource_kind: $resource,
                risk: AuditRisk::$risk,
                privileged: $privileged,
            }),+
        ];
    };
}

audit_events!(
    (
        "organization.bootstrapped",
        "organization.tenant",
        Critical,
        true
    ),
    (
        "identity.principal.created",
        "identity.principal",
        High,
        true
    ),
    (
        "identity.oidc_owner.bound",
        "identity.principal",
        Critical,
        true
    ),
    ("identity.agent.registered", "identity.agent", High, true),
    (
        "identity.agent_credential.added",
        "identity.agent_credential",
        High,
        true
    ),
    (
        "identity.agent_mtls_credential.added",
        "identity.agent_credential",
        High,
        true
    ),
    (
        "identity.agent_credential.revoked",
        "identity.agent_credential",
        High,
        true
    ),
    (
        "identity.scim_token.issued",
        "identity.scim_token",
        High,
        true
    ),
    (
        "identity.scim_token.revoked",
        "identity.scim_token",
        High,
        true
    ),
    (
        "identity.human_session.created",
        "identity.human_session",
        High,
        true
    ),
    (
        "identity.human_session.revoked",
        "identity.human_session",
        High,
        true
    ),
    (
        "identity.scim_user.provisioned",
        "identity.scim_user",
        Medium,
        true
    ),
    (
        "identity.scim_user.adopted",
        "identity.scim_user",
        Medium,
        true
    ),
    (
        "identity.scim_user.updated",
        "identity.scim_user",
        High,
        true
    ),
    (
        "identity.scim_group.provisioned",
        "identity.scim_group",
        Medium,
        true
    ),
    (
        "identity.scim_group.updated",
        "identity.scim_group",
        Medium,
        true
    ),
    (
        "identity.scim_group.deleted",
        "identity.scim_group",
        High,
        true
    ),
    (
        "authorization.approval_policy.created",
        "authorization.approval_policy",
        High,
        true
    ),
    (
        "authorization.approval_policy.versioned",
        "authorization.approval_policy",
        High,
        true
    ),
    (
        "authorization.approval.requested",
        "authorization.approval_request",
        Medium,
        true
    ),
    (
        "authorization.approval.decided",
        "authorization.approval_request",
        High,
        true
    ),
    (
        "authorization.approval.cancelled",
        "authorization.approval_request",
        High,
        true
    ),
    (
        "authorization.approval.queried",
        "authorization.approval_request_query",
        High,
        true
    ),
    (
        "authorization.role_assignment.granted",
        "authorization.role_assignment",
        Critical,
        true
    ),
    (
        "authorization.role_assignment.revoked",
        "authorization.role_assignment",
        Critical,
        true
    ),
    (
        "authorization.role_assignment.queried",
        "authorization.role_assignment_query",
        High,
        true
    ),
    ("mailbox.created", "mailbox", Medium, true),
    ("mail.messages.queried", "mailbox", Medium, true),
    ("mail.message.read", "mail.message", High, true),
    ("mail.drafts.queried", "mailbox", Medium, true),
    ("mail.draft.read", "mail.draft", High, true),
    ("mail.submissions.queried", "mailbox", Medium, true),
    ("agent.mail_content.read", "mail.message", High, true),
    (
        "agent.mail_policy.activated",
        "agent.mail_policy",
        Critical,
        true
    ),
    (
        "agent.mail_policy.versioned",
        "agent.mail_policy",
        High,
        true
    ),
    (
        "agent.mail_policy.queried",
        "agent.mail_policy_query",
        High,
        true
    ),
    (
        "agent.processing_subscription.created",
        "agent.processing_subscription",
        Medium,
        true
    ),
    (
        "agent.message.claimed",
        "agent.message_processing",
        Low,
        true
    ),
    (
        "agent.message.lease_extended",
        "agent.message_processing",
        Low,
        true
    ),
    (
        "agent.message.acknowledged",
        "agent.message_processing",
        Low,
        true
    ),
    (
        "agent.message.nacked",
        "agent.message_processing",
        Medium,
        true
    ),
    (
        "agent.message.dead_lettered",
        "agent.message_processing",
        Medium,
        true
    ),
    (
        "agent.message_dead_letters.queried",
        "agent.message_dead_letter_query",
        Medium,
        true
    ),
    (
        "agent.processing_subscription.paused",
        "agent.processing_subscription",
        Medium,
        true
    ),
    (
        "agent.processing_subscription.resumed",
        "agent.processing_subscription",
        Medium,
        true
    ),
    (
        "agent.processing_subscription.retired",
        "agent.processing_subscription",
        High,
        true
    ),
    (
        "agent.mail.admitted",
        "agent.mail_admission",
        Critical,
        true
    ),
    (
        "agent.mail_sandbox.captured",
        "agent.mail_sandbox_submission",
        High,
        true
    ),
    ("mail.submission.requested", "mail.submission", High, true),
    ("mail.submission.accepted", "mail.submission", High, true),
    ("mail.submission.rejected", "mail.submission", High, true),
    (
        "mail.submission.review_required",
        "mail.submission",
        Critical,
        true
    ),
    (
        "mail_engine.mailbox_binding.updated",
        "mail_engine.mailbox_binding",
        High,
        true
    ),
    (
        "authorization.mailbox_delegation.granted",
        "authorization.mailbox_delegation",
        High,
        true
    ),
    (
        "authorization.mailbox_delegation.revoked",
        "authorization.mailbox_delegation",
        High,
        true
    ),
    ("mail_domain.registered", "mail_domain", Medium, true),
    ("mail_domain.challenge_rotated", "mail_domain", High, true),
    ("mail_domain.dns_observed", "mail_domain", Low, true),
    ("mail_domain.verified", "mail_domain", High, true),
    ("mail_domain.activated", "mail_domain", Critical, true),
    ("mail_domain.suspended", "mail_domain", Critical, true),
    ("mail_domain.route_updated", "mail_domain", Critical, true),
    (
        "agent.webhook_subscription.created",
        "agent.webhook_subscription",
        High,
        true
    ),
    (
        "agent.webhook_subscription.updated",
        "agent.webhook_subscription",
        High,
        true
    ),
    (
        "agent.webhook_subscription.disabled",
        "agent.webhook_subscription",
        High,
        true
    ),
    (
        "agent.webhook_delivery.replayed",
        "agent.webhook_delivery",
        Critical,
        true
    ),
    (
        "agent.event_cursor.read",
        "agent.event_cursor",
        Medium,
        true
    ),
    (
        "agent.event_stream.opened",
        "agent.event_stream",
        Medium,
        true
    ),
    ("audit.events.queried", "audit.query", High, true),
    (
        "audit.checkpoint.created",
        "audit.checkpoint",
        Critical,
        true
    ),
    ("audit.archive.completed", "audit.archive", Critical, true),
    (
        "audit.export.authorized",
        "audit.export_grant",
        Critical,
        true
    ),
    (
        "audit.export.downloaded",
        "audit.export_grant",
        Critical,
        true
    ),
    (
        "compliance.retention_policy.created",
        "compliance.retention_policy",
        High,
        true
    ),
    (
        "compliance.retention_policy.versioned",
        "compliance.retention_policy",
        High,
        true
    ),
    (
        "compliance.retention_policy.activated",
        "compliance.retention_policy",
        Critical,
        true
    ),
    (
        "compliance.retention_policy.queried",
        "compliance.retention_policy_query",
        High,
        true
    ),
    ("compliance.case.created", "compliance.case", High, true),
    ("compliance.case.closed", "compliance.case", High, true),
    (
        "compliance.case.queried",
        "compliance.case_query",
        High,
        true
    ),
    (
        "compliance.legal_hold.applied",
        "compliance.legal_hold",
        Critical,
        true
    ),
    (
        "compliance.legal_hold.released",
        "compliance.legal_hold",
        Critical,
        true
    ),
    (
        "compliance.legal_hold.queried",
        "compliance.legal_hold_query",
        High,
        true
    ),
);

#[must_use]
pub fn audit_event_definition(action: &str) -> Option<&'static AuditEventDefinition> {
    AUDIT_EVENT_CATALOG
        .iter()
        .find(|definition| definition.action == action)
}

/// Request and governance correlation fields kept out of free-form details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditContext {
    pub request_id: String,
    pub subject_principal_id: Option<PrincipalId>,
    pub source_ip: Option<String>,
    pub client_id: Option<String>,
    pub session_id: Option<String>,
    pub token_jti: Option<String>,
    pub policy_id: Option<PolicyId>,
    pub policy_version_id: Option<PolicyVersionId>,
    pub approval_request_id: Option<ApprovalRequestId>,
    pub ticket_reference: Option<String>,
    pub environment: Option<String>,
    pub region: Option<String>,
}

impl AuditContext {
    pub fn for_request(request_id: impl Into<String>) -> Result<Self, DomainError> {
        let context = Self {
            request_id: request_id.into(),
            subject_principal_id: None,
            source_ip: None,
            client_id: None,
            session_id: None,
            token_jti: None,
            policy_id: None,
            policy_version_id: None,
            approval_request_id: None,
            ticket_reference: None,
            environment: None,
            region: None,
        };
        context.validate()?;
        Ok(context)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if !valid_context_value(&self.request_id, 200)
            || self
                .source_ip
                .as_deref()
                .is_some_and(|value| value.parse::<std::net::IpAddr>().is_err())
            || [
                self.client_id.as_deref(),
                self.session_id.as_deref(),
                self.token_jti.as_deref(),
                self.ticket_reference.as_deref(),
                self.environment.as_deref(),
                self.region.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| !valid_context_value(value, 500))
        {
            return Err(DomainError::InvalidAuditContext);
        }
        Ok(())
    }
}

/// Identity and delegation context responsible for an audited action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    /// Actual principal that performed the action.
    pub principal_id: PrincipalId,
    /// Optional principal on whose behalf the action was performed.
    pub on_behalf_of: Option<PrincipalId>,
    /// Optional non-secret credential identifier used for the action.
    pub credential_id: Option<String>,
}

/// Resource affected by an audited action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditResource {
    /// Stable dotted resource type such as `identity.principal`.
    pub kind: String,
    /// Stable resource identifier.
    pub id: String,
}

/// Outcome of an audited action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    /// The requested action completed successfully.
    Success,
    /// The action failed after it was authorized.
    Failure,
    /// Policy denied the action before execution.
    Denied,
}

/// Append-only description of a security or business action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Globally unique event identifier.
    pub id: AuditEventId,
    /// Organization in which the action occurred.
    pub organization_id: OrganizationId,
    /// Identity and delegation context.
    pub actor: AuditActor,
    /// Stable dotted action such as `identity.principal.created`.
    pub action: String,
    /// Resource affected by the action.
    pub resource: AuditResource,
    /// Action result.
    pub outcome: AuditOutcome,
    /// Correlation identifier propagated across system boundaries.
    pub trace_id: String,
    /// Explicit request and governance correlation.
    pub context: AuditContext,
    /// Sanitized metadata that must never contain message bodies or secrets.
    pub details: Value,
    /// Server timestamp for the business action.
    pub occurred_at: DateTime<Utc>,
    /// Time the control plane accepted the event for persistence.
    pub received_at: DateTime<Utc>,
}

impl AuditEvent {
    /// Creates an event after validating stable action and resource identifiers.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        actor: AuditActor,
        action: impl Into<String>,
        resource: AuditResource,
        outcome: AuditOutcome,
        trace_id: impl Into<String>,
        details: Value,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let action = action.into();
        let trace_id = trace_id.into();
        let context = AuditContext::for_request(trace_id.clone())?;
        let definition =
            audit_event_definition(&action).ok_or(DomainError::UnregisteredAuditEvent)?;
        if !valid_dotted_identifier(&action)
            || !valid_dotted_identifier(&resource.kind)
            || definition.resource_kind != resource.kind
        {
            return Err(DomainError::InvalidAuditIdentifier);
        }
        if !details.is_object()
            || contains_forbidden_audit_data(&details)
            || serde_json::to_vec(&details).map_or(true, |encoded| encoded.len() > 64 * 1024)
        {
            return Err(DomainError::InvalidAuditDetails);
        }
        Ok(Self {
            id: AuditEventId::new(),
            organization_id,
            actor,
            action,
            resource,
            outcome,
            trace_id,
            context,
            details,
            occurred_at,
            received_at: Utc::now(),
        })
    }

    pub fn with_context(mut self, context: AuditContext) -> Result<Self, DomainError> {
        context.validate()?;
        self.context = context;
        Ok(self)
    }
}

fn valid_dotted_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn valid_context_value(value: &str, max: usize) -> bool {
    !value.is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control)
}

fn contains_forbidden_audit_data(value: &Value) -> bool {
    const FORBIDDEN_KEYS: [&str; 13] = [
        "password",
        "access_token",
        "refresh_token",
        "client_assertion",
        "authorization",
        "cookie",
        "secret",
        "private_key",
        "plaintext",
        "body",
        "raw_mime",
        "attachment_content",
        "message_body",
    ];
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            FORBIDDEN_KEYS.contains(&key.to_ascii_lowercase().as_str())
                || contains_forbidden_audit_data(value)
        }),
        Value::Array(values) => values.iter().any(contains_forbidden_audit_data),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unstable_audit_names() {
        let result = AuditEvent::new(
            OrganizationId::new(),
            AuditActor {
                principal_id: PrincipalId::new(),
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.principal.created",
            AuditResource {
                kind: "wrong.resource".into(),
                id: "id".into(),
            },
            AuditOutcome::Success,
            "trace",
            Value::Null,
            Utc::now(),
        );
        assert_eq!(result.unwrap_err(), DomainError::InvalidAuditIdentifier);
    }

    #[test]
    fn catalog_is_unique_valid_and_privileged_actions_are_explicit() {
        let mut actions = std::collections::BTreeSet::new();
        for definition in AUDIT_EVENT_CATALOG {
            assert!(valid_dotted_identifier(definition.action));
            assert!(valid_dotted_identifier(definition.resource_kind));
            assert!(actions.insert(definition.action));
            assert!(definition.privileged);
        }
    }

    #[test]
    fn rejects_secrets_and_message_content_at_any_depth() {
        let result = AuditEvent::new(
            OrganizationId::new(),
            AuditActor {
                principal_id: PrincipalId::new(),
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.principal.created",
            AuditResource {
                kind: "identity.principal".into(),
                id: "id".into(),
            },
            AuditOutcome::Success,
            "trace",
            serde_json::json!({"nested": {"access_token": "must-not-persist"}}),
            Utc::now(),
        );
        assert_eq!(result.unwrap_err(), DomainError::InvalidAuditDetails);
    }

    #[test]
    fn audit_details_must_be_a_structured_object() {
        let result = AuditEvent::new(
            OrganizationId::new(),
            AuditActor {
                principal_id: PrincipalId::new(),
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.principal.created",
            AuditResource {
                kind: "identity.principal".into(),
                id: "id".into(),
            },
            AuditOutcome::Success,
            "trace",
            serde_json::json!(["unstructured"]),
            Utc::now(),
        );
        assert_eq!(result.unwrap_err(), DomainError::InvalidAuditDetails);
    }
}
