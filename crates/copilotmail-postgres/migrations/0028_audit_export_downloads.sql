INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('audit.export.downloaded', 'audit.export_grant', 'critical', true);

-- Records every retrieval performed with an export grant. The ledger is
-- append-only: a download can never be hidden by rewriting or deleting rows.
CREATE TABLE audit_export_downloads (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    grant_id uuid NOT NULL,
    archive_id uuid NOT NULL,
    downloaded_by uuid NOT NULL,
    artifact text NOT NULL CHECK (artifact IN ('events', 'manifest')),
    object_key text NOT NULL CHECK (length(object_key) BETWEEN 1 AND 1024),
    object_version_id text NOT NULL CHECK (length(object_version_id) BETWEEN 1 AND 1024),
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    size_bytes bigint NOT NULL CHECK (size_bytes > 0),
    downloaded_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    FOREIGN KEY(grant_id, organization_id)
        REFERENCES audit_export_grants(id, organization_id),
    FOREIGN KEY(archive_id, organization_id)
        REFERENCES audit_archives(id, organization_id),
    FOREIGN KEY(downloaded_by, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE INDEX audit_export_downloads_tenant_time_idx
    ON audit_export_downloads(organization_id, downloaded_at DESC, id DESC);

CREATE INDEX audit_export_downloads_grant_idx
    ON audit_export_downloads(organization_id, grant_id, downloaded_at DESC);

-- A download row is valid only when the grant belongs to the same tenant,
-- names the same archive, was requested by the same Human, and had not yet
-- expired at the moment the object was served.
CREATE OR REPLACE FUNCTION validate_audit_export_download_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM audit_export_grants AS grant_row
         WHERE grant_row.id = NEW.grant_id
           AND grant_row.organization_id = NEW.organization_id
           AND grant_row.archive_id = NEW.archive_id
           AND grant_row.requested_by = NEW.downloaded_by
           AND NEW.downloaded_at >= grant_row.created_at
           AND NEW.downloaded_at < grant_row.expires_at
    )
    OR NOT EXISTS (
        SELECT 1
          FROM principals AS actor
         WHERE actor.id = NEW.downloaded_by
           AND actor.organization_id = NEW.organization_id
           AND actor.kind = 'human'
           AND actor.status = 'active'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM audit_archives AS archive
         WHERE archive.id = NEW.archive_id
           AND archive.organization_id = NEW.organization_id
           AND (
               (NEW.artifact = 'events'
                    AND archive.events_object_key = NEW.object_key
                    AND archive.events_version_id = NEW.object_version_id
                    AND archive.events_sha256 = NEW.sha256
                    AND archive.events_size_bytes = NEW.size_bytes)
               OR (NEW.artifact = 'manifest'
                    AND archive.manifest_object_key = NEW.object_key
                    AND archive.manifest_version_id = NEW.object_version_id
                    AND archive.manifest_sha256 = NEW.sha256
                    AND archive.manifest_size_bytes = NEW.size_bytes)
           )
    )
    THEN
        RAISE EXCEPTION 'audit export download is not exact and authorized'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER audit_export_downloads_validate_insert
BEFORE INSERT ON audit_export_downloads
FOR EACH ROW EXECUTE FUNCTION validate_audit_export_download_insert();

CREATE OR REPLACE FUNCTION reject_audit_export_download_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit export downloads are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_export_downloads_reject_update
BEFORE UPDATE ON audit_export_downloads
FOR EACH ROW EXECUTE FUNCTION reject_audit_export_download_mutation();

CREATE TRIGGER audit_export_downloads_reject_delete
BEFORE DELETE ON audit_export_downloads
FOR EACH ROW EXECUTE FUNCTION reject_audit_export_download_mutation();

CREATE TRIGGER audit_export_downloads_reject_truncate
BEFORE TRUNCATE ON audit_export_downloads
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_export_download_mutation();
