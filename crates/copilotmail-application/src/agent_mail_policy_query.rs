use crate::{AgentMailPolicyQueryStore, AgentMailPolicyRuntimeSnapshot, StoreError};
use copilotmail_domain::{AuthorizationContext, Permission};

#[derive(Debug, Clone)]
pub struct GetOwnAgentMailPolicyCommand {
    pub context: AuthorizationContext,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentMailPolicyQueryError {
    #[error("Agent mail policy query is forbidden")]
    Forbidden,
    #[error("Agent mail policy state is unavailable")]
    Unavailable,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct AgentMailPolicyQueryService<S> {
    store: S,
}

impl<S> AgentMailPolicyQueryService<S>
where
    S: AgentMailPolicyQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Returns only the authenticated Agent's own policy and aggregate usage.
    ///
    /// The principal and tenant are taken exclusively from the authorization
    /// context. A missing row also covers suspended, retired, non-Agent, and
    /// cross-tenant identities so callers cannot use this endpoint to probe
    /// other principals.
    pub async fn get_own_policy(
        &self,
        command: GetOwnAgentMailPolicyCommand,
    ) -> Result<AgentMailPolicyRuntimeSnapshot, AgentMailPolicyQueryError> {
        if !command
            .context
            .permits(command.context.organization_id, Permission::AgentOperate)
        {
            return Err(AgentMailPolicyQueryError::Forbidden);
        }
        let snapshot = self
            .store
            .get_agent_mail_policy_runtime(
                command.context.organization_id,
                command.context.principal_id,
            )
            .await?
            .ok_or(AgentMailPolicyQueryError::Forbidden)?;
        if snapshot.organization_id != command.context.organization_id
            || snapshot.agent_id != command.context.principal_id
            || snapshot.active_policy.as_ref().is_some_and(|policy| {
                policy.organization_id() != command.context.organization_id
                    || policy.agent_id() != command.context.principal_id
            })
        {
            return Err(AgentMailPolicyQueryError::Unavailable);
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use copilotmail_domain::{
        AgentEnvironment, AgentMailPolicyVersion, OrganizationId, PrincipalId,
    };
    use std::collections::BTreeSet;

    struct PanicStore;

    #[async_trait]
    impl AgentMailPolicyQueryStore for PanicStore {
        async fn get_agent_mail_policy_runtime(
            &self,
            _organization_id: OrganizationId,
            _agent_id: PrincipalId,
        ) -> Result<Option<AgentMailPolicyRuntimeSnapshot>, StoreError> {
            panic!("store must not be called before authorization")
        }
    }

    #[tokio::test]
    async fn missing_agent_operate_fails_before_storage() {
        let organization_id = OrganizationId::new();
        let result = AgentMailPolicyQueryService::new(PanicStore)
            .get_own_policy(GetOwnAgentMailPolicyCommand {
                context: AuthorizationContext {
                    principal_id: PrincipalId::new(),
                    organization_id,
                    permissions: BTreeSet::new(),
                    authentication: None,
                },
            })
            .await;
        assert!(matches!(result, Err(AgentMailPolicyQueryError::Forbidden)));
    }

    struct SnapshotStore {
        snapshot: AgentMailPolicyRuntimeSnapshot,
    }

    #[async_trait]
    impl AgentMailPolicyQueryStore for SnapshotStore {
        async fn get_agent_mail_policy_runtime(
            &self,
            _organization_id: OrganizationId,
            _agent_id: PrincipalId,
        ) -> Result<Option<AgentMailPolicyRuntimeSnapshot>, StoreError> {
            Ok(Some(self.snapshot.clone()))
        }
    }

    #[tokio::test]
    async fn query_is_bound_to_the_authenticated_agent_and_returns_aggregate_usage() {
        let organization_id = OrganizationId::new();
        let agent_id = PrincipalId::new();
        let policy = AgentMailPolicyVersion::new(
            organization_id,
            agent_id,
            3,
            ["internal.example".into()],
            10,
            25,
            4,
            true,
            2,
        )
        .unwrap();
        let snapshot = AgentMailPolicyRuntimeSnapshot {
            organization_id,
            agent_id,
            environment: AgentEnvironment::Production,
            active_policy: Some(policy),
            observed_at: Utc.with_ymd_and_hms(2026, 7, 27, 2, 15, 0).unwrap(),
            hourly_window_started_at: Utc.with_ymd_and_hms(2026, 7, 27, 1, 15, 0).unwrap(),
            admitted_messages_in_hour: 7,
        };
        let result = AgentMailPolicyQueryService::new(SnapshotStore {
            snapshot: snapshot.clone(),
        })
        .get_own_policy(GetOwnAgentMailPolicyCommand {
            context: AuthorizationContext {
                principal_id: agent_id,
                organization_id,
                permissions: BTreeSet::from([Permission::AgentOperate]),
                authentication: None,
            },
        })
        .await
        .unwrap();
        assert_eq!(result, snapshot);
    }

    #[tokio::test]
    async fn mismatched_adapter_identity_fails_closed() {
        let organization_id = OrganizationId::new();
        let agent_id = PrincipalId::new();
        let result = AgentMailPolicyQueryService::new(SnapshotStore {
            snapshot: AgentMailPolicyRuntimeSnapshot {
                organization_id,
                agent_id: PrincipalId::new(),
                environment: AgentEnvironment::Testing,
                active_policy: None,
                observed_at: Utc.with_ymd_and_hms(2026, 7, 27, 2, 15, 0).unwrap(),
                hourly_window_started_at: Utc.with_ymd_and_hms(2026, 7, 27, 1, 15, 0).unwrap(),
                admitted_messages_in_hour: 0,
            },
        })
        .get_own_policy(GetOwnAgentMailPolicyCommand {
            context: AuthorizationContext {
                principal_id: agent_id,
                organization_id,
                permissions: BTreeSet::from([Permission::AgentOperate]),
                authentication: None,
            },
        })
        .await;
        assert!(matches!(
            result,
            Err(AgentMailPolicyQueryError::Unavailable)
        ));
    }
}
