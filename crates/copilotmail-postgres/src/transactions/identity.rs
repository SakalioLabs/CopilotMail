use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_find_organization_bootstrap(
        &mut self,
        slug: &OrganizationSlug,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(slug.as_str())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct ExistingRow {
            organization_id: Uuid,
            root_principal_id: Uuid,
            slug: String,
            display_name: String,
        }

        let existing = sqlx::query_as::<_, ExistingRow>(
            r#"
            SELECT organization.id AS organization_id,
                   bootstrap.root_principal_id,
                   organization.slug,
                   organization.display_name
              FROM organizations AS organization
              JOIN organization_bootstraps AS bootstrap
                ON bootstrap.organization_id = organization.id
             WHERE organization.slug = $1
            "#,
        )
        .bind(slug.as_str())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        Ok(existing.map(|row| ExistingOrganizationBootstrap {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            root_principal_id: PrincipalId::from_uuid(row.root_principal_id),
            slug: row.slug,
            display_name: row.display_name,
        }))
    }

    pub(crate) async fn tx_insert_organization(
        &mut self,
        organization: &Organization,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO organizations(id, slug, display_name, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(organization.id().as_uuid())
        .bind(organization.slug().as_str())
        .bind(organization.display_name())
        .bind(organization.created_at())
        .bind(organization.updated_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_insert_principal(
        &mut self,
        principal: &Principal,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO principals(
                id, organization_id, kind, status, display_name, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(principal.id().as_uuid())
        .bind(principal.organization_id().as_uuid())
        .bind(principal_kind(principal.kind()))
        .bind(principal_status(principal.status()))
        .bind(principal.display_name())
        .bind(principal.created_at())
        .bind(principal.updated_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_record_organization_bootstrap(
        &mut self,
        organization_id: OrganizationId,
        root_principal_id: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO organization_bootstraps(
                organization_id, root_principal_id, created_at
            )
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(root_principal_id.as_uuid())
        .bind(created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_oidc_owner_binding(
        &mut self,
        organization_id: OrganizationId,
    ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 1))")
            .bind(organization_id.to_string())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct BindingRow {
            organization_id: Uuid,
            principal_id: Uuid,
            identity_provider: String,
            identity_subject: String,
            display_name: String,
        }
        let row = sqlx::query_as::<_, BindingRow>(
            r#"
            SELECT binding.organization_id,
                   binding.principal_id,
                   binding.identity_provider,
                   binding.identity_subject,
                   principal.display_name
              FROM oidc_owner_bindings AS binding
              JOIN principals AS principal ON principal.id = binding.principal_id
             WHERE binding.organization_id = $1
            "#,
        )
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingOidcOwnerBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            issuer: row.identity_provider,
            subject: row.identity_subject,
            display_name: row.display_name,
        }))
    }

    pub(crate) async fn tx_find_organization_bootstrap_by_id(
        &mut self,
        organization_id: OrganizationId,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct BootstrapRow {
            organization_id: Uuid,
            root_principal_id: Uuid,
            slug: String,
            display_name: String,
        }
        let row = sqlx::query_as::<_, BootstrapRow>(
            r#"
            SELECT organization.id AS organization_id,
                   bootstrap.root_principal_id,
                   organization.slug,
                   organization.display_name
              FROM organization_bootstraps AS bootstrap
              JOIN organizations AS organization
                ON organization.id = bootstrap.organization_id
             WHERE organization.id = $1
            "#,
        )
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingOrganizationBootstrap {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            root_principal_id: PrincipalId::from_uuid(row.root_principal_id),
            slug: row.slug,
            display_name: row.display_name,
        }))
    }

    pub(crate) async fn tx_insert_human_profile(
        &mut self,
        profile: &HumanProfile,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO human_profiles(
                principal_id, identity_provider, identity_subject
            )
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(profile.principal_id().as_uuid())
        .bind(profile.identity_provider())
        .bind(profile.identity_subject())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_record_oidc_owner_binding(
        &mut self,
        binding: &ExistingOidcOwnerBinding,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO oidc_owner_bindings(
                organization_id, principal_id, identity_provider,
                identity_subject, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(binding.organization_id.as_uuid())
        .bind(binding.principal_id.as_uuid())
        .bind(&binding.issuer)
        .bind(&binding.subject)
        .bind(created_by.as_uuid())
        .bind(created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_retire_principal(
        &mut self,
        principal_id: PrincipalId,
        retired_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE principals
               SET status = 'retired',
                   updated_at = $2
             WHERE id = $1
               AND kind = 'service'
               AND status <> 'retired'
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(retired_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(StoreError::MissingReference)
        }
    }

    pub(crate) async fn tx_revoke_active_role_assignments(
        &mut self,
        principal_id: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE role_assignments
               SET revoked_at = $2
             WHERE principal_id = $1
               AND revoked_at IS NULL
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_human_identity(
        &mut self,
        organization_id: OrganizationId,
        identity_provider: &str,
        external_id: &str,
    ) -> Result<Option<ExistingHumanIdentity>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct HumanRow {
            organization_id: Uuid,
            principal_id: Uuid,
            display_name: String,
            active: bool,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, HumanRow>(
            r#"
            SELECT principal.organization_id,
                   principal.id AS principal_id,
                   principal.display_name,
                   principal.status = 'active' AS active,
                   principal.created_at
              FROM human_profiles AS profile
              JOIN principals AS principal ON principal.id = profile.principal_id
             WHERE principal.organization_id = $1
               AND principal.kind = 'human'
               AND principal.status <> 'retired'
               AND profile.identity_provider = $2
               AND profile.identity_subject = $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(identity_provider)
        .bind(external_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingHumanIdentity {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            display_name: row.display_name,
            active: row.active,
            created_at: row.created_at,
        }))
    }
}
