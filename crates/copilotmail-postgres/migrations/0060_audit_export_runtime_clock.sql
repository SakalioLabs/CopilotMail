-- Database-owned export timestamps close stale/backdated caller-clock windows.
-- Existing immutable grants/downloads remain untouched; no history is rewritten.

CREATE OR REPLACE FUNCTION validate_audit_export_grant_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    -- Match the service lock order: actor before the exact approval.
    PERFORM id FROM principals
     WHERE id = NEW.requested_by AND organization_id = NEW.organization_id
     FOR UPDATE;
    PERFORM id FROM approval_requests
     WHERE id = NEW.approval_request_id AND organization_id = NEW.organization_id
     FOR UPDATE;
    NEW.created_at := clock_timestamp();
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS actor
         WHERE actor.id = NEW.requested_by
           AND actor.organization_id = NEW.organization_id
           AND actor.kind = 'human'
           AND actor.status = 'active'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM audit_archives AS archive
         WHERE archive.id = NEW.archive_id
           AND archive.organization_id = NEW.organization_id
    )
    OR NOT EXISTS (
        SELECT 1
          FROM approval_requests AS request
         WHERE request.id = NEW.approval_request_id
           AND request.organization_id = NEW.organization_id
           AND request.action = 'audit.export'
           AND request.resource_kind = 'audit.archive'
           AND request.resource_id = NEW.archive_id::text
           AND request.requested_by = NEW.requested_by
           AND request.status = 'approved'
           AND request.required_approvals >= 2
           AND request.created_at <= NEW.created_at
           AND request.expires_at > NEW.created_at
           AND NEW.expires_at <= request.expires_at
           AND (
               SELECT count(*)
                 FROM approval_steps AS step
                WHERE step.approval_request_id = request.id
                  AND step.organization_id = request.organization_id
                  AND step.decision = 'approved'
           ) >= request.required_approvals
           AND NOT EXISTS (
               SELECT 1
                 FROM approval_steps AS step
                WHERE step.approval_request_id = request.id
                  AND step.organization_id = request.organization_id
                  AND step.decision = 'rejected'
           )
    )
    THEN
        RAISE EXCEPTION 'audit export grant is not exact and authorized'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_audit_export_download_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM id FROM principals
     WHERE id = NEW.downloaded_by AND organization_id = NEW.organization_id
     FOR UPDATE;
    -- The grant and archive are immutable. Time must be sampled after the
    -- mutable actor lock, never taken from a caller or transaction start.
    NEW.downloaded_at := clock_timestamp();
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
