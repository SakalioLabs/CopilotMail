CREATE EXTENSION IF NOT EXISTS pgcrypto;

ALTER TABLE audit_events
    ADD COLUMN chain_sequence bigint,
    ADD COLUMN hash_version smallint,
    ADD COLUMN hash_payload text;

CREATE OR REPLACE FUNCTION build_audit_event_hash_payload(p_event audit_events)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
    SELECT jsonb_build_object(
        'action', p_event.action,
        'actor_principal_id', p_event.actor_principal_id,
        'approval_request_id', p_event.approval_request_id,
        'chain_sequence', p_event.chain_sequence,
        'client_id', p_event.client_id,
        'credential_id', p_event.credential_id,
        'details', p_event.details,
        'environment', p_event.environment,
        'hash_version', p_event.hash_version,
        'id', p_event.id,
        'on_behalf_of_principal_id', p_event.on_behalf_of_principal_id,
        'organization_id', p_event.organization_id,
        'outcome', p_event.outcome,
        'occurred_at', p_event.occurred_at,
        'policy_id', p_event.policy_id,
        'policy_version_id', p_event.policy_version_id,
        'received_at', p_event.received_at,
        'region', p_event.region,
        'request_id', p_event.request_id,
        'resource_id', p_event.resource_id,
        'resource_type', p_event.resource_type,
        'session_id', p_event.session_id,
        'source_ip', p_event.source_ip,
        'subject_principal_id', p_event.subject_principal_id,
        'ticket_reference', p_event.ticket_reference,
        'token_jti', p_event.token_jti,
        'trace_id', p_event.trace_id
    )::text
$$;

-- Existing installations may already contain audit events. Assign a stable
-- per-tenant order before calculating the first version of the chain.
ALTER TABLE audit_events DISABLE TRIGGER audit_events_append_only;

WITH ordered AS (
    SELECT id,
           row_number() OVER (
               PARTITION BY organization_id
               ORDER BY occurred_at, id
           ) AS sequence
      FROM audit_events
)
UPDATE audit_events AS event
   SET chain_sequence = ordered.sequence,
       hash_version = 1
  FROM ordered
 WHERE event.id = ordered.id;

UPDATE audit_events
   SET hash_payload = build_audit_event_hash_payload(audit_events);

WITH RECURSIVE chain AS (
    SELECT event.id,
           event.organization_id,
           event.chain_sequence,
           NULL::bytea AS previous_hash,
           digest(convert_to(event.hash_payload, 'UTF8'), 'sha256') AS event_hash
      FROM audit_events AS event
     WHERE event.chain_sequence = 1

    UNION ALL

    SELECT event.id,
           event.organization_id,
           event.chain_sequence,
           chain.event_hash,
           digest(
               chain.event_hash || convert_to(event.hash_payload, 'UTF8'),
               'sha256'
           )
      FROM chain
      JOIN audit_events AS event
        ON event.organization_id = chain.organization_id
       AND event.chain_sequence = chain.chain_sequence + 1
)
UPDATE audit_events AS event
   SET previous_hash = chain.previous_hash,
       event_hash = chain.event_hash
  FROM chain
 WHERE event.id = chain.id;

ALTER TABLE audit_events ENABLE TRIGGER audit_events_append_only;

ALTER TABLE audit_events
    ALTER COLUMN chain_sequence SET NOT NULL,
    ALTER COLUMN hash_version SET NOT NULL,
    ALTER COLUMN hash_payload SET NOT NULL,
    ALTER COLUMN event_hash SET NOT NULL,
    ADD CONSTRAINT audit_events_chain_sequence_positive
        CHECK (chain_sequence > 0),
    ADD CONSTRAINT audit_events_hash_version_supported
        CHECK (hash_version = 1),
    ADD CONSTRAINT audit_events_previous_hash_length
        CHECK (previous_hash IS NULL OR octet_length(previous_hash) = 32),
    ADD CONSTRAINT audit_events_event_hash_length
        CHECK (octet_length(event_hash) = 32),
    ADD CONSTRAINT audit_events_tenant_chain_sequence_unique
        UNIQUE(organization_id, chain_sequence);

CREATE OR REPLACE FUNCTION chain_audit_event_before_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    prior_sequence bigint;
    prior_hash bytea;
BEGIN
    IF NEW.chain_sequence IS NOT NULL
       OR NEW.hash_version IS NOT NULL
       OR NEW.hash_payload IS NOT NULL
       OR NEW.previous_hash IS NOT NULL
       OR NEW.event_hash IS NOT NULL THEN
        RAISE EXCEPTION 'audit hash metadata is database-managed'
            USING ERRCODE = '22023';
    END IF;

    -- Serialize only writers for this tenant. Different tenants remain
    -- independent and can append concurrently.
    PERFORM pg_advisory_xact_lock(
        hashtextextended('copilotmail.audit.' || NEW.organization_id::text, 0)
    );

    SELECT event.chain_sequence, event.event_hash
      INTO prior_sequence, prior_hash
      FROM audit_events AS event
     WHERE event.organization_id = NEW.organization_id
     ORDER BY event.chain_sequence DESC
     LIMIT 1;

    NEW.chain_sequence := COALESCE(prior_sequence, 0) + 1;
    NEW.hash_version := 1;
    NEW.previous_hash := prior_hash;
    NEW.hash_payload := build_audit_event_hash_payload(NEW);
    NEW.event_hash := digest(
        COALESCE(NEW.previous_hash, ''::bytea)
            || convert_to(NEW.hash_payload, 'UTF8'),
        'sha256'
    );
    RETURN NEW;
END;
$$;

CREATE TRIGGER audit_events_hash_chain
BEFORE INSERT ON audit_events
FOR EACH ROW EXECUTE FUNCTION chain_audit_event_before_insert();

CREATE INDEX audit_events_chain_head_idx
    ON audit_events(organization_id, chain_sequence DESC);

CREATE OR REPLACE FUNCTION reject_audit_truncate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit data is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_events_reject_truncate
BEFORE TRUNCATE ON audit_events
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_truncate();

CREATE TRIGGER audit_event_catalog_reject_truncate
BEFORE TRUNCATE ON audit_event_catalog
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_truncate();
