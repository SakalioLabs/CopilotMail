use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_mail_domain(
        &mut self,
        domain: &MailDomain,
        created_by: PrincipalId,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mail_domains(
                id, organization_id, name, status, route_mode,
                ownership_token_sha256, ownership_challenge_version,
                mail_engine_domain_ref, created_by,
                created_at, updated_at, verified_at, activated_at, suspended_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, $8, $9, $9, $10, $11, $12)
            "#,
        )
        .bind(domain.id.as_uuid())
        .bind(domain.organization_id.as_uuid())
        .bind(domain.name.as_str())
        .bind(mail_domain_status(domain.status))
        .bind(mail_domain_route_mode(domain.route_mode))
        .bind(domain.ownership_challenge_sha256.as_slice())
        .bind(domain.ownership_challenge_version)
        .bind(created_by.as_uuid())
        .bind(domain.created_at)
        .bind(domain.verified_at)
        .bind(domain.activated_at)
        .bind(domain.suspended_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_insert_mail_domain_route(
        &mut self,
        route: &MailDomainRoute,
        updated_by: PrincipalId,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mail_domain_routes(
                mail_domain_id, organization_id, version,
                inbound_enabled, outbound_enabled, smart_host_ref,
                dkim_key_ref, updated_by, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(route.mail_domain_id.as_uuid())
        .bind(route.organization_id.as_uuid())
        .bind(route.version)
        .bind(route.inbound_enabled)
        .bind(route.outbound_enabled)
        .bind(&route.smart_host_ref)
        .bind(&route.dkim_key_ref)
        .bind(updated_by.as_uuid())
        .bind(route.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_mail_domain_for_update(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomain>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            name: String,
            status: String,
            route_mode: String,
            ownership_token_sha256: Vec<u8>,
            ownership_challenge_version: i64,
            created_at: DateTime<Utc>,
            verified_at: Option<DateTime<Utc>>,
            activated_at: Option<DateTime<Utc>>,
            suspended_at: Option<DateTime<Utc>>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, name, status, route_mode,
                   ownership_token_sha256, ownership_challenge_version,
                   created_at, verified_at,
                   activated_at, suspended_at
              FROM mail_domains
             WHERE id = $1 AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        row.map(|row| {
            Ok(MailDomain {
                id: MailDomainId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                name: MailDomainName::parse(row.name).map_err(|error| {
                    tracing::error!(%error, "invalid mail domain name persisted");
                    StoreError::Unavailable
                })?,
                status: mail_domain_status_from_str(&row.status)?,
                route_mode: mail_domain_route_mode_from_str(&row.route_mode)?,
                ownership_challenge_sha256: sha256_array(
                    &row.ownership_token_sha256,
                    "mail domain ownership digest",
                )?,
                ownership_challenge_version: row.ownership_challenge_version,
                created_at: row.created_at,
                verified_at: row.verified_at,
                activated_at: row.activated_at,
                suspended_at: row.suspended_at,
            })
        })
        .transpose()
    }

    pub(crate) async fn tx_update_mail_domain(
        &mut self,
        domain: &MailDomain,
        updated_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mail_domains
               SET status = $3,
                   updated_at = $4,
                   verified_at = $5,
                   activated_at = $6,
                   suspended_at = $7,
                   ownership_token_sha256 = $8,
                   ownership_challenge_version = $9,
                   mail_engine_projection_status = CASE
                       WHEN status <> $3 AND $3 IN ('active', 'suspended')
                           THEN 'pending'
                       ELSE mail_engine_projection_status
                   END,
                   mail_engine_projection_error_code = CASE
                       WHEN status <> $3 AND $3 IN ('active', 'suspended')
                           THEN NULL
                       ELSE mail_engine_projection_error_code
                   END
             WHERE id = $1 AND organization_id = $2
            "#,
        )
        .bind(domain.id.as_uuid())
        .bind(domain.organization_id.as_uuid())
        .bind(mail_domain_status(domain.status))
        .bind(updated_at)
        .bind(domain.verified_at)
        .bind(domain.activated_at)
        .bind(domain.suspended_at)
        .bind(domain.ownership_challenge_sha256.as_slice())
        .bind(domain.ownership_challenge_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn tx_insert_mail_domain_dns_observation(
        &mut self,
        observation: &MailDomainDnsObservation,
    ) -> Result<(), StoreError> {
        let evidence = &observation.evidence;
        sqlx::query(
            r#"
            INSERT INTO mail_domain_dns_observations(
                id, organization_id, mail_domain_id, ownership_token_sha256,
                observed_at, valid_until, ownership_txt, mx_present,
                mx_hosts_public, spf_present, dkim_present, dmarc_present,
                raw_answers, failure_codes, created_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                $13, $14, $15
            )
            "#,
        )
        .bind(evidence.id.as_uuid())
        .bind(observation.organization_id.as_uuid())
        .bind(observation.mail_domain_id.as_uuid())
        .bind(evidence.ownership_challenge_sha256.as_slice())
        .bind(evidence.observed_at)
        .bind(evidence.valid_until)
        .bind(evidence.ownership_txt)
        .bind(evidence.mx_present)
        .bind(evidence.mx_hosts_public)
        .bind(evidence.spf_present)
        .bind(evidence.dkim_present)
        .bind(evidence.dmarc_present)
        .bind(&observation.raw_answers)
        .bind(&observation.failure_codes)
        .bind(observation.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_latest_mail_domain_dns_is_activation_ready(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        ownership_challenge_sha256: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let ready = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT ownership_txt
               AND mx_present
               AND mx_hosts_public
               AND spf_present
               AND dkim_present
               AND dmarc_present
               AND observed_at <= $4
               AND valid_until > $4
              FROM mail_domain_dns_observations
             WHERE organization_id = $1
               AND mail_domain_id = $2
               AND ownership_token_sha256 = $3
             ORDER BY observed_at DESC, created_at DESC, id DESC
             LIMIT 1
             FOR SHARE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mail_domain_id.as_uuid())
        .bind(ownership_challenge_sha256.as_slice())
        .bind(now)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(ready.unwrap_or(false))
    }

    pub(crate) async fn tx_assign_mail_domain_data_plane(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        data_plane_id: MailDataPlaneId,
        assigned_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mail_domains AS domain
               SET mail_data_plane_id = COALESCE(domain.mail_data_plane_id, $3),
                   mail_data_plane_assigned_at = COALESCE(
                       domain.mail_data_plane_assigned_at,
                       $4
                   )
             WHERE domain.id = $1
               AND domain.organization_id = $2
               AND domain.status IN ('verified', 'active', 'suspended')
               AND (
                   (
                       domain.mail_data_plane_id IS NULL
                       AND domain.mail_data_plane_assigned_at IS NULL
                   )
                   OR (
                       domain.mail_data_plane_id = $3
                       AND domain.mail_data_plane_assigned_at IS NOT NULL
                   )
               )
               AND $4 >= domain.created_at
               AND EXISTS (
                   SELECT 1
                     FROM mail_data_planes AS plane
                    WHERE plane.id = $3
               )
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(data_plane_id.as_uuid())
        .bind(assigned_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn tx_assigned_mail_domain_data_plane(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDataPlaneId>, StoreError> {
        sqlx::query_scalar::<_, Option<Uuid>>(
            r#"
            SELECT mail_data_plane_id
              FROM mail_domains
             WHERE id = $1
               AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
        .map(Option::flatten)
        .map(|value| value.map(MailDataPlaneId::from_uuid))
    }

    pub(crate) async fn tx_find_mail_domain_route_for_update(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomainRoute>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            mail_domain_id: Uuid,
            organization_id: Uuid,
            version: i64,
            inbound_enabled: bool,
            outbound_enabled: bool,
            smart_host_ref: Option<String>,
            dkim_key_ref: Option<String>,
            updated_at: DateTime<Utc>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT mail_domain_id, organization_id, version,
                   inbound_enabled, outbound_enabled, smart_host_ref,
                   dkim_key_ref, updated_at
              FROM mail_domain_routes
             WHERE mail_domain_id = $1 AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        Ok(row.map(|row| MailDomainRoute {
            mail_domain_id: MailDomainId::from_uuid(row.mail_domain_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            version: row.version,
            inbound_enabled: row.inbound_enabled,
            outbound_enabled: row.outbound_enabled,
            smart_host_ref: row.smart_host_ref,
            dkim_key_ref: row.dkim_key_ref,
            updated_at: row.updated_at,
        }))
    }

    pub(crate) async fn tx_update_mail_domain_route(
        &mut self,
        route: &MailDomainRoute,
        expected_version: i64,
        updated_by: PrincipalId,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mail_domain_routes
               SET version = $4,
                   inbound_enabled = $5,
                   outbound_enabled = $6,
                   smart_host_ref = $7,
                   dkim_key_ref = $8,
                   updated_by = $9,
                   updated_at = $10
             WHERE mail_domain_id = $1
               AND organization_id = $2
               AND version = $3
            "#,
        )
        .bind(route.mail_domain_id.as_uuid())
        .bind(route.organization_id.as_uuid())
        .bind(expected_version)
        .bind(route.version)
        .bind(route.inbound_enabled)
        .bind(route.outbound_enabled)
        .bind(&route.smart_host_ref)
        .bind(&route.dkim_key_ref)
        .bind(updated_by.as_uuid())
        .bind(route.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }
}
