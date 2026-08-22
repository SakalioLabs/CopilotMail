-- Migration 0051 established permanent provider-account ownership. This
-- follow-up closes the remaining direct-INSERT boundary without changing the
-- checksum of the already-applied migration.
--
-- The binding trigger writes owners while a binding row is still being
-- inserted or updated, so an immediate owner-table check cannot distinguish
-- that trusted nested write from a standalone INSERT. A deferred constraint
-- trigger instead validates the committed transaction shape: every newly
-- claimed owner must have the exact current binding, including a timestamp
-- produced by that binding row.

-- The normal binding write order is bindings -> owners. Acquire locks in the
-- same order, wait for all in-flight writers, and retain both locks until the
-- history repair and INSERT guard become visible together at commit.
LOCK TABLE mail_engine_mailbox_bindings IN SHARE ROW EXCLUSIVE MODE;
LOCK TABLE mail_engine_account_owners IN SHARE ROW EXCLUSIVE MODE;

-- Successful binding audit events are produced atomically with the binding
-- write. Validate their actual persisted JSON shape before extracting values;
-- malformed evidence must fail with a governed check violation rather than a
-- cast error or a silently incomplete ownership ledger.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM audit_events AS event
         WHERE event.action = 'mail_engine.mailbox_binding.updated'
           AND event.resource_type = 'mail_engine.mailbox_binding'
           AND event.outcome = 'success'
           AND (
               jsonb_typeof(event.details) IS DISTINCT FROM 'object'
               OR jsonb_typeof(event.details -> 'mailbox_id')
                   IS DISTINCT FROM 'string'
               OR jsonb_typeof(event.details -> 'source')
                   IS DISTINCT FROM 'string'
               OR jsonb_typeof(event.details -> 'account_ref')
                   IS DISTINCT FROM 'string'
               OR jsonb_typeof(event.details -> 'version')
                   IS DISTINCT FROM 'number'
               OR event.details ->> 'mailbox_id' IS DISTINCT FROM event.resource_id
               OR event.resource_id
                   !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
               OR event.details ->> 'source' <> 'stalwart'
               OR event.details ->> 'account_ref'
                   !~ '^(0|[1-9][0-9]{0,19})$'
               OR event.details ->> 'version' !~ '^[1-9][0-9]*$'
           )
    ) THEN
        RAISE EXCEPTION
            'mail-engine account ownership history contains malformed audit evidence'
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'mail_engine_account_owner_history_malformed';
    END IF;

    -- The textual UUIDs above are now safe to cast. Tenant/mailbox mismatch is
    -- evidence corruption and cannot be repaired by choosing either side.
    IF EXISTS (
        SELECT 1
          FROM audit_events AS event
         WHERE event.action = 'mail_engine.mailbox_binding.updated'
           AND event.resource_type = 'mail_engine.mailbox_binding'
           AND event.outcome = 'success'
           AND NOT EXISTS (
               SELECT 1
                 FROM mailboxes AS mailbox
                WHERE mailbox.id = event.resource_id::uuid
                  AND mailbox.organization_id = event.organization_id
           )
    ) THEN
        RAISE EXCEPTION
            'mail-engine account ownership history references a foreign mailbox'
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'mail_engine_account_owner_history_tenant_mismatch';
    END IF;
END;
$$;

CREATE TEMPORARY TABLE mail_engine_account_owner_evidence_raw (
    source text NOT NULL,
    account_ref text NOT NULL,
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    first_bound_at timestamptz NOT NULL
) ON COMMIT DROP;

-- A current binding is trusted evidence even if it predates audit production.
-- updated_at is the only persisted time that identifies the current account
-- after an in-place binding rotation.
INSERT INTO mail_engine_account_owner_evidence_raw(
    source,
    account_ref,
    organization_id,
    mailbox_id,
    first_bound_at
)
SELECT source, account_ref, organization_id, mailbox_id, updated_at
  FROM mail_engine_mailbox_bindings;

-- Append-only, hash-chained successful audit events recover every account that
-- was retired before the current binding snapshot.
INSERT INTO mail_engine_account_owner_evidence_raw(
    source,
    account_ref,
    organization_id,
    mailbox_id,
    first_bound_at
)
SELECT event.details ->> 'source',
       event.details ->> 'account_ref',
       event.organization_id,
       event.resource_id::uuid,
       event.occurred_at
  FROM audit_events AS event
 WHERE event.action = 'mail_engine.mailbox_binding.updated'
   AND event.resource_type = 'mail_engine.mailbox_binding'
   AND event.outcome = 'success';

DO $$
BEGIN
    IF EXISTS (
        SELECT source, account_ref
          FROM mail_engine_account_owner_evidence_raw
         GROUP BY source, account_ref
        HAVING COUNT(DISTINCT (organization_id, mailbox_id)) > 1
    ) THEN
        RAISE EXCEPTION
            'historical mail-engine account ownership is ambiguous'
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'mail_engine_account_owner_history_conflict';
    END IF;
END;
$$;

CREATE TEMPORARY TABLE mail_engine_account_owner_expected (
    source text NOT NULL,
    account_ref text NOT NULL,
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    first_bound_at timestamptz NOT NULL,
    PRIMARY KEY(source, account_ref)
) ON COMMIT DROP;

INSERT INTO mail_engine_account_owner_expected(
    source,
    account_ref,
    organization_id,
    mailbox_id,
    first_bound_at
)
SELECT source,
       account_ref,
       organization_id,
       mailbox_id,
       MIN(first_bound_at)
  FROM mail_engine_account_owner_evidence_raw
 GROUP BY source, account_ref, organization_id, mailbox_id;

-- Never overwrite immutable ownership. An existing row that disagrees with
-- trusted history, including its first-bound timestamp, requires forensic
-- reconciliation and an explicit future migration.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM mail_engine_account_owners AS owner
          JOIN mail_engine_account_owner_expected AS expected
            ON expected.source = owner.source
           AND expected.account_ref = owner.account_ref
         WHERE owner.organization_id <> expected.organization_id
            OR owner.mailbox_id <> expected.mailbox_id
            OR owner.first_bound_at <> expected.first_bound_at
    ) THEN
        RAISE EXCEPTION
            'persisted mail-engine account ownership conflicts with trusted history'
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'mail_engine_account_owner_persisted_conflict';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM mail_engine_account_owners AS owner
          LEFT JOIN mail_engine_account_owner_expected AS expected
            ON expected.source = owner.source
           AND expected.account_ref = owner.account_ref
         WHERE expected.account_ref IS NULL
    ) THEN
        RAISE EXCEPTION
            'persisted mail-engine account ownership has no trusted binding evidence'
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'mail_engine_account_owner_unproven';
    END IF;
END;
$$;

-- This is append-only repair: missing proven owners are inserted, while the
-- checks above reject every conflicting or unexplained row.
INSERT INTO mail_engine_account_owners(
    source,
    account_ref,
    organization_id,
    mailbox_id,
    first_bound_at
)
SELECT expected.source,
       expected.account_ref,
       expected.organization_id,
       expected.mailbox_id,
       expected.first_bound_at
  FROM mail_engine_account_owner_expected AS expected
  LEFT JOIN mail_engine_account_owners AS owner
    ON owner.source = expected.source
   AND owner.account_ref = expected.account_ref
 WHERE owner.account_ref IS NULL;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
          LEFT JOIN mail_engine_account_owners AS owner
            ON owner.source = binding.source
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

CREATE OR REPLACE FUNCTION require_current_binding_for_account_owner()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
         WHERE binding.source = NEW.source
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

-- Deferral is required because migration 0051 claims a new owner from the
-- binding's BEFORE trigger. At commit, the outer INSERT/UPDATE must have
-- succeeded and the exact binding must be visible; a standalone owner INSERT
-- or a speculative owner left by a zero-row CAS therefore aborts atomically.
CREATE CONSTRAINT TRIGGER mail_engine_account_owners_require_current_binding
AFTER INSERT ON mail_engine_account_owners
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION require_current_binding_for_account_owner();
