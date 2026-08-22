use crate::{AgentIngressRateLimitDecision, AgentIngressRateLimitStore, StoreError};
use copilotmail_domain::{OrganizationId, PrincipalId};

/// Validates configured hierarchy before delegating cluster-wide admission to
/// the infrastructure adapter.
pub struct AgentIngressRateLimiter<S> {
    store: S,
}

impl<S> AgentIngressRateLimiter<S>
where
    S: AgentIngressRateLimitStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn admit(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        tenant_requests_per_minute: u32,
        principal_requests_per_minute: u32,
    ) -> Result<AgentIngressRateLimitDecision, AgentIngressRateLimitError> {
        if tenant_requests_per_minute == 0
            || principal_requests_per_minute == 0
            || principal_requests_per_minute > tenant_requests_per_minute
        {
            return Err(AgentIngressRateLimitError::InvalidLimits);
        }
        self.store
            .consume_agent_ingress_capacity(
                organization_id,
                principal_id,
                tenant_requests_per_minute,
                principal_requests_per_minute,
            )
            .await
            .map_err(AgentIngressRateLimitError::Store)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentIngressRateLimitError {
    #[error("Agent ingress rate limits are invalid")]
    InvalidLimits,
    #[error("Agent ingress rate-limit store failed")]
    Store(StoreError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MemoryStore {
        calls: Arc<Mutex<Vec<(u32, u32)>>>,
        decision: AgentIngressRateLimitDecision,
    }

    #[async_trait]
    impl AgentIngressRateLimitStore for MemoryStore {
        async fn consume_agent_ingress_capacity(
            &self,
            _: OrganizationId,
            _: PrincipalId,
            tenant: u32,
            principal: u32,
        ) -> Result<AgentIngressRateLimitDecision, StoreError> {
            self.calls.lock().unwrap().push((tenant, principal));
            Ok(self.decision)
        }
    }

    #[tokio::test]
    async fn invalid_hierarchy_fails_before_store_access() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let limiter = AgentIngressRateLimiter::new(MemoryStore {
            calls: calls.clone(),
            decision: AgentIngressRateLimitDecision::Allowed,
        });
        assert!(matches!(
            limiter
                .admit(OrganizationId::new(), PrincipalId::new(), 10, 11)
                .await,
            Err(AgentIngressRateLimitError::InvalidLimits)
        ));
        assert!(calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn preserves_cluster_admission_decision() {
        let limiter = AgentIngressRateLimiter::new(MemoryStore {
            calls: Arc::new(Mutex::new(Vec::new())),
            decision: AgentIngressRateLimitDecision::PrincipalLimited {
                retry_after_seconds: 17,
            },
        });
        assert_eq!(
            limiter
                .admit(OrganizationId::new(), PrincipalId::new(), 100, 10)
                .await
                .unwrap(),
            AgentIngressRateLimitDecision::PrincipalLimited {
                retry_after_seconds: 17
            }
        );
    }
}
