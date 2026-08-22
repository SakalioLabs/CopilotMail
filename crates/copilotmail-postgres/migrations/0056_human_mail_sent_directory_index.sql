-- no-transaction
--
-- PostgreSQL forbids CREATE INDEX CONCURRENTLY inside SQLx's normal migration
-- transaction. The catalog action is committed in 0055 first; this index is
-- a separately recorded, online build. If a process dies after the index is
-- built but before SQLx records 0056, IF NOT EXISTS makes the retry safe and
-- schema_ready() still requires a valid index before this binary is ready.
--
-- Cursor pagination is by immutable acceptance order, not delivery or update
-- timestamps. The partial predicate also keeps rejected and pending work out
-- of the Human "Sent" directory by construction.
CREATE INDEX CONCURRENTLY IF NOT EXISTS mail_submission_intents_human_sent_directory_idx
    ON mail_submission_intents(
        organization_id,
        requested_by,
        mailbox_id,
        accepted_at DESC,
        id DESC
    )
    INCLUDE (draft_id, draft_version, mail_data_plane_id)
    WHERE status = 'accepted' AND accepted_at IS NOT NULL;
