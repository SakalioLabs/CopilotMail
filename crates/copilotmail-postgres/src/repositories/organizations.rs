use crate::*;

#[async_trait]
impl OrganizationQueryStore for PgControlPlaneStore {
    async fn get_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<OrganizationSummary>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct OrganizationRow {
            id: Uuid,
            slug: String,
            display_name: String,
        }

        sqlx::query_as::<_, OrganizationRow>(
            "SELECT id, slug, display_name FROM organizations WHERE id = $1",
        )
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)
        .map(|row| {
            row.map(|row| OrganizationSummary {
                organization_id: OrganizationId::from_uuid(row.id),
                slug: row.slug,
                display_name: row.display_name,
            })
        })
    }
}
