-- Production Agent outbound admission. Recipient addresses remain in the mail
-- data plane; the control plane persists only canonical domains and
-- tenant-and-Agent-scoped SHA-256 references.
ALTER TABLE policy_versions
    DROP CONSTRAINT policy_versions_action_check;
ALTER TABLE policy_versions
    ADD CONSTRAINT policy_versions_action_check CHECK (action IN (
        'mail.send_as',
        'mail.send_on_behalf_of',
        'compliance.content_review',
        'audit.export',
        'compliance.retention_change',
        'authorization.privilege_elevation',
        'compliance.legal_hold',
        'security.break_glass',
        'mail.bulk_delete',
        'agent.production_mail',
        'agent.first_contact'
    ));

ALTER TABLE approval_requests
    DROP CONSTRAINT approval_requests_action_check;
ALTER TABLE approval_requests
    ADD CONSTRAINT approval_requests_action_check CHECK (action IN (
        'mail.send_as',
        'mail.send_on_behalf_of',
        'compliance.content_review',
        'audit.export',
        'compliance.retention_change',
        'authorization.privilege_elevation',
        'compliance.legal_hold',
        'security.break_glass',
        'mail.bulk_delete',
        'agent.production_mail',
        'agent.first_contact'
    ));

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('agent.mail.admitted', 'agent.mail_admission', 'critical', true);

ALTER TABLE mail_drafts
    ADD COLUMN recipient_refs bytea[],
    ADD COLUMN recipient_domains text[],
    ADD CONSTRAINT mail_drafts_recipient_evidence_check CHECK (
        (recipient_refs IS NULL AND recipient_domains IS NULL)
        OR (
            recipient_refs IS NOT NULL
            AND recipient_domains IS NOT NULL
            AND cardinality(recipient_refs) BETWEEN 1 AND 100
            AND cardinality(recipient_refs) = cardinality(recipient_domains)
        )
    );

CREATE OR REPLACE FUNCTION valid_mail_draft_recipient_evidence(
    refs bytea[],
    domains text[]
)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT refs IS NOT NULL
       AND domains IS NOT NULL
       AND cardinality(refs) BETWEEN 1 AND 100
       AND cardinality(refs) = cardinality(domains)
       AND NOT EXISTS (
           SELECT 1
             FROM unnest(refs) AS ref(value)
            WHERE octet_length(value) <> 32
       )
       AND cardinality(refs) = cardinality(ARRAY(
           SELECT DISTINCT value FROM unnest(refs) AS ref(value)
       ))
       AND NOT EXISTS (
           SELECT 1
             FROM unnest(domains) AS domain(value)
            WHERE value <> lower(value)
               OR value <> btrim(value)
               OR value !~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?([.][a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$'
               OR char_length(value) > 253
       );
$$;

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
           AND binding.source = NEW.engine_source
           AND binding.account_ref = NEW.engine_account_ref
    ) THEN
        RAISE EXCEPTION 'mail draft engine account must match the current mailbox binding'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_mail_draft_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
        OR NEW.organization_id <> OLD.organization_id
        OR NEW.mailbox_id <> OLD.mailbox_id
        OR NEW.created_by <> OLD.created_by
        OR NEW.engine_source <> OLD.engine_source
        OR NEW.engine_account_ref <> OLD.engine_account_ref
        OR NEW.created_at <> OLD.created_at
        OR NEW.version <> OLD.version + 1
        OR NEW.updated_at < OLD.updated_at
        OR NOT valid_mail_draft_recipient_evidence(
            NEW.recipient_refs,
            NEW.recipient_domains
        )
    THEN
        RAISE EXCEPTION 'mail draft identity, evidence, or version transition is invalid'
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

CREATE TABLE agent_mail_admissions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    agent_id uuid NOT NULL,
    submission_id uuid NOT NULL,
    policy_activation_id uuid NOT NULL,
    policy_version_id uuid NOT NULL,
    policy_version integer NOT NULL CHECK (policy_version > 0),
    draft_id uuid NOT NULL,
    draft_version bigint NOT NULL CHECK (draft_version > 1),
    thread_ref bytea NOT NULL CHECK (octet_length(thread_ref) = 32),
    recipient_count integer NOT NULL CHECK (recipient_count BETWEEN 1 AND 100),
    first_contact_count integer NOT NULL
        CHECK (first_contact_count BETWEEN 0 AND recipient_count),
    first_contact_approval_request_id uuid,
    admitted_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    UNIQUE(submission_id),
    FOREIGN KEY(agent_id, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(submission_id, organization_id)
        REFERENCES mail_submission_intents(id, organization_id),
    FOREIGN KEY(policy_activation_id, organization_id)
        REFERENCES agent_mail_policy_activations(id, organization_id),
    FOREIGN KEY(policy_version_id, organization_id)
        REFERENCES agent_mail_policy_versions(id, organization_id),
    FOREIGN KEY(draft_id, organization_id)
        REFERENCES mail_drafts(id, organization_id),
    FOREIGN KEY(first_contact_approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id)
);

CREATE INDEX agent_mail_admissions_quota_idx
    ON agent_mail_admissions(organization_id, agent_id, admitted_at DESC);

CREATE TABLE agent_mail_admission_recipients (
    admission_id uuid NOT NULL,
    organization_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    recipient_ref bytea NOT NULL CHECK (octet_length(recipient_ref) = 32),
    recipient_domain text NOT NULL,
    first_contact boolean NOT NULL,
    admitted_at timestamptz NOT NULL,
    PRIMARY KEY(admission_id, recipient_ref),
    FOREIGN KEY(admission_id, organization_id)
        REFERENCES agent_mail_admissions(id, organization_id),
    FOREIGN KEY(agent_id, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE INDEX agent_mail_admission_recipients_quota_idx
    ON agent_mail_admission_recipients(
        organization_id,
        agent_id,
        recipient_ref,
        admitted_at DESC
    );

CREATE OR REPLACE FUNCTION validate_agent_mail_admission_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    current_activation agent_mail_policy_activations%ROWTYPE;
    current_policy agent_mail_policy_versions%ROWTYPE;
    current_draft mail_drafts%ROWTYPE;
    expected_first_contacts integer;
    current_thread_ref bytea;
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended(NEW.organization_id::text || ':' || NEW.agent_id::text, 0)
    );
    -- Quota windows use the database clock so API-host skew cannot expand or
    -- shrink capacity across replicas.
    NEW.admitted_at := clock_timestamp();

    SELECT activation.*
      INTO current_activation
      FROM agent_mail_policy_activations AS activation
     WHERE activation.organization_id = NEW.organization_id
       AND activation.agent_id = NEW.agent_id
     ORDER BY activation.policy_version DESC
     LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'production Agent has no active mail policy'
            USING ERRCODE = '23514';
    END IF;

    SELECT policy.*
      INTO current_policy
      FROM agent_mail_policy_versions AS policy
     WHERE policy.id = current_activation.policy_version_id
       AND policy.organization_id = NEW.organization_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'active production Agent mail policy is unavailable'
            USING ERRCODE = '23514';
    END IF;

    SELECT draft.*
      INTO current_draft
      FROM mail_submission_intents AS intent
      JOIN principals AS principal
        ON principal.id = intent.requested_by
       AND principal.organization_id = intent.organization_id
      JOIN agent_profiles AS profile
        ON profile.principal_id = principal.id
      JOIN mail_drafts AS draft
        ON draft.id = intent.draft_id
       AND draft.organization_id = intent.organization_id
     WHERE intent.id = NEW.submission_id
       AND intent.organization_id = NEW.organization_id
       AND intent.requested_by = NEW.agent_id
       AND intent.status = 'pending'
       AND draft.id = NEW.draft_id
       AND draft.version = NEW.draft_version
       AND draft.status = 'frozen'
       AND principal.status = 'active'
       AND principal.kind = 'agent'
       AND profile.environment = 'production';
    IF NOT FOUND OR NOT valid_mail_draft_recipient_evidence(
        current_draft.recipient_refs,
        current_draft.recipient_domains
    ) THEN
        RAISE EXCEPTION 'admission must bind one exact production Agent draft'
            USING ERRCODE = '23514';
    END IF;

    IF cardinality(current_draft.recipient_refs)
        > current_policy.max_recipients_per_message
       OR EXISTS (
           SELECT 1
             FROM unnest(current_draft.recipient_domains) AS domain(value)
            WHERE NOT value = ANY(current_policy.allowed_recipient_domains)
       )
    THEN
        RAISE EXCEPTION 'recipient policy rejected production Agent mail'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_recipient_policy';
    END IF;

    IF (
        SELECT count(*)
          FROM agent_mail_admissions AS admission
         WHERE admission.organization_id = NEW.organization_id
           AND admission.agent_id = NEW.agent_id
           AND admission.admitted_at > NEW.admitted_at - interval '1 hour'
           AND admission.admitted_at <= NEW.admitted_at
    ) >= current_policy.max_messages_per_hour
    THEN
        RAISE EXCEPTION 'production Agent hourly quota exceeded'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_hourly_quota';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM unnest(current_draft.recipient_refs) AS draft_recipient(value)
         WHERE (
             SELECT count(*)
               FROM agent_mail_admission_recipients AS prior
              WHERE prior.organization_id = NEW.organization_id
                AND prior.agent_id = NEW.agent_id
                AND prior.recipient_ref = draft_recipient.value
                AND prior.admitted_at >= date_trunc(
                    'day',
                    NEW.admitted_at,
                    'UTC'
                )
                AND prior.admitted_at <= NEW.admitted_at
         ) >= current_policy.max_messages_per_recipient_per_day
    ) THEN
        RAISE EXCEPTION 'production Agent recipient daily quota exceeded'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_recipient_daily_quota';
    END IF;

    current_thread_ref := digest(
        convert_to(
            NEW.organization_id::text || ':' || NEW.agent_id::text || ':'
                || current_draft.engine_thread_ref,
            'UTF8'
        ),
        'sha256'
    );
    IF EXISTS (
        SELECT 1
          FROM agent_mail_admissions AS prior
         WHERE prior.organization_id = NEW.organization_id
           AND prior.agent_id = NEW.agent_id
           AND prior.thread_ref = current_thread_ref
    ) AND (
        SELECT count(*)
          FROM agent_mail_admissions AS admission
         WHERE admission.organization_id = NEW.organization_id
           AND admission.agent_id = NEW.agent_id
           AND admission.thread_ref = current_thread_ref
           AND admission.admitted_at >= date_trunc(
               'day',
               NEW.admitted_at,
               'UTC'
           )
           AND admission.admitted_at <= NEW.admitted_at
    ) >= current_policy.max_auto_replies_per_thread_per_day
    THEN
        RAISE EXCEPTION 'production Agent thread circuit breaker opened'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_thread_circuit_breaker';
    END IF;

    SELECT count(*)
      INTO expected_first_contacts
      FROM unnest(current_draft.recipient_refs) AS draft_recipient(value)
     WHERE NOT EXISTS (
         SELECT 1
           FROM agent_mail_admission_recipients AS prior
          WHERE prior.organization_id = NEW.organization_id
            AND prior.agent_id = NEW.agent_id
            AND prior.recipient_ref = draft_recipient.value
     );

    IF current_policy.require_first_contact_approval
       AND expected_first_contacts > 0
       AND NOT EXISTS (
           SELECT 1
             FROM approval_requests AS request
            WHERE request.id = NEW.first_contact_approval_request_id
              AND request.organization_id = NEW.organization_id
              AND request.requested_by = NEW.agent_id
              AND request.action = 'agent.first_contact'
              AND request.resource_kind = 'agent.mail_draft'
              AND request.resource_id = (
                  NEW.draft_id::text || ':' || (NEW.draft_version - 1)::text
              )
              AND request.status = 'approved'
              AND request.required_approvals >= 1
              AND request.created_at <= NEW.admitted_at
              AND request.expires_at > NEW.admitted_at
              AND (
                  SELECT count(*)
                    FROM approval_steps AS step
                   WHERE step.organization_id = NEW.organization_id
                     AND step.approval_request_id = request.id
                     AND step.decision = 'approved'
              ) >= request.required_approvals
              AND NOT EXISTS (
                  SELECT 1
                    FROM approval_steps AS step
                   WHERE step.organization_id = NEW.organization_id
                     AND step.approval_request_id = request.id
                     AND step.decision = 'rejected'
              )
       )
    THEN
        RAISE EXCEPTION 'first-contact production mail requires exact live approval'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_first_contact_approval';
    END IF;

    NEW.policy_activation_id := current_activation.id;
    NEW.policy_version_id := current_activation.policy_version_id;
    NEW.policy_version := current_activation.policy_version;
    NEW.thread_ref := current_thread_ref;
    NEW.recipient_count := cardinality(current_draft.recipient_refs);
    NEW.first_contact_count := expected_first_contacts;
    IF expected_first_contacts = 0 THEN
        NEW.first_contact_approval_request_id := NULL;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_mail_admissions_validate_insert
BEFORE INSERT ON agent_mail_admissions
FOR EACH ROW EXECUTE FUNCTION validate_agent_mail_admission_insert();

CREATE OR REPLACE FUNCTION materialize_agent_mail_admission_recipients()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO agent_mail_admission_recipients(
        admission_id,
        organization_id,
        agent_id,
        recipient_ref,
        recipient_domain,
        first_contact,
        admitted_at
    )
    SELECT NEW.id,
           NEW.organization_id,
           NEW.agent_id,
           recipient.ref,
           recipient.domain,
           NOT EXISTS (
               SELECT 1
                 FROM agent_mail_admission_recipients AS prior
                WHERE prior.organization_id = NEW.organization_id
                  AND prior.agent_id = NEW.agent_id
                  AND prior.recipient_ref = recipient.ref
           ),
           NEW.admitted_at
      FROM mail_drafts AS draft
      CROSS JOIN LATERAL unnest(
          draft.recipient_refs,
          draft.recipient_domains
      ) AS recipient(ref, domain)
     WHERE draft.id = NEW.draft_id
       AND draft.organization_id = NEW.organization_id;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_mail_admissions_materialize_recipients
AFTER INSERT ON agent_mail_admissions
FOR EACH ROW EXECUTE FUNCTION materialize_agent_mail_admission_recipients();

CREATE OR REPLACE FUNCTION reject_agent_mail_admission_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent mail admission evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_mail_admissions_reject_update
BEFORE UPDATE ON agent_mail_admissions
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_admission_mutation();
CREATE TRIGGER agent_mail_admissions_reject_delete
BEFORE DELETE ON agent_mail_admissions
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_admission_mutation();
CREATE TRIGGER agent_mail_admissions_reject_truncate
BEFORE TRUNCATE ON agent_mail_admissions
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_mail_admission_mutation();
CREATE TRIGGER agent_mail_admission_recipients_reject_update
BEFORE UPDATE ON agent_mail_admission_recipients
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_admission_mutation();
CREATE TRIGGER agent_mail_admission_recipients_reject_delete
BEFORE DELETE ON agent_mail_admission_recipients
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_admission_mutation();
CREATE TRIGGER agent_mail_admission_recipients_reject_truncate
BEFORE TRUNCATE ON agent_mail_admission_recipients
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_mail_admission_mutation();

CREATE OR REPLACE FUNCTION validate_mail_submission_intent_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
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
    IF NOT EXISTS (
        SELECT 1
          FROM mail_drafts AS draft
         WHERE draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id
           AND draft.mailbox_id = NEW.mailbox_id
           AND draft.created_by = NEW.requested_by
           AND draft.version = NEW.draft_version
           AND draft.status = 'frozen'
           AND draft.engine_source = NEW.engine_source
           AND draft.engine_account_ref = NEW.engine_account_ref
           AND draft.engine_email_ref = NEW.engine_email_ref
    ) THEN
        RAISE EXCEPTION 'mail submission intent must bind one exact frozen owned draft'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
