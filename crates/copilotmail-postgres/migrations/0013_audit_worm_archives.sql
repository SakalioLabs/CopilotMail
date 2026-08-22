INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('audit.archive.completed', 'audit.archive', 'critical', true);

ALTER TABLE audit_checkpoints
    ADD CONSTRAINT audit_checkpoints_tenant_id_unique
        UNIQUE(id, organization_id);

CREATE TABLE audit_archives (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    checkpoint_id uuid NOT NULL,
    provider text NOT NULL,
    bucket text NOT NULL,
    events_object_key text NOT NULL,
    events_version_id text NOT NULL,
    events_sha256 text NOT NULL,
    events_size_bytes bigint NOT NULL,
    manifest_object_key text NOT NULL,
    manifest_version_id text NOT NULL,
    manifest_sha256 text NOT NULL,
    manifest_size_bytes bigint NOT NULL,
    retention_mode text NOT NULL,
    retain_until timestamptz NOT NULL,
    verified_at timestamptz NOT NULL,
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    CONSTRAINT audit_archives_checkpoint_fk
        FOREIGN KEY(checkpoint_id, organization_id)
        REFERENCES audit_checkpoints(id, organization_id),
    CONSTRAINT audit_archives_creator_fk
        FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT audit_archives_provider_check
        CHECK (provider = 's3'),
    CONSTRAINT audit_archives_bucket_check
        CHECK (
            char_length(bucket) BETWEEN 1 AND 255
            AND bucket = btrim(bucket)
            AND bucket !~ '[[:cntrl:]]'
        ),
    CONSTRAINT audit_archives_object_keys_check
        CHECK (
            char_length(events_object_key) BETWEEN 1 AND 1024
            AND char_length(manifest_object_key) BETWEEN 1 AND 1024
            AND events_object_key <> manifest_object_key
            AND events_object_key = btrim(events_object_key)
            AND manifest_object_key = btrim(manifest_object_key)
            AND events_object_key !~ '[[:cntrl:]]'
            AND manifest_object_key !~ '[[:cntrl:]]'
        ),
    CONSTRAINT audit_archives_versions_check
        CHECK (
            char_length(events_version_id) BETWEEN 1 AND 1024
            AND char_length(manifest_version_id) BETWEEN 1 AND 1024
            AND events_version_id = btrim(events_version_id)
            AND manifest_version_id = btrim(manifest_version_id)
            AND events_version_id !~ '[[:cntrl:]]'
            AND manifest_version_id !~ '[[:cntrl:]]'
        ),
    CONSTRAINT audit_archives_digests_check
        CHECK (
            events_sha256 ~ '^[0-9a-f]{64}$'
            AND manifest_sha256 ~ '^[0-9a-f]{64}$'
        ),
    CONSTRAINT audit_archives_sizes_check
        CHECK (events_size_bytes > 0 AND manifest_size_bytes > 0),
    CONSTRAINT audit_archives_retention_check
        CHECK (
            retention_mode IN ('governance', 'compliance')
            AND retain_until > verified_at
            AND verified_at <= created_at
        ),
    CONSTRAINT audit_archives_target_unique
        UNIQUE(organization_id, bucket, events_object_key),
    CONSTRAINT audit_archives_manifest_target_unique
        UNIQUE(organization_id, bucket, manifest_object_key)
);

CREATE OR REPLACE FUNCTION reject_audit_archive_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit_archives is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_archives_append_only
BEFORE UPDATE OR DELETE ON audit_archives
FOR EACH ROW EXECUTE FUNCTION reject_audit_archive_mutation();

CREATE TRIGGER audit_archives_reject_truncate
BEFORE TRUNCATE ON audit_archives
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_truncate();

CREATE INDEX audit_archives_checkpoint_idx
    ON audit_archives(organization_id, checkpoint_id, created_at DESC);
