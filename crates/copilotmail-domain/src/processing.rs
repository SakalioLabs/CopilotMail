use crate::{DomainError, LeaseId, MessageId, PrincipalId, SubscriptionId};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Machine-processing status, deliberately separate from human read state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum AgentProcessingStatus {
    /// Available to be claimed by an Agent subscriber.
    Pending,
    /// Temporarily owned by a specific Agent until the lease expires.
    Leased {
        /// Unique lease token required for acknowledgement.
        lease_id: LeaseId,
        /// Principal currently processing the message.
        principal_id: PrincipalId,
        /// Time after which another consumer may reclaim the message.
        expires_at: DateTime<Utc>,
    },
    /// Successfully processed and acknowledged.
    Acknowledged {
        /// Time of acknowledgement.
        acknowledged_at: DateTime<Utc>,
    },
    /// Permanently removed from automatic processing after repeated failure.
    DeadLettered {
        /// Time the terminal failure was recorded.
        failed_at: DateTime<Utc>,
        /// Sanitized machine-readable failure reason.
        reason: String,
    },
}

/// Per-subscription Agent processing state for one message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessageState {
    /// Subscription receiving the message.
    pub subscription_id: SubscriptionId,
    /// Referenced message.
    pub message_id: MessageId,
    /// Number of successful claims made so far.
    pub attempt_count: u32,
    /// Current processing state.
    pub status: AgentProcessingStatus,
    /// Most recent sanitized processing error.
    pub last_error: Option<String>,
}

impl AgentMessageState {
    /// Creates an independently pending delivery for an Agent subscription.
    #[must_use]
    pub const fn new(subscription_id: SubscriptionId, message_id: MessageId) -> Self {
        Self {
            subscription_id,
            message_id,
            attempt_count: 0,
            status: AgentProcessingStatus::Pending,
            last_error: None,
        }
    }

    /// Claims the message and returns a lease token required by later operations.
    pub fn claim(
        &mut self,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
        duration: Duration,
    ) -> Result<LeaseId, DomainError> {
        if duration <= Duration::zero() {
            return Err(DomainError::InvalidLeaseDuration);
        }

        match &self.status {
            AgentProcessingStatus::Pending => {}
            AgentProcessingStatus::Leased { expires_at, .. } if *expires_at <= now => {}
            AgentProcessingStatus::Leased { .. } => {
                return Err(DomainError::InvalidProcessingTransition);
            }
            AgentProcessingStatus::Acknowledged { .. }
            | AgentProcessingStatus::DeadLettered { .. } => {
                return Err(DomainError::TerminalProcessingState);
            }
        }

        let lease_id = LeaseId::new();
        self.attempt_count = self.attempt_count.saturating_add(1);
        self.status = AgentProcessingStatus::Leased {
            lease_id,
            principal_id,
            expires_at: now + duration,
        };
        Ok(lease_id)
    }

    /// Acknowledges successful processing when the active lease matches.
    pub fn acknowledge(
        &mut self,
        principal_id: PrincipalId,
        lease_id: LeaseId,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        self.require_live_lease(principal_id, lease_id, now)?;
        self.status = AgentProcessingStatus::Acknowledged {
            acknowledged_at: now,
        };
        self.last_error = None;
        Ok(())
    }

    /// Records a processing failure and either retries or enters the dead letter state.
    pub fn reject(
        &mut self,
        principal_id: PrincipalId,
        lease_id: LeaseId,
        now: DateTime<Utc>,
        reason: impl Into<String>,
        max_attempts: u32,
    ) -> Result<(), DomainError> {
        self.require_live_lease(principal_id, lease_id, now)?;
        let reason = sanitize_error(reason.into());
        self.last_error = Some(reason.clone());
        if max_attempts == 0 || self.attempt_count >= max_attempts {
            self.status = AgentProcessingStatus::DeadLettered {
                failed_at: now,
                reason,
            };
        } else {
            self.status = AgentProcessingStatus::Pending;
        }
        Ok(())
    }

    /// Extends only the caller's current unexpired lease. The new deadline is
    /// relative to server time and cannot exceed the supplied bounded duration.
    pub fn extend_lease(
        &mut self,
        principal_id: PrincipalId,
        lease_id: LeaseId,
        now: DateTime<Utc>,
        duration: Duration,
    ) -> Result<DateTime<Utc>, DomainError> {
        if duration <= Duration::zero() {
            return Err(DomainError::InvalidLeaseDuration);
        }
        self.require_live_lease(principal_id, lease_id, now)?;
        let expires_at = now + duration;
        self.status = AgentProcessingStatus::Leased {
            lease_id,
            principal_id,
            expires_at,
        };
        Ok(expires_at)
    }

    fn require_live_lease(
        &self,
        principal_id: PrincipalId,
        lease_id: LeaseId,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        match &self.status {
            AgentProcessingStatus::Leased {
                lease_id: active,
                principal_id: owner,
                expires_at,
            } if *active == lease_id && *owner == principal_id && *expires_at > now => Ok(()),
            AgentProcessingStatus::Acknowledged { .. }
            | AgentProcessingStatus::DeadLettered { .. } => {
                Err(DomainError::TerminalProcessingState)
            }
            _ => Err(DomainError::LeaseMismatch),
        }
    }
}

fn sanitize_error(value: String) -> String {
    let value = value
        .chars()
        .filter(|character| !character.is_control())
        .take(500)
        .collect::<String>();
    if value.is_empty() {
        "processing_failed".into()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_read_state_cannot_ack_agent_delivery() {
        let now = Utc::now();
        let mut state = AgentMessageState::new(SubscriptionId::new(), MessageId::new());
        let lease = state
            .claim(PrincipalId::new(), now, Duration::minutes(5))
            .unwrap();
        let principal_id = match state.status {
            AgentProcessingStatus::Leased { principal_id, .. } => principal_id,
            _ => unreachable!(),
        };
        assert!(
            state
                .acknowledge(principal_id, LeaseId::new(), now)
                .is_err()
        );
        state.acknowledge(principal_id, lease, now).unwrap();
        assert!(matches!(
            state.status,
            AgentProcessingStatus::Acknowledged { .. }
        ));
    }

    #[test]
    fn expired_lease_can_be_reclaimed_and_failed_to_dead_letter() {
        let now = Utc::now();
        let mut state = AgentMessageState::new(SubscriptionId::new(), MessageId::new());
        state
            .claim(PrincipalId::new(), now, Duration::seconds(1))
            .unwrap();
        let principal_id = PrincipalId::new();
        let second = state
            .claim(
                principal_id,
                now + Duration::seconds(2),
                Duration::minutes(1),
            )
            .unwrap();
        state
            .reject(
                principal_id,
                second,
                now + Duration::seconds(3),
                "fatal\nsecret",
                2,
            )
            .unwrap();
        assert!(matches!(
            state.status,
            AgentProcessingStatus::DeadLettered { .. }
        ));
        assert_eq!(state.last_error.as_deref(), Some("fatalsecret"));
    }

    #[test]
    fn expired_or_foreign_leases_cannot_complete_and_extension_is_bounded() {
        let now = Utc::now();
        let owner = PrincipalId::new();
        let mut state = AgentMessageState::new(SubscriptionId::new(), MessageId::new());
        let lease = state.claim(owner, now, Duration::seconds(10)).unwrap();
        assert!(state.acknowledge(PrincipalId::new(), lease, now).is_err());
        assert!(
            state
                .reject(owner, lease, now + Duration::seconds(10), "late", 3)
                .is_err()
        );
        let expires_at = state
            .extend_lease(
                owner,
                lease,
                now + Duration::seconds(1),
                Duration::seconds(30),
            )
            .unwrap();
        assert_eq!(expires_at, now + Duration::seconds(31));
    }
}
