-- No released producer wrote this reserved V1 table. Refuse a silent upgrade
-- if an operator populated it manually; such rows need an explicit conversion.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM audit_checkpoints) THEN
        RAISE EXCEPTION
            'audit_checkpoints contains legacy rows; convert them before migration 0012'
            USING ERRCODE = '55000';
    END IF;
END;
$$;

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('audit.checkpoint.created', 'audit.checkpoint', 'critical', true);

ALTER TABLE audit_events
    ADD CONSTRAINT audit_events_checkpoint_first_target
        UNIQUE(id, organization_id, chain_sequence),
    ADD CONSTRAINT audit_events_checkpoint_last_target
        UNIQUE(id, organization_id, chain_sequence, event_hash, hash_version);

ALTER TABLE audit_checkpoints
    DROP CONSTRAINT audit_checkpoints_check,
    ADD COLUMN chain_sequence_start bigint NOT NULL,
    ADD COLUMN chain_sequence_end bigint NOT NULL,
    ADD COLUMN event_count bigint NOT NULL,
    ADD COLUMN first_event_id uuid NOT NULL,
    ADD COLUMN last_event_id uuid NOT NULL,
    ADD COLUMN hash_version smallint NOT NULL,
    ADD COLUMN signature_algorithm text NOT NULL,
    ADD COLUMN signed_payload text NOT NULL,
    ADD COLUMN public_key_fingerprint text NOT NULL,
    ADD CONSTRAINT audit_checkpoints_period_check
        CHECK (period_end >= period_start),
    ADD CONSTRAINT audit_checkpoints_sequence_check
        CHECK (
            chain_sequence_start = 1
            AND chain_sequence_end >= chain_sequence_start
            AND event_count = chain_sequence_end - chain_sequence_start + 1
        ),
    ADD CONSTRAINT audit_checkpoints_hash_check
        CHECK (octet_length(root_hash) = 32 AND hash_version = 1),
    ADD CONSTRAINT audit_checkpoints_signature_check
        CHECK (
            signature_algorithm = 'RSASSA-PKCS1-v1_5-SHA256'
            AND octet_length(signature) BETWEEN 256 AND 1024
            AND char_length(key_id) BETWEEN 1 AND 128
            AND key_id = btrim(key_id)
            AND key_id !~ '[[:cntrl:]]'
            AND public_key_fingerprint ~ '^[0-9a-f]{64}$'
            AND octet_length(signed_payload) BETWEEN 2 AND 16384
        ),
    ADD CONSTRAINT audit_checkpoints_object_key_check
        CHECK (
            char_length(object_key) BETWEEN 1 AND 1024
            AND object_key = btrim(object_key)
            AND object_key !~ '[[:cntrl:]]'
        ),
    ADD CONSTRAINT audit_checkpoints_first_event_fk
        FOREIGN KEY(first_event_id, organization_id, chain_sequence_start)
        REFERENCES audit_events(id, organization_id, chain_sequence),
    ADD CONSTRAINT audit_checkpoints_last_event_fk
        FOREIGN KEY(
            last_event_id, organization_id, chain_sequence_end, root_hash, hash_version
        )
        REFERENCES audit_events(
            id, organization_id, chain_sequence, event_hash, hash_version
        ),
    ADD CONSTRAINT audit_checkpoints_tenant_head_key_unique
        UNIQUE(organization_id, chain_sequence_end, key_id);

-- PostgreSQL independently verifies that security-critical structured columns
-- match the exact JSON bytes that were signed.
ALTER TABLE audit_checkpoints
    ADD CONSTRAINT audit_checkpoints_signed_payload_check CHECK (
        (signed_payload::jsonb ->> 'format_version')::smallint = 1
        AND (signed_payload::jsonb ->> 'checkpoint_id')::uuid = id
        AND (signed_payload::jsonb ->> 'organization_id')::uuid = organization_id
        AND (signed_payload::jsonb ->> 'chain_sequence_start')::bigint
            = chain_sequence_start
        AND (signed_payload::jsonb ->> 'chain_sequence_end')::bigint
            = chain_sequence_end
        AND (signed_payload::jsonb ->> 'event_count')::bigint = event_count
        AND (signed_payload::jsonb ->> 'first_event_id')::uuid = first_event_id
        AND (signed_payload::jsonb ->> 'last_event_id')::uuid = last_event_id
        AND signed_payload::jsonb ->> 'root_hash' = encode(root_hash, 'hex')
        AND (signed_payload::jsonb ->> 'hash_version')::smallint = hash_version
        AND signed_payload::jsonb ->> 'key_id' = key_id
        AND signed_payload::jsonb ->> 'public_key_sha256'
            = public_key_fingerprint
    );

CREATE OR REPLACE FUNCTION reject_audit_checkpoint_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit_checkpoints is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_checkpoints_append_only
BEFORE UPDATE OR DELETE ON audit_checkpoints
FOR EACH ROW EXECUTE FUNCTION reject_audit_checkpoint_mutation();

CREATE TRIGGER audit_checkpoints_reject_truncate
BEFORE TRUNCATE ON audit_checkpoints
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_truncate();

CREATE INDEX audit_checkpoints_chain_head_idx
    ON audit_checkpoints(organization_id, chain_sequence_end DESC, created_at DESC);
