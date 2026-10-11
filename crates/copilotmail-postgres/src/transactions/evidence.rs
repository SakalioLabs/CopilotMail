use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_append_audit_event(
        &mut self,
        event: &AuditEvent,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO audit_events(
                id, organization_id, actor_principal_id, on_behalf_of_principal_id,
                credential_id, action, resource_type, resource_id, outcome,
                trace_id, details, occurred_at, received_at, request_id,
                subject_principal_id, source_ip, client_id, session_id, token_jti,
                policy_id, policy_version_id, approval_request_id, ticket_reference,
                environment, region
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                $14, $15, $16::inet, $17, $18, $19, $20, $21, $22, $23, $24, $25
            )
            "#,
        )
        .bind(event.id.as_uuid())
        .bind(event.organization_id.as_uuid())
        .bind(event.actor.principal_id.as_uuid())
        .bind(event.actor.on_behalf_of.map(|id| id.as_uuid()))
        .bind(event.actor.credential_id.as_deref())
        .bind(&event.action)
        .bind(&event.resource.kind)
        .bind(&event.resource.id)
        .bind(audit_outcome(event.outcome))
        .bind(&event.trace_id)
        .bind(&event.details)
        .bind(event.occurred_at)
        .bind(event.received_at)
        .bind(&event.context.request_id)
        .bind(event.context.subject_principal_id.map(PrincipalId::as_uuid))
        .bind(&event.context.source_ip)
        .bind(&event.context.client_id)
        .bind(&event.context.session_id)
        .bind(&event.context.token_jti)
        .bind(event.context.policy_id.map(PolicyId::as_uuid))
        .bind(
            event
                .context
                .policy_version_id
                .map(PolicyVersionId::as_uuid),
        )
        .bind(
            event
                .context
                .approval_request_id
                .map(copilotmail_domain::ApprovalRequestId::as_uuid),
        )
        .bind(&event.context.ticket_reference)
        .bind(&event.context.environment)
        .bind(&event.context.region)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_enqueue_event(
        &mut self,
        event: &PendingEvent,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO outbox_events(
                id, event_type, aggregate_type, aggregate_id,
                schema_version, payload, mail_data_plane_id, occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(event.id)
        .bind(&event.event_type)
        .bind(&event.aggregate_type)
        .bind(&event.aggregate_id)
        .bind(i32::try_from(event.schema_version).map_err(|_| StoreError::Conflict)?)
        .bind(&event.payload)
        .bind(event.data_plane_id.map(MailDataPlaneId::as_uuid))
        .bind(event.occurred_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_commit(mut self: Box<Self>) -> Result<(), StoreError> {
        if !self.tx_bound_authentication_still_valid().await? {
            return Err(StoreError::AuthenticationInvalid);
        }
        self.transaction
            .take()
            .ok_or(StoreError::Unavailable)?
            .commit()
            .await
            .map_err(map_store_error)
    }
}
