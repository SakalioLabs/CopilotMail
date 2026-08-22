-- A provider account is permanently owned by the first tenant mailbox that
-- binds it. Current bindings may rotate to a new account, but an old account
-- can never be reassigned across a mailbox or tenant after a concurrent
-- content operation has resolved it.
CREATE TABLE mail_engine_account_owners (
    source text NOT NULL CHECK (source = 'stalwart'),
    account_ref text NOT NULL
        CHECK (
            char_length(account_ref) BETWEEN 1 AND 20
            AND account_ref ~ '^(0|[1-9][0-9]{0,19})$'
        ),
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    first_bound_at timestamptz NOT NULL,
    PRIMARY KEY(source, account_ref),
    CONSTRAINT mail_engine_account_owners_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id)
);

-- SQLx runs each migration in one transaction. Take the binding writer lock
-- before the historical scan and retain it until the ownership trigger becomes
-- visible at commit; otherwise a concurrent rotation could fall between the
-- backfill and trigger installation.
LOCK TABLE mail_engine_mailbox_bindings IN SHARE ROW EXCLUSIVE MODE;

CREATE TEMPORARY TABLE mail_engine_account_owner_backfill (
    source text NOT NULL,
    account_ref text NOT NULL,
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    first_bound_at timestamptz NOT NULL
) ON COMMIT DROP;

INSERT INTO mail_engine_account_owner_backfill(
    source,
    account_ref,
    organization_id,
    mailbox_id,
    first_bound_at
)
SELECT source, account_ref, organization_id, mailbox_id, updated_at
  FROM mail_engine_mailbox_bindings;

-- Binding rows retain only the current account. Successful append-only audit
-- events are the authoritative source for accounts retired before this
-- migration. A malformed historical event aborts the migration rather than
-- silently weakening the permanent ownership boundary.
INSERT INTO mail_engine_account_owner_backfill(
    source,
    account_ref,
    organization_id,
    mailbox_id,
    first_bound_at
)
SELECT details ->> 'source',
       details ->> 'account_ref',
       organization_id,
       CASE
           WHEN details ->> 'mailbox_id' = resource_id
           THEN resource_id::uuid
       END,
       occurred_at
  FROM audit_events
 WHERE action = 'mail_engine.mailbox_binding.updated'
   AND resource_type = 'mail_engine.mailbox_binding'
   AND outcome = 'success';

DO $$
BEGIN
    IF EXISTS (
        SELECT source, account_ref
          FROM mail_engine_account_owner_backfill
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

INSERT INTO mail_engine_account_owners(
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
  FROM mail_engine_account_owner_backfill
 GROUP BY source, account_ref, organization_id, mailbox_id;

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
        source,
        account_ref,
        organization_id,
        mailbox_id,
        first_bound_at
    )
    VALUES (
        NEW.source,
        NEW.account_ref,
        NEW.organization_id,
        NEW.mailbox_id,
        CASE WHEN TG_OP = 'INSERT' THEN NEW.created_at ELSE NEW.updated_at END
    )
    ON CONFLICT(source, account_ref) DO NOTHING;

    GET DIAGNOSTICS claimed_rows = ROW_COUNT;
    IF claimed_rows = 0 THEN
        SELECT organization_id, mailbox_id
          INTO owner_organization_id, owner_mailbox_id
          FROM mail_engine_account_owners
         WHERE source = NEW.source
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
BEFORE INSERT OR UPDATE OF source, account_ref, organization_id, mailbox_id
ON mail_engine_mailbox_bindings
FOR EACH ROW EXECUTE FUNCTION enforce_mail_engine_account_owner();

CREATE OR REPLACE FUNCTION reject_mail_engine_account_owner_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail_engine_account_owners are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_engine_account_owners_reject_update_delete
BEFORE UPDATE OR DELETE ON mail_engine_account_owners
FOR EACH ROW EXECUTE FUNCTION reject_mail_engine_account_owner_mutation();

CREATE TRIGGER mail_engine_account_owners_reject_truncate
BEFORE TRUNCATE ON mail_engine_account_owners
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_engine_account_owner_mutation();
