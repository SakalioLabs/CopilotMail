use crate::*;

#[async_trait]
impl MailDomainQueryStore for PgControlPlaneStore {
    async fn get_mail_domain(
        &self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        now: DateTime<Utc>,
    ) -> Result<Option<MailDomainSummary>, StoreError> {
        let row = sqlx::query_as::<_, MailDomainSummaryRow>(
            r#"
            SELECT domain.id AS mail_domain_id,
                   domain.organization_id,
                   domain.name,
                   domain.status,
                   domain.route_mode,
                   domain.ownership_challenge_version,
                   route.version AS route_version,
                   route.inbound_enabled,
                   route.outbound_enabled,
                   route.smart_host_ref,
                   route.dkim_key_ref,
                   domain.mail_engine_domain_ref,
                   domain.mail_engine_projection_status,
                   domain.mail_engine_projected_enabled,
                   domain.mail_engine_projection_last_attempt_at,
                   domain.mail_engine_projection_succeeded_at,
                   domain.mail_engine_projection_error_code,
                   observation.observed_at AS latest_dns_observed_at,
                   observation.valid_until AS latest_dns_valid_until,
                   COALESCE(
                       observation.ownership_txt
                       AND observation.mx_present
                       AND observation.mx_hosts_public
                       AND observation.spf_present
                       AND observation.dkim_present
                       AND observation.dmarc_present
                       AND observation.observed_at <= $3
                       AND observation.valid_until > $3,
                       false
                   ) AS latest_dns_activation_ready,
                   COALESCE(observation.failure_codes, '{}'::text[])
                       AS latest_dns_failure_codes
              FROM mail_domains AS domain
              JOIN mail_domain_routes AS route
                ON route.mail_domain_id = domain.id
               AND route.organization_id = domain.organization_id
              LEFT JOIN LATERAL (
                    SELECT observed_at, valid_until, ownership_txt, mx_present,
                           mx_hosts_public, spf_present, dkim_present, dmarc_present,
                           failure_codes
                      FROM mail_domain_dns_observations
                     WHERE organization_id = domain.organization_id
                       AND mail_domain_id = domain.id
                       AND ownership_token_sha256 = domain.ownership_token_sha256
                     ORDER BY observed_at DESC, created_at DESC, id DESC
                     LIMIT 1
              ) AS observation ON true
             WHERE domain.id = $1 AND domain.organization_id = $2
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_domain_summary_from_row).transpose()
    }

    async fn list_mail_domains(
        &self,
        organization_id: OrganizationId,
        after_name: Option<&MailDomainName>,
        limit: i64,
        now: DateTime<Utc>,
    ) -> Result<Vec<MailDomainSummary>, StoreError> {
        if !(1..=200).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, MailDomainSummaryRow>(
            r#"
            SELECT domain.id AS mail_domain_id,
                   domain.organization_id,
                   domain.name,
                   domain.status,
                   domain.route_mode,
                   domain.ownership_challenge_version,
                   route.version AS route_version,
                   route.inbound_enabled,
                   route.outbound_enabled,
                   route.smart_host_ref,
                   route.dkim_key_ref,
                   domain.mail_engine_domain_ref,
                   domain.mail_engine_projection_status,
                   domain.mail_engine_projected_enabled,
                   domain.mail_engine_projection_last_attempt_at,
                   domain.mail_engine_projection_succeeded_at,
                   domain.mail_engine_projection_error_code,
                   observation.observed_at AS latest_dns_observed_at,
                   observation.valid_until AS latest_dns_valid_until,
                   COALESCE(
                       observation.ownership_txt
                       AND observation.mx_present
                       AND observation.mx_hosts_public
                       AND observation.spf_present
                       AND observation.dkim_present
                       AND observation.dmarc_present
                       AND observation.observed_at <= $4
                       AND observation.valid_until > $4,
                       false
                   ) AS latest_dns_activation_ready,
                   COALESCE(observation.failure_codes, '{}'::text[])
                       AS latest_dns_failure_codes
              FROM mail_domains AS domain
              JOIN mail_domain_routes AS route
                ON route.mail_domain_id = domain.id
               AND route.organization_id = domain.organization_id
              LEFT JOIN LATERAL (
                    SELECT observed_at, valid_until, ownership_txt, mx_present,
                           mx_hosts_public, spf_present, dkim_present, dmarc_present,
                           failure_codes
                      FROM mail_domain_dns_observations
                     WHERE organization_id = domain.organization_id
                       AND mail_domain_id = domain.id
                       AND ownership_token_sha256 = domain.ownership_token_sha256
                     ORDER BY observed_at DESC, created_at DESC, id DESC
                     LIMIT 1
              ) AS observation ON true
             WHERE domain.organization_id = $1
               AND ($2::text IS NULL OR domain.name > $2)
             ORDER BY domain.name
             LIMIT $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(after_name.map(MailDomainName::as_str))
        .bind(limit)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter().map(mail_domain_summary_from_row).collect()
    }
}
