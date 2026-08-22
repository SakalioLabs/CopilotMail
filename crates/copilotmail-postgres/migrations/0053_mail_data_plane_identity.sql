-- Provider-neutral mail data-plane identity. This is intentionally a
-- stop-the-world migration: old API and Worker binaries must be stopped before
-- it runs, and only binaries that explicitly write the typed identity may be
-- started afterwards. Legacy source columns, checks, keys, and defaults remain
-- intact as immutable compatibility evidence and rollback buffer.

-- 0040 introduced the Agent mail-content scope and granted it to newly
-- bootstrapped Agent Operator roles, but did not expand the database permission
-- catalog. Existing deployments with an Agent Operator had no row to expose the
-- drift; the next organization bootstrap would fail closed. Keep the persisted
-- permission catalog aligned before any new tenant can be created.
ALTER TABLE role_permissions
    DROP CONSTRAINT role_permissions_permission_check;
ALTER TABLE role_permissions
    ADD CONSTRAINT role_permissions_permission_check CHECK (
        permission IN (
            'organization.read',
            'organization.manage',
            'identity.manage',
            'authorization.manage',
            'mail.admin',
            'security.admin',
            'audit.read',
            'audit.export',
            'legal_hold.manage',
            'retention.manage',
            'content.review',
            'agent.operate',
            'agent.mail.read',
            'mailbox.use',
            'break_glass.invoke'
        )
    );

-- Keep both Agent credential stores aligned with the fourth least-privilege
-- runtime scope introduced by 0040. Without this catalog repair, a newly
-- registered Agent that requests the documented token scope fails at the
-- database boundary even though the domain and RBAC catalogs allow it.
ALTER TABLE agent_private_key_credentials
    DROP CONSTRAINT agent_private_key_credentials_allowed_scopes_check;
ALTER TABLE agent_private_key_credentials
    ADD CONSTRAINT agent_private_key_credentials_allowed_scopes_check CHECK (
        cardinality(allowed_scopes) BETWEEN 1 AND 4
        AND allowed_scopes <@ ARRAY[
            'organization.read',
            'agent.operate',
            'agent.mail.read',
            'mailbox.use'
        ]::text[]
    );

ALTER TABLE agent_mtls_credentials
    DROP CONSTRAINT agent_mtls_credentials_allowed_scopes_check;
ALTER TABLE agent_mtls_credentials
    ADD CONSTRAINT agent_mtls_credentials_allowed_scopes_check CHECK (
        cardinality(allowed_scopes) BETWEEN 1 AND 4
        AND allowed_scopes <@ ARRAY[
            'organization.read',
            'agent.operate',
            'agent.mail.read',
            'mailbox.use'
        ]::text[]
    );

CREATE TABLE mail_data_planes (
    id uuid PRIMARY KEY,
    adapter_kind text NOT NULL,
    label text NOT NULL,
    created_at timestamptz NOT NULL,
    CONSTRAINT mail_data_planes_non_nil_id_check CHECK (
        id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    CONSTRAINT mail_data_planes_adapter_kind_check CHECK (
        char_length(adapter_kind) BETWEEN 1 AND 64
        AND adapter_kind = lower(adapter_kind)
        AND adapter_kind = btrim(adapter_kind)
        AND adapter_kind ~ '^[a-z][a-z0-9._-]{0,63}$'
    ),
    CONSTRAINT mail_data_planes_label_check CHECK (
        char_length(label) BETWEEN 1 AND 128
        AND label = btrim(label)
        AND label !~ '[[:cntrl:]]'
    )
);

INSERT INTO mail_data_planes(id, adapter_kind, label, created_at)
VALUES (
    '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    'stalwart_jmap',
    'Legacy Stalwart data plane',
    TIMESTAMPTZ '2026-08-12 00:00:00+00'
);

CREATE OR REPLACE FUNCTION reject_mail_data_plane_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail_data_planes is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_data_planes_reject_update_delete
BEFORE UPDATE OR DELETE ON mail_data_planes
FOR EACH ROW EXECUTE FUNCTION reject_mail_data_plane_mutation();

CREATE TRIGGER mail_data_planes_reject_truncate
BEFORE TRUNCATE ON mail_data_planes
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_data_plane_mutation();

-- An identity is permanently bound to one canonical network origin. The table
-- starts empty: the explicit migrate command performs the first bind after the
-- schema transaction commits; serve, worker, and check are read-only verifiers.
CREATE TABLE mail_data_plane_origin_bindings (
    data_plane_id uuid PRIMARY KEY REFERENCES mail_data_planes(id),
    canonical_origin text NOT NULL UNIQUE,
    bound_at timestamptz NOT NULL,
    CONSTRAINT mail_data_plane_origin_bindings_origin_shape_check CHECK (
        char_length(canonical_origin) BETWEEN 8 AND 512
        AND canonical_origin = btrim(canonical_origin)
        AND canonical_origin = lower(canonical_origin)
        AND canonical_origin !~ '[[:cntrl:]]'
        AND canonical_origin ~ '^https?://([a-z0-9]([a-z0-9.-]{0,250}[a-z0-9])?|\[[0-9a-f:.]+\])(:[0-9]{1,5})?$'
        AND canonical_origin !~ '(\.\.|\.-|-\.)'
    )
);

CREATE OR REPLACE FUNCTION validate_mail_data_plane_origin_binding_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    registry_created_at timestamptz;
    origin_match text[];
    origin_port integer;
    origin_host text;
    host_label text;
    parsed_address inet;
BEGIN
    SELECT created_at
      INTO registry_created_at
      FROM mail_data_planes
     WHERE id = NEW.data_plane_id
       AND id <> '00000000-0000-0000-0000-000000000000'::uuid;
    IF NOT FOUND OR NEW.bound_at < registry_created_at THEN
        RAISE EXCEPTION 'mail data-plane origin binding is invalid'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'mail_data_plane_origin_binding_registry_mismatch';
    END IF;

    origin_match := regexp_match(
        NEW.canonical_origin,
        '^(https?)://([a-z0-9]([a-z0-9.-]{0,250}[a-z0-9])?|\[[0-9a-f:.]+\])(:([0-9]{1,5}))?$'
    );
    IF origin_match IS NULL THEN
        RAISE EXCEPTION 'mail data-plane origin must be a canonical HTTP(S) origin'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'mail_data_plane_origin_binding_canonical';
    END IF;
    origin_host := origin_match[2];
    IF origin_host LIKE '[%]' THEN
        BEGIN
            parsed_address := substring(origin_host FROM 2 FOR char_length(origin_host) - 2)::inet;
        EXCEPTION WHEN invalid_text_representation THEN
            RAISE EXCEPTION 'mail data-plane IPv6 origin is invalid'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'mail_data_plane_origin_binding_host';
        END;
        IF family(parsed_address) <> 6
           OR host(parsed_address) <> substring(origin_host FROM 2 FOR char_length(origin_host) - 2)
        THEN
            RAISE EXCEPTION 'mail data-plane IPv6 origin is not canonical'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'mail_data_plane_origin_binding_host';
        END IF;
    ELSIF origin_host ~ '^[0-9.]+$' THEN
        BEGIN
            parsed_address := origin_host::inet;
        EXCEPTION WHEN invalid_text_representation THEN
            RAISE EXCEPTION 'mail data-plane IPv4 origin is invalid'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'mail_data_plane_origin_binding_host';
        END;
        IF family(parsed_address) <> 4 OR host(parsed_address) <> origin_host THEN
            RAISE EXCEPTION 'mail data-plane IPv4 origin is not canonical'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'mail_data_plane_origin_binding_host';
        END IF;
    ELSE
        FOREACH host_label IN ARRAY string_to_array(origin_host, '.') LOOP
            IF char_length(host_label) NOT BETWEEN 1 AND 63
               OR host_label !~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
            THEN
                RAISE EXCEPTION 'mail data-plane DNS origin is invalid'
                    USING ERRCODE = '23514',
                          CONSTRAINT = 'mail_data_plane_origin_binding_host';
            END IF;
        END LOOP;
    END IF;
    IF origin_match[5] IS NOT NULL THEN
        origin_port := origin_match[5]::integer;
        IF origin_port NOT BETWEEN 1 AND 65535
            OR origin_match[5] <> origin_port::text
            OR (origin_match[1] = 'http' AND origin_port = 80)
            OR (origin_match[1] = 'https' AND origin_port = 443)
        THEN
            RAISE EXCEPTION 'mail data-plane origin port is not canonical'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'mail_data_plane_origin_binding_port';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_data_plane_origin_bindings_validate_insert
BEFORE INSERT ON mail_data_plane_origin_bindings
FOR EACH ROW EXECUTE FUNCTION validate_mail_data_plane_origin_binding_insert();

CREATE OR REPLACE FUNCTION reject_mail_data_plane_origin_binding_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail_data_plane_origin_bindings is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_data_plane_origin_bindings_reject_update_delete
BEFORE UPDATE OR DELETE ON mail_data_plane_origin_bindings
FOR EACH ROW EXECUTE FUNCTION reject_mail_data_plane_origin_binding_mutation();

CREATE TRIGGER mail_data_plane_origin_bindings_reject_truncate
BEFORE TRUNCATE ON mail_data_plane_origin_bindings
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_data_plane_origin_binding_mutation();

-- Lock every mutable provenance root before the singleton legacy namespace is
-- backfilled. Any ambiguity or orphan below aborts the whole SQLx transaction.
LOCK TABLE mail_domains IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE mail_engine_mailbox_bindings IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE mail_engine_account_owners IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE inbound_mail_engine_events IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE outbound_mail_engine_events IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE mail_drafts IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE mail_submission_intents IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE mail_delivery_snapshots IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE agent_mail_admissions IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE outbox_events IN SHARE ROW EXCLUSIVE MODE;

ALTER TABLE mail_domains
    ADD COLUMN mail_data_plane_id uuid,
    ADD COLUMN mail_data_plane_assigned_at timestamptz,
    ADD CONSTRAINT mail_domains_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id);

UPDATE mail_domains
   SET mail_data_plane_id = '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
       mail_data_plane_assigned_at = GREATEST(
           created_at,
           COALESCE(activated_at, suspended_at, updated_at)
       )
 WHERE mail_engine_domain_ref IS NOT NULL
    OR status IN ('active', 'suspended');

ALTER TABLE mail_domains
    ADD CONSTRAINT mail_domains_data_plane_assignment_check CHECK (
        (mail_data_plane_id IS NULL AND mail_data_plane_assigned_at IS NULL)
        OR (
            mail_data_plane_id IS NOT NULL
            AND mail_data_plane_assigned_at IS NOT NULL
            AND mail_data_plane_assigned_at >= created_at
        )
    ),
    ADD CONSTRAINT mail_domains_engine_ref_data_plane_check CHECK (
        mail_engine_domain_ref IS NULL OR mail_data_plane_id IS NOT NULL
    ),
    ADD CONSTRAINT mail_domains_projection_data_plane_check CHECK (
        mail_engine_projection_status <> 'ready'
        OR (
            mail_data_plane_id IS NOT NULL
            AND mail_data_plane_assigned_at IS NOT NULL
            AND mail_engine_domain_ref IS NOT NULL
        )
    );

CREATE UNIQUE INDEX mail_domains_data_plane_engine_ref_unique
    ON mail_domains(mail_data_plane_id, mail_engine_domain_ref)
    WHERE mail_engine_domain_ref IS NOT NULL;

CREATE OR REPLACE FUNCTION guard_mail_domain_data_plane_identity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.mail_data_plane_id IS NOT NULL
       AND (
           NEW.mail_data_plane_id IS DISTINCT FROM OLD.mail_data_plane_id
           OR NEW.mail_data_plane_assigned_at IS DISTINCT FROM OLD.mail_data_plane_assigned_at
       )
    THEN
        RAISE EXCEPTION 'mail domain data-plane assignment is immutable'
            USING ERRCODE = '55000';
    END IF;
    IF OLD.mail_data_plane_id IS NULL
       AND (
           (NEW.mail_data_plane_id IS NULL) <> (NEW.mail_data_plane_assigned_at IS NULL)
           OR NEW.mail_data_plane_assigned_at < OLD.created_at
           OR NEW.mail_engine_domain_ref IS NOT NULL
       )
    THEN
        RAISE EXCEPTION 'mail domain data-plane assignment must be atomic'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.mail_engine_domain_ref IS NOT NULL
       AND NEW.mail_engine_domain_ref IS DISTINCT FROM OLD.mail_engine_domain_ref
    THEN
        RAISE EXCEPTION 'mail domain remote reference is immutable'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

-- A normal binding update may rotate the remote account within one stable
-- namespace, but it must never switch the mailbox to another namespace.
CREATE OR REPLACE FUNCTION guard_mail_engine_binding_data_plane()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.mail_data_plane_id IS DISTINCT FROM OLD.mail_data_plane_id THEN
        RAISE EXCEPTION 'mail-engine mailbox data plane is immutable'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

-- New immutable ingress evidence must be owned by the exact current binding.
-- Historical evidence remains valid after a later account rotation.
CREATE OR REPLACE FUNCTION validate_inbound_mail_engine_event_data_plane()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
         WHERE binding.organization_id = NEW.organization_id
           AND binding.mailbox_id = NEW.mailbox_id
           AND binding.mail_data_plane_id = NEW.mail_data_plane_id
           AND binding.source = NEW.source
           AND binding.account_ref = NEW.account_ref
    ) THEN
        RAISE EXCEPTION 'inbound event requires the exact current mailbox binding'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_outbound_mail_engine_event_data_plane()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
         WHERE binding.organization_id = NEW.organization_id
           AND binding.mailbox_id = NEW.mailbox_id
           AND binding.mail_data_plane_id = NEW.mail_data_plane_id
           AND binding.source = NEW.source
    ) THEN
        RAISE EXCEPTION 'outbound event requires the exact current mailbox binding'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

-- Admission quotas retain their authoritative 0042/0047 implementation. This
-- additional trigger proves that immutable admission evidence, its pending
-- Intent, and its frozen draft share one typed namespace.
CREATE OR REPLACE FUNCTION validate_agent_mail_admission_data_plane()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM mail_submission_intents AS intent
          JOIN mail_drafts AS draft
            ON draft.id = intent.draft_id
           AND draft.organization_id = intent.organization_id
         WHERE intent.id = NEW.submission_id
           AND intent.organization_id = NEW.organization_id
           AND intent.requested_by = NEW.agent_id
           AND intent.status = 'pending'
           AND intent.draft_id = NEW.draft_id
           AND intent.draft_version = NEW.draft_version
           AND intent.mail_data_plane_id = NEW.mail_data_plane_id
           AND draft.mail_data_plane_id = NEW.mail_data_plane_id
           AND draft.engine_source = intent.engine_source
           AND draft.engine_account_ref = intent.engine_account_ref
           AND draft.engine_email_ref = intent.engine_email_ref
           AND draft.status = 'frozen'
    ) THEN
        RAISE EXCEPTION 'Agent admission requires an exact same-plane pending Intent and draft'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_domains_guard_data_plane_identity
BEFORE UPDATE OF mail_data_plane_id, mail_data_plane_assigned_at, mail_engine_domain_ref
ON mail_domains
FOR EACH ROW EXECUTE FUNCTION guard_mail_domain_data_plane_identity();

ALTER TABLE mail_engine_mailbox_bindings
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT mail_engine_mailbox_bindings_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id),
    ADD CONSTRAINT mail_engine_mailbox_bindings_data_plane_account_unique
        UNIQUE(mail_data_plane_id, account_ref);

CREATE TRIGGER mail_engine_mailbox_bindings_guard_data_plane
BEFORE UPDATE OF mail_data_plane_id ON mail_engine_mailbox_bindings
FOR EACH ROW EXECUTE FUNCTION guard_mail_engine_binding_data_plane();

ALTER TABLE mail_engine_account_owners
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT mail_engine_account_owners_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id),
    ADD CONSTRAINT mail_engine_account_owners_data_plane_account_unique
        UNIQUE(mail_data_plane_id, account_ref);

ALTER TABLE inbound_mail_engine_events
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT inbound_mail_engine_events_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id),
    ADD CONSTRAINT inbound_mail_engine_events_data_plane_event_unique
        UNIQUE(mail_data_plane_id, source_event_id),
    ADD CONSTRAINT inbound_mail_engine_events_data_plane_message_unique
        UNIQUE(mail_data_plane_id, account_ref, document_ref);

ALTER TABLE outbound_mail_engine_events
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT outbound_mail_engine_events_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id),
    ADD CONSTRAINT outbound_mail_engine_events_data_plane_event_unique
        UNIQUE(mail_data_plane_id, source_event_id);

ALTER TABLE mail_drafts
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT mail_drafts_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id),
    ADD CONSTRAINT mail_drafts_data_plane_email_unique
        UNIQUE(mail_data_plane_id, engine_account_ref, engine_email_ref);

ALTER TABLE mail_submission_intents
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT mail_submission_intents_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id);

ALTER TABLE mail_delivery_snapshots
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT mail_delivery_snapshots_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id),
    ADD CONSTRAINT mail_delivery_snapshots_data_plane_source_unique
        UNIQUE(organization_id, submission_id, mail_data_plane_id, source_sha256);

ALTER TABLE agent_mail_admissions
    ADD COLUMN mail_data_plane_id uuid NOT NULL
        DEFAULT '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101',
    ADD CONSTRAINT agent_mail_admissions_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id);

-- Preserve the legacy thread digest byte-for-byte, but scope only the two
-- per-thread circuit-breaker lookups by the typed plane. Hourly, recipient,
-- and first-contact quotas deliberately remain cross-plane governance limits.
DO $$
DECLARE
    old_definition text;
    new_definition text;
BEGIN
    old_definition := pg_get_functiondef(
        'validate_agent_mail_admission_insert()'::regprocedure
    );
    new_definition := regexp_replace(
        old_definition,
        'AND\s+prior\.thread_ref\s*=\s*current_thread_ref',
        'AND prior.thread_ref = current_thread_ref
           AND prior.mail_data_plane_id = NEW.mail_data_plane_id',
        'g'
    );
    IF new_definition = old_definition THEN
        RAISE EXCEPTION 'authoritative Agent thread existence guard was not found'
            USING ERRCODE = '23514';
    END IF;
    old_definition := new_definition;
    new_definition := regexp_replace(
        old_definition,
        'AND\s+admission\.thread_ref\s*=\s*current_thread_ref',
        'AND admission.thread_ref = current_thread_ref
           AND admission.mail_data_plane_id = NEW.mail_data_plane_id',
        'g'
    );
    IF new_definition = old_definition THEN
        RAISE EXCEPTION 'authoritative Agent thread quota guard was not found'
            USING ERRCODE = '23514';
    END IF;
    EXECUTE new_definition;
END;
$$;

CREATE TRIGGER inbound_mail_engine_events_validate_data_plane
BEFORE INSERT ON inbound_mail_engine_events
FOR EACH ROW EXECUTE FUNCTION validate_inbound_mail_engine_event_data_plane();

CREATE TRIGGER outbound_mail_engine_events_validate_data_plane
BEFORE INSERT ON outbound_mail_engine_events
FOR EACH ROW EXECUTE FUNCTION validate_outbound_mail_engine_event_data_plane();

CREATE TRIGGER agent_mail_admissions_validate_data_plane
BEFORE INSERT ON agent_mail_admissions
FOR EACH ROW EXECUTE FUNCTION validate_agent_mail_admission_data_plane();

ALTER TABLE outbox_events
    ADD COLUMN mail_data_plane_id uuid,
    ADD CONSTRAINT outbox_events_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id);

-- Historical events are routed only through their authoritative aggregate.
-- Any malformed/orphaned required event remains NULL and fails the assertion
-- below instead of being silently guessed into the legacy namespace.
UPDATE outbox_events AS event
   SET mail_data_plane_id = binding.mail_data_plane_id
  FROM mail_engine_mailbox_bindings AS binding
 WHERE event.event_type IN (
        'mail_engine.mailbox_binding.updated.v1',
        'mail_engine.mailbox_binding.updated.v2'
   )
   AND event.aggregate_type = 'mail_engine.mailbox_binding'
   AND event.aggregate_id ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
   AND binding.mailbox_id = event.aggregate_id::uuid
   AND event.payload ->> 'organization_id' = binding.organization_id::text
   AND event.payload ->> 'mailbox_id' = binding.mailbox_id::text
   AND (
       (event.event_type = 'mail_engine.mailbox_binding.updated.v1'
        AND event.payload ->> 'source' = binding.source)
       OR (event.event_type = 'mail_engine.mailbox_binding.updated.v2'
           AND event.payload ->> 'data_plane_id' = binding.mail_data_plane_id::text)
   );

UPDATE outbox_events AS event
   SET mail_data_plane_id = inbound.mail_data_plane_id
  FROM inbound_mail_engine_events AS inbound
 WHERE event.event_type = 'message.received.v1'
   AND event.aggregate_type = 'message'
   AND event.id = inbound.event_id
   AND event.aggregate_id = inbound.message_id::text
   AND event.payload ->> 'organization_id' = inbound.organization_id::text
   AND event.payload ->> 'mailbox_id' = inbound.mailbox_id::text
   AND event.payload ->> 'message_id' = inbound.message_id::text;

UPDATE outbox_events AS event
   SET mail_data_plane_id = outbound.mail_data_plane_id
  FROM outbound_mail_engine_events AS outbound
 WHERE event.event_type = 'mail_engine.delivery_trace.observed.v1'
   AND event.aggregate_type = 'mail_engine.delivery_trace'
   AND event.id = outbound.event_id
   AND event.aggregate_id = outbound.event_id::text
   AND event.payload ->> 'organization_id' = outbound.organization_id::text
   AND event.payload ->> 'mailbox_id' = outbound.mailbox_id::text
   AND event.payload ->> 'trace_event_id' = outbound.event_id::text;

UPDATE outbox_events AS event
   SET mail_data_plane_id = submission.mail_data_plane_id
  FROM mail_submission_intents AS submission
 WHERE event.event_type IN (
        'mail.submission.requested.v1',
        'mail.submission.accepted.v1',
        'mail.submission.rejected.v1',
        'mail.submission.review_required.v1',
        'mail.delivery.reconciliation.requested.v1',
        'message.delivery_status_changed.v1'
   )
   AND event.aggregate_type = 'mail.submission'
   AND event.aggregate_id ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
   AND submission.id = event.aggregate_id::uuid
   AND event.payload ->> 'organization_id' = submission.organization_id::text
   AND event.payload ->> 'mailbox_id' = submission.mailbox_id::text
   AND event.payload ->> 'submission_id' = submission.id::text;

UPDATE outbox_events AS event
   SET mail_data_plane_id = admission.mail_data_plane_id
  FROM agent_mail_admissions AS admission
 WHERE event.event_type = 'agent.mail.admitted.v1'
   AND event.aggregate_type = 'agent.mail_admission'
   AND event.aggregate_id ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
   AND admission.id = event.aggregate_id::uuid
   AND event.payload ->> 'organization_id' = admission.organization_id::text
   AND event.payload ->> 'agent_id' = admission.agent_id::text
   AND event.payload ->> 'submission_id' = admission.submission_id::text;

UPDATE outbox_events AS event
   SET mail_data_plane_id = domain.mail_data_plane_id
  FROM mail_domains AS domain
 WHERE event.event_type IN ('mail_domain.activated.v1', 'mail_domain.suspended.v1')
   AND event.aggregate_type = 'mail_domain'
   AND event.aggregate_id ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
   AND domain.id = event.aggregate_id::uuid
   AND event.payload ->> 'organization_id' = domain.organization_id::text
   AND event.payload ->> 'mail_domain_id' = domain.id::text;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM outbox_events
         WHERE event_type IN (
             'mail_engine.mailbox_binding.updated.v1',
             'mail_engine.mailbox_binding.updated.v2',
             'message.received.v1',
             'mail_engine.delivery_trace.observed.v1',
             'mail.submission.requested.v1',
             'agent.mail.admitted.v1',
             'mail.submission.accepted.v1',
             'mail.submission.rejected.v1',
             'mail.submission.review_required.v1',
             'mail.delivery.reconciliation.requested.v1',
             'message.delivery_status_changed.v1',
             'mail_domain.activated.v1',
             'mail_domain.suspended.v1'
         )
           AND mail_data_plane_id IS NULL
    ) THEN
        RAISE EXCEPTION 'required mail outbox route cannot be proven'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'outbox_events_required_data_plane_route';
    END IF;
END;
$$;

ALTER TABLE outbox_events
    ADD CONSTRAINT outbox_events_required_data_plane_route_check CHECK (
        event_type NOT IN (
            'mail_engine.mailbox_binding.updated.v1',
            'mail_engine.mailbox_binding.updated.v2',
            'message.received.v1',
            'mail_engine.delivery_trace.observed.v1',
            'mail.submission.requested.v1',
            'agent.mail.admitted.v1',
            'mail.submission.accepted.v1',
            'mail.submission.rejected.v1',
            'mail.submission.review_required.v1',
            'mail.delivery.reconciliation.requested.v1',
            'message.delivery_status_changed.v1',
            'mail_domain.activated.v1',
            'mail_domain.suspended.v1'
        )
        OR mail_data_plane_id IS NOT NULL
    );

CREATE INDEX outbox_unpublished_data_plane_idx
    ON outbox_events(mail_data_plane_id, next_attempt_at, occurred_at, id)
    WHERE published_at IS NULL;

CREATE OR REPLACE FUNCTION validate_outbox_event_data_plane_route()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    aggregate_uuid uuid;
BEGIN
    IF NEW.event_type NOT IN (
        'mail_engine.mailbox_binding.updated.v1',
        'mail_engine.mailbox_binding.updated.v2',
        'message.received.v1',
        'mail_engine.delivery_trace.observed.v1',
        'mail.submission.requested.v1',
        'agent.mail.admitted.v1',
        'mail.submission.accepted.v1',
        'mail.submission.rejected.v1',
        'mail.submission.review_required.v1',
        'mail.delivery.reconciliation.requested.v1',
        'message.delivery_status_changed.v1',
        'mail_domain.activated.v1',
        'mail_domain.suspended.v1'
    ) THEN
        RETURN NEW;
    END IF;

    IF NEW.mail_data_plane_id IS NULL THEN
        RAISE EXCEPTION 'mail work outbox event requires a data-plane route'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.event_type IN (
        'mail_engine.mailbox_binding.updated.v1',
        'mail_engine.mailbox_binding.updated.v2'
    ) THEN
        IF NEW.aggregate_type <> 'mail_engine.mailbox_binding'
           OR NEW.aggregate_id !~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
        THEN
            RAISE EXCEPTION 'mailbox binding outbox aggregate is invalid'
                USING ERRCODE = '23514';
        END IF;
        aggregate_uuid := NEW.aggregate_id::uuid;
        IF NOT EXISTS (
            SELECT 1 FROM mail_engine_mailbox_bindings AS binding
             WHERE binding.mailbox_id = aggregate_uuid
               AND binding.mail_data_plane_id = NEW.mail_data_plane_id
               AND NEW.payload ->> 'organization_id' = binding.organization_id::text
               AND NEW.payload ->> 'mailbox_id' = binding.mailbox_id::text
               AND (
                   (NEW.event_type = 'mail_engine.mailbox_binding.updated.v1'
                    AND NEW.payload ->> 'source' = binding.source)
                   OR (NEW.event_type = 'mail_engine.mailbox_binding.updated.v2'
                       AND NEW.payload ->> 'data_plane_id' = NEW.mail_data_plane_id::text)
               )
        ) THEN
            RAISE EXCEPTION 'mailbox binding outbox route does not match its aggregate'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.event_type = 'message.received.v1' THEN
        IF NEW.aggregate_type <> 'message'
           OR NOT EXISTS (
               SELECT 1 FROM inbound_mail_engine_events AS inbound
                WHERE inbound.event_id = NEW.id
                  AND inbound.message_id::text = NEW.aggregate_id
                  AND inbound.mail_data_plane_id = NEW.mail_data_plane_id
                  AND NEW.payload ->> 'organization_id' = inbound.organization_id::text
                  AND NEW.payload ->> 'mailbox_id' = inbound.mailbox_id::text
                  AND NEW.payload ->> 'message_id' = inbound.message_id::text
           )
        THEN
            RAISE EXCEPTION 'message received outbox route does not match ingress evidence'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.event_type = 'mail_engine.delivery_trace.observed.v1' THEN
        IF NEW.aggregate_type <> 'mail_engine.delivery_trace'
           OR NEW.aggregate_id <> NEW.id::text
           OR NOT EXISTS (
               SELECT 1 FROM outbound_mail_engine_events AS outbound
                WHERE outbound.event_id = NEW.id
                  AND outbound.mail_data_plane_id = NEW.mail_data_plane_id
                  AND NEW.payload ->> 'organization_id' = outbound.organization_id::text
                  AND NEW.payload ->> 'mailbox_id' = outbound.mailbox_id::text
                  AND NEW.payload ->> 'trace_event_id' = outbound.event_id::text
           )
        THEN
            RAISE EXCEPTION 'delivery trace outbox route does not match ingress evidence'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.event_type IN (
        'mail.submission.requested.v1',
        'mail.submission.accepted.v1',
        'mail.submission.rejected.v1',
        'mail.submission.review_required.v1',
        'mail.delivery.reconciliation.requested.v1',
        'message.delivery_status_changed.v1'
    ) THEN
        IF NEW.aggregate_type <> 'mail.submission'
           OR NEW.aggregate_id !~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
        THEN
            RAISE EXCEPTION 'mail submission outbox aggregate is invalid'
                USING ERRCODE = '23514';
        END IF;
        aggregate_uuid := NEW.aggregate_id::uuid;
        IF NOT EXISTS (
            SELECT 1 FROM mail_submission_intents AS submission
             WHERE submission.id = aggregate_uuid
               AND submission.mail_data_plane_id = NEW.mail_data_plane_id
               AND NEW.payload ->> 'organization_id' = submission.organization_id::text
               AND NEW.payload ->> 'mailbox_id' = submission.mailbox_id::text
               AND NEW.payload ->> 'submission_id' = submission.id::text
        ) THEN
            RAISE EXCEPTION 'mail submission outbox route does not match its Intent'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.event_type = 'agent.mail.admitted.v1' THEN
        IF NEW.aggregate_type <> 'agent.mail_admission'
           OR NEW.aggregate_id !~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
        THEN
            RAISE EXCEPTION 'Agent admission outbox aggregate is invalid'
                USING ERRCODE = '23514';
        END IF;
        aggregate_uuid := NEW.aggregate_id::uuid;
        IF NOT EXISTS (
            SELECT 1 FROM agent_mail_admissions AS admission
             WHERE admission.id = aggregate_uuid
               AND admission.mail_data_plane_id = NEW.mail_data_plane_id
               AND NEW.payload ->> 'organization_id' = admission.organization_id::text
               AND NEW.payload ->> 'agent_id' = admission.agent_id::text
               AND NEW.payload ->> 'submission_id' = admission.submission_id::text
        ) THEN
            RAISE EXCEPTION 'Agent admission outbox route does not match evidence'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        IF NEW.aggregate_type <> 'mail_domain'
           OR NEW.aggregate_id !~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
        THEN
            RAISE EXCEPTION 'mail domain outbox aggregate is invalid'
                USING ERRCODE = '23514';
        END IF;
        aggregate_uuid := NEW.aggregate_id::uuid;
        IF NOT EXISTS (
            SELECT 1 FROM mail_domains AS domain
             WHERE domain.id = aggregate_uuid
               AND domain.mail_data_plane_id = NEW.mail_data_plane_id
               AND NEW.payload ->> 'organization_id' = domain.organization_id::text
               AND NEW.payload ->> 'mail_domain_id' = domain.id::text
        ) THEN
            RAISE EXCEPTION 'mail domain outbox route does not match its assignment'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER outbox_events_validate_data_plane_route
BEFORE INSERT ON outbox_events
FOR EACH ROW EXECUTE FUNCTION validate_outbox_event_data_plane_route();

CREATE OR REPLACE FUNCTION guard_outbox_event_identity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.event_type IS DISTINCT FROM OLD.event_type
        OR NEW.aggregate_type IS DISTINCT FROM OLD.aggregate_type
        OR NEW.aggregate_id IS DISTINCT FROM OLD.aggregate_id
        OR NEW.schema_version IS DISTINCT FROM OLD.schema_version
        OR NEW.payload IS DISTINCT FROM OLD.payload
        OR NEW.occurred_at IS DISTINCT FROM OLD.occurred_at
        OR NEW.mail_data_plane_id IS DISTINCT FROM OLD.mail_data_plane_id
    THEN
        RAISE EXCEPTION 'outbox event identity and route are immutable'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER outbox_events_guard_identity
BEFORE UPDATE ON outbox_events
FOR EACH ROW EXECUTE FUNCTION guard_outbox_event_identity();

-- 0042 is the authoritative draft-insert policy. Preserve Human/Agent
-- eligibility and recipient-policy checks while changing only affinity.
CREATE OR REPLACE FUNCTION validate_mail_draft_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.version <> 1 OR NEW.status <> 'active' THEN
        RAISE EXCEPTION 'new mail draft must start active at version one'
            USING ERRCODE = '23514';
    END IF;
    IF NOT valid_mail_draft_recipient_evidence(
        NEW.recipient_refs,
        NEW.recipient_domains
    ) THEN
        RAISE EXCEPTION 'mail draft recipient evidence is invalid'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS principal
          LEFT JOIN agent_profiles AS agent
            ON agent.principal_id = principal.id
         WHERE principal.id = NEW.created_by
           AND principal.organization_id = NEW.organization_id
           AND principal.status = 'active'
           AND (
               principal.kind = 'human'
               OR (
                   principal.kind = 'agent'
                   AND (
                       agent.environment IN ('development', 'testing')
                       OR (
                           agent.environment = 'production'
                           AND EXISTS (
                               SELECT 1
                                 FROM agent_mail_policy_activations AS activation
                                WHERE activation.organization_id = NEW.organization_id
                                  AND activation.agent_id = NEW.created_by
                           )
                       )
                   )
               )
           )
    ) THEN
        RAISE EXCEPTION 'mail draft creator is not eligible for outbound mail'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
         WHERE binding.organization_id = NEW.organization_id
           AND binding.mailbox_id = NEW.mailbox_id
           AND binding.mail_data_plane_id = NEW.mail_data_plane_id
           AND binding.source = NEW.engine_source
           AND binding.account_ref = NEW.engine_account_ref
    ) THEN
        RAISE EXCEPTION 'mail draft engine account must match the current mailbox binding'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.reply_to_message_id IS NOT NULL
       AND NOT EXISTS (
           SELECT 1
             FROM inbound_mail_engine_events AS inbound
            WHERE inbound.message_id = NEW.reply_to_message_id
              AND inbound.organization_id = NEW.organization_id
              AND inbound.mailbox_id = NEW.mailbox_id
              AND inbound.mail_data_plane_id = NEW.mail_data_plane_id
       )
    THEN
        RAISE EXCEPTION 'mail draft reply target must use the same data plane'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

-- 0047 is the authoritative draft-update policy, including explicit-reply
-- and automatic-reply loop evidence.
CREATE OR REPLACE FUNCTION validate_mail_draft_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
        OR NEW.organization_id <> OLD.organization_id
        OR NEW.mailbox_id <> OLD.mailbox_id
        OR NEW.created_by <> OLD.created_by
        OR NEW.mail_data_plane_id <> OLD.mail_data_plane_id
        OR NEW.engine_source <> OLD.engine_source
        OR NEW.engine_account_ref <> OLD.engine_account_ref
        OR NEW.reply_to_message_id IS DISTINCT FROM OLD.reply_to_message_id
        OR NEW.reply_to_automatic <> OLD.reply_to_automatic
        OR NEW.created_at <> OLD.created_at
        OR NEW.version <> OLD.version + 1
        OR NEW.updated_at < OLD.updated_at
        OR NOT valid_mail_draft_recipient_evidence(
            NEW.recipient_refs,
            NEW.recipient_domains
        )
    THEN
        RAISE EXCEPTION 'mail draft identity, reply source, evidence, or version transition is invalid'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.status = 'active' AND NEW.status = 'active' THEN
        IF NEW.engine_email_ref = OLD.engine_email_ref
            OR NEW.content_sha256 = OLD.content_sha256
        THEN
            RAISE EXCEPTION 'active draft replacement must advance remote identity and content'
                USING ERRCODE = '23514';
        END IF;
    ELSIF (OLD.status = 'active' AND NEW.status = 'frozen')
        OR (OLD.status = 'frozen' AND NEW.status = 'sent')
    THEN
        IF NEW.engine_email_ref <> OLD.engine_email_ref
            OR NEW.engine_thread_ref <> OLD.engine_thread_ref
            OR NEW.reply_to_message_id IS DISTINCT FROM OLD.reply_to_message_id
            OR NEW.reply_to_automatic <> OLD.reply_to_automatic
            OR NEW.body_ref <> OLD.body_ref
            OR NEW.content_sha256 <> OLD.content_sha256
            OR NEW.recipient_refs <> OLD.recipient_refs
            OR NEW.recipient_domains <> OLD.recipient_domains
        THEN
            RAISE EXCEPTION 'draft lifecycle transition cannot change frozen evidence'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        RAISE EXCEPTION 'mail draft status transition is invalid'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

-- 0047 is also the authoritative submission-insert policy. Keep Agent
-- admission, sandbox, duplicate-content, and automatic-reply guards intact.
CREATE OR REPLACE FUNCTION validate_mail_submission_intent_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    current_body_ref bytea;
BEGIN
    IF NEW.status <> 'pending'
        OR NEW.state_version <> 1
        OR NEW.attempt_count <> 0
        OR NEW.lease_id IS NOT NULL
        OR NEW.engine_identity_ref IS NOT NULL
        OR NEW.engine_submission_ref IS NOT NULL
        OR NEW.error_code IS NOT NULL
        OR NEW.accepted_at IS NOT NULL
    THEN
        RAISE EXCEPTION 'new mail submission intent must start pending'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS principal
          LEFT JOIN agent_profiles AS agent
            ON agent.principal_id = principal.id
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.status = 'active'
           AND (
               principal.kind = 'human'
               OR (
                   principal.kind = 'agent'
                   AND (
                       agent.environment IN ('development', 'testing')
                       OR (
                           agent.environment = 'production'
                           AND EXISTS (
                               SELECT 1
                                 FROM agent_mail_policy_activations AS activation
                                WHERE activation.organization_id = NEW.organization_id
                                  AND activation.agent_id = NEW.requested_by
                           )
                       )
                   )
               )
           )
    ) THEN
        RAISE EXCEPTION 'mail submission requester is not eligible'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1
          FROM principals AS principal
          JOIN agent_profiles AS profile
            ON profile.principal_id = principal.id
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'agent'
           AND profile.environment = 'testing'
    ) THEN
        RAISE EXCEPTION 'testing Agent must use the non-delivering sandbox'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_testing_requires_sandbox';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM mail_drafts AS draft
         WHERE draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id
           AND draft.mailbox_id = NEW.mailbox_id
           AND draft.created_by = NEW.requested_by
           AND draft.version = NEW.draft_version
           AND draft.status = 'frozen'
           AND draft.mail_data_plane_id = NEW.mail_data_plane_id
           AND draft.engine_source = NEW.engine_source
           AND draft.engine_account_ref = NEW.engine_account_ref
           AND draft.engine_email_ref = NEW.engine_email_ref
    ) THEN
        RAISE EXCEPTION 'mail submission intent must bind one exact frozen owned draft'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1
          FROM principals AS principal
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'agent'
    ) THEN
        PERFORM pg_advisory_xact_lock(
            hashtextextended(
                NEW.organization_id::text || ':' || NEW.requested_by::text,
                0
            )
        );
        SELECT draft.body_ref
          INTO STRICT current_body_ref
          FROM mail_drafts AS draft
         WHERE draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id;
        IF (
            SELECT count(*)
              FROM mail_submission_intents AS prior
              JOIN mail_drafts AS prior_draft
                ON prior_draft.id = prior.draft_id
               AND prior_draft.organization_id = prior.organization_id
             WHERE prior.organization_id = NEW.organization_id
               AND prior.requested_by = NEW.requested_by
               AND prior.created_at > NEW.created_at - interval '1 day'
               AND prior.created_at <= NEW.created_at
               AND prior_draft.body_ref = current_body_ref
        ) >= 10
        THEN
            RAISE EXCEPTION 'Agent duplicate content circuit breaker opened'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'agent_mail_duplicate_content_circuit_breaker';
        END IF;
    END IF;
    IF EXISTS (
        SELECT 1
          FROM principals AS principal
          JOIN mail_drafts AS draft
            ON draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'agent'
           AND draft.reply_to_automatic
    ) THEN
        RAISE EXCEPTION 'Agent automatic reply loop guard rejected submission'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_automatic_reply_loop';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_mail_submission_intent_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
        OR NEW.organization_id <> OLD.organization_id
        OR NEW.requested_by <> OLD.requested_by
        OR NEW.mailbox_id <> OLD.mailbox_id
        OR NEW.draft_id <> OLD.draft_id
        OR NEW.draft_version <> OLD.draft_version
        OR NEW.idempotency_key_sha256 <> OLD.idempotency_key_sha256
        OR NEW.request_sha256 <> OLD.request_sha256
        OR NEW.mail_data_plane_id <> OLD.mail_data_plane_id
        OR NEW.engine_source <> OLD.engine_source
        OR NEW.engine_account_ref <> OLD.engine_account_ref
        OR NEW.engine_email_ref <> OLD.engine_email_ref
        OR NEW.created_at <> OLD.created_at
        OR NEW.state_version <> OLD.state_version + 1
        OR NEW.updated_at < OLD.updated_at
        OR NEW.attempt_count < OLD.attempt_count
    THEN
        RAISE EXCEPTION 'mail submission intent identity or version transition is invalid'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.status = 'pending'
        AND NEW.status NOT IN ('pending', 'reconciling', 'rejected', 'manual_review')
    THEN
        RAISE EXCEPTION 'pending mail submission transition is invalid'
            USING ERRCODE = '23514';
    ELSIF OLD.status = 'reconciling'
        AND NEW.status NOT IN ('reconciling', 'accepted', 'rejected', 'manual_review')
    THEN
        RAISE EXCEPTION 'reconciling mail submission transition is invalid'
            USING ERRCODE = '23514';
    ELSIF OLD.status IN ('accepted', 'rejected', 'manual_review') THEN
        RAISE EXCEPTION 'terminal mail submission intent is immutable'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_mail_delivery_snapshot_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM mail_submission_intents AS submission
         WHERE submission.id = NEW.submission_id
           AND submission.organization_id = NEW.organization_id
           AND submission.status = 'accepted'
           AND submission.mail_data_plane_id = NEW.mail_data_plane_id
           AND submission.engine_source = 'stalwart'
           AND submission.engine_identity_ref IS NOT NULL
           AND submission.engine_submission_ref IS NOT NULL
           AND submission.accepted_at IS NOT NULL
           AND NEW.observed_at >= submission.accepted_at
    ) THEN
        RAISE EXCEPTION 'delivery evidence requires an accepted bound submission'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

-- Permanent account ownership now carries the typed plane while the legacy
-- source key remains enforced during this stop-the-world compatibility phase.
DROP TRIGGER mail_engine_mailbox_bindings_enforce_account_owner
    ON mail_engine_mailbox_bindings;

CREATE OR REPLACE FUNCTION enforce_mail_engine_account_owner()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    claimed_rows bigint;
    owner_organization_id uuid;
    owner_mailbox_id uuid;
BEGIN
    INSERT INTO mail_engine_account_owners(
        mail_data_plane_id,
        source,
        account_ref,
        organization_id,
        mailbox_id,
        first_bound_at
    )
    VALUES (
        NEW.mail_data_plane_id,
        NEW.source,
        NEW.account_ref,
        NEW.organization_id,
        NEW.mailbox_id,
        CASE WHEN TG_OP = 'INSERT' THEN NEW.created_at ELSE NEW.updated_at END
    )
    -- The legacy key remains authoritative until a later contract migration.
    -- A same-source account collision is caught here, then the typed lookup
    -- below turns any namespace/owner mismatch into a stable fail-closed error.
    ON CONFLICT(source, account_ref) DO NOTHING;

    GET DIAGNOSTICS claimed_rows = ROW_COUNT;
    IF claimed_rows = 0 THEN
        SELECT organization_id, mailbox_id
          INTO owner_organization_id, owner_mailbox_id
          FROM mail_engine_account_owners
         WHERE mail_data_plane_id = NEW.mail_data_plane_id
           AND account_ref = NEW.account_ref;

        IF owner_organization_id IS NULL
            OR owner_organization_id <> NEW.organization_id
            OR owner_mailbox_id <> NEW.mailbox_id
        THEN
            RAISE EXCEPTION
                'mail-engine account ownership cannot be reassigned'
                USING
                    ERRCODE = '23514',
                    CONSTRAINT = 'mail_engine_account_owner_mismatch';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_engine_mailbox_bindings_enforce_account_owner
BEFORE INSERT OR UPDATE OF mail_data_plane_id, source, account_ref, organization_id, mailbox_id
ON mail_engine_mailbox_bindings
FOR EACH ROW EXECUTE FUNCTION enforce_mail_engine_account_owner();

CREATE OR REPLACE FUNCTION require_current_binding_for_account_owner()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
         WHERE binding.mail_data_plane_id = NEW.mail_data_plane_id
           AND binding.source = NEW.source
           AND binding.account_ref = NEW.account_ref
           AND binding.organization_id = NEW.organization_id
           AND binding.mailbox_id = NEW.mailbox_id
           AND NEW.first_bound_at IN (binding.created_at, binding.updated_at)
    ) THEN
        RAISE EXCEPTION
            'mail-engine account owner insert requires the exact current binding'
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'mail_engine_account_owner_insert_without_binding';
    END IF;

    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
          LEFT JOIN mail_engine_account_owners AS owner
            ON owner.mail_data_plane_id = binding.mail_data_plane_id
           AND owner.source = binding.source
           AND owner.account_ref = binding.account_ref
         WHERE owner.account_ref IS NULL
            OR owner.organization_id <> binding.organization_id
            OR owner.mailbox_id <> binding.mailbox_id
    ) THEN
        RAISE EXCEPTION
            'current mail-engine bindings are not exactly covered by permanent ownership'
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'mail_engine_account_owner_current_binding_mismatch';
    END IF;
END;
$$;
