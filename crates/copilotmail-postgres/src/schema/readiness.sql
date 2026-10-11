SELECT to_regclass('public.organizations') IS NOT NULL
   AND to_regclass('public.organization_bootstraps') IS NOT NULL
   AND to_regclass('public.oidc_owner_bindings') IS NOT NULL
   AND to_regclass('public.scim_tokens') IS NOT NULL
   AND to_regclass('public.scim_users') IS NOT NULL
   AND to_regclass('public.scim_groups') IS NOT NULL
   AND to_regclass('public.scim_group_members') IS NOT NULL
   AND to_regclass('public.agent_private_key_credentials') IS NOT NULL
   AND to_regclass('public.agent_mtls_credentials') IS NOT NULL
   AND to_regclass('public.oauth_client_assertion_jtis') IS NOT NULL
   AND to_regclass('public.roles') IS NOT NULL
   AND EXISTS (
       SELECT 1
         FROM pg_constraint AS permission_check
        WHERE permission_check.conrelid = 'public.role_permissions'::regclass
          AND permission_check.conname = 'role_permissions_permission_check'
          AND permission_check.contype = 'c'
          AND permission_check.convalidated
          AND pg_get_constraintdef(permission_check.oid)
                  LIKE '%agent.mail.read%'
          AND pg_get_constraintdef(permission_check.oid)
                  LIKE '%session.revoke_others%'
   )
   AND to_regclass('public.human_session_governance_executions') IS NOT NULL
   AND to_regclass('public.mailboxes') IS NOT NULL
   AND to_regclass('public.mail_domains') IS NOT NULL
   AND to_regclass('public.mail_domain_dns_observations') IS NOT NULL
   AND to_regclass('public.mail_domain_routes') IS NOT NULL
   AND to_regclass('public.mail_data_planes') IS NOT NULL
   AND EXISTS (
       SELECT 1
         FROM mail_data_planes
        WHERE id = '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101'::uuid
          AND adapter_kind = 'stalwart_jmap'
   )
   AND to_regclass('public.mail_data_plane_origin_bindings') IS NOT NULL
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_data_planes', 'mail_data_planes_pkey', 'p', ARRAY['id']::text[]),
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_pkey', 'p', ARRAY['data_plane_id']::text[]),
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_canonical_origin_key', 'u', ARRAY['canonical_origin']::text[]),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_account_unique', 'u', ARRAY['mail_data_plane_id', 'account_ref']::text[]),
             ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_account_unique', 'u', ARRAY['mail_data_plane_id', 'account_ref']::text[]),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_event_unique', 'u', ARRAY['mail_data_plane_id', 'source_event_id']::text[]),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_message_unique', 'u', ARRAY['mail_data_plane_id', 'account_ref', 'document_ref']::text[]),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_event_unique', 'u', ARRAY['mail_data_plane_id', 'source_event_id']::text[]),
             ('mail_drafts', 'mail_drafts_data_plane_email_unique', 'u', ARRAY['mail_data_plane_id', 'engine_account_ref', 'engine_email_ref']::text[]),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_source_unique', 'u', ARRAY['organization_id', 'submission_id', 'mail_data_plane_id', 'source_sha256']::text[]),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_account_unique', 'u', ARRAY['source', 'account_ref']::text[]),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_event_unique', 'u', ARRAY['source', 'source_event_id']::text[]),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_message_unique', 'u', ARRAY['source', 'account_ref', 'document_ref']::text[]),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_event_unique', 'u', ARRAY['source', 'source_event_id']::text[]),
             ('mail_drafts', 'mail_drafts_engine_email_unique', 'u', ARRAY['engine_source', 'engine_account_ref', 'engine_email_ref']::text[])
         ) AS required_key(table_name, constraint_name, constraint_type, column_names)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_constraint AS constraint_row
             WHERE constraint_row.conrelid =
                       to_regclass('public.' || required_key.table_name)
               AND constraint_row.conname = required_key.constraint_name
               AND constraint_row.contype::text = required_key.constraint_type
               AND constraint_row.convalidated
               AND ARRAY(
                   SELECT attribute.attname::text
                     FROM unnest(constraint_row.conkey)
                          WITH ORDINALITY AS key_column(attnum, position)
                     JOIN pg_attribute AS attribute
                       ON attribute.attrelid = constraint_row.conrelid
                      AND attribute.attnum = key_column.attnum
                    WHERE NOT attribute.attisdropped
                    ORDER BY key_column.position
               ) = required_key.column_names
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_data_plane_id_fkey', 'data_plane_id'),
             ('mail_domains', 'mail_domains_data_plane_fkey', 'mail_data_plane_id'),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_fkey', 'mail_data_plane_id'),
             ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_fkey', 'mail_data_plane_id'),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_fkey', 'mail_data_plane_id'),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_fkey', 'mail_data_plane_id'),
             ('mail_drafts', 'mail_drafts_data_plane_fkey', 'mail_data_plane_id'),
             ('mail_submission_intents', 'mail_submission_intents_data_plane_fkey', 'mail_data_plane_id'),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_fkey', 'mail_data_plane_id'),
             ('agent_mail_admissions', 'agent_mail_admissions_data_plane_fkey', 'mail_data_plane_id'),
             ('outbox_events', 'outbox_events_data_plane_fkey', 'mail_data_plane_id')
         ) AS required_fkey(table_name, constraint_name, column_name)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_constraint AS constraint_row
             WHERE constraint_row.conrelid =
                       to_regclass('public.' || required_fkey.table_name)
               AND constraint_row.conname = required_fkey.constraint_name
               AND constraint_row.contype = 'f'
               AND constraint_row.convalidated
               AND constraint_row.confrelid = 'public.mail_data_planes'::regclass
               AND ARRAY(
                   SELECT attribute.attname::text
                     FROM unnest(constraint_row.conkey)
                          WITH ORDINALITY AS key_column(attnum, position)
                     JOIN pg_attribute AS attribute
                       ON attribute.attrelid = constraint_row.conrelid
                      AND attribute.attnum = key_column.attnum
                    ORDER BY key_column.position
               ) = ARRAY[required_fkey.column_name]::text[]
               AND ARRAY(
                   SELECT attribute.attname::text
                     FROM unnest(constraint_row.confkey)
                          WITH ORDINALITY AS key_column(attnum, position)
                     JOIN pg_attribute AS attribute
                       ON attribute.attrelid = constraint_row.confrelid
                      AND attribute.attnum = key_column.attnum
                    ORDER BY key_column.position
               ) = ARRAY['id']::text[]
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_data_planes', 'mail_data_planes_non_nil_id_check', ARRAY['%00000000-0000-0000-0000-000000000000%']::text[]),
             ('agent_private_key_credentials', 'agent_private_key_credentials_allowed_scopes_check', ARRAY['%cardinality(allowed_scopes) >= 1%', '%cardinality(allowed_scopes) <= 4%', '%agent.mail.read%']::text[]),
             ('agent_mtls_credentials', 'agent_mtls_credentials_allowed_scopes_check', ARRAY['%cardinality(allowed_scopes) >= 1%', '%cardinality(allowed_scopes) <= 4%', '%agent.mail.read%']::text[]),
             ('mail_data_planes', 'mail_data_planes_adapter_kind_check', ARRAY['%lower(adapter_kind)%', '%^[a-z][a-z0-9._-]{0,63}$%']::text[]),
             ('mail_data_planes', 'mail_data_planes_label_check', ARRAY['%btrim(label)%', '%[[:cntrl:]]%']::text[]),
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_origin_shape_check', ARRAY['%lower(canonical_origin)%', '%https?://%']::text[]),
             ('mail_domains', 'mail_domains_data_plane_assignment_check', ARRAY['%mail_data_plane_assigned_at >= created_at%']::text[]),
             ('mail_domains', 'mail_domains_engine_ref_data_plane_check', ARRAY['%mail_engine_domain_ref IS NULL%', '%mail_data_plane_id IS NOT NULL%']::text[]),
             ('mail_domains', 'mail_domains_projection_data_plane_check', ARRAY['%mail_engine_projection_status%', '%mail_engine_domain_ref IS NOT NULL%']::text[]),
             ('outbox_events', 'outbox_events_required_data_plane_route_check', ARRAY['%mail_engine.mailbox_binding.updated.v2%', '%mail_domain.suspended.v1%', '%mail_data_plane_id IS NOT NULL%']::text[]),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_check', ARRAY['%stalwart%']::text[]),
             ('mail_engine_account_owners', 'mail_engine_account_owners_source_check', ARRAY['%stalwart%']::text[]),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_check', ARRAY['%stalwart%']::text[]),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_check', ARRAY['%stalwart%']::text[]),
             ('mail_drafts', 'mail_drafts_engine_source_check', ARRAY['%stalwart%']::text[]),
             ('mail_submission_intents', 'mail_submission_intents_engine_source_check', ARRAY['%stalwart%']::text[]),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_source_check', ARRAY['%stalwart_jmap%']::text[])
         ) AS required_check(table_name, constraint_name, definition_patterns)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_constraint AS constraint_row
             WHERE constraint_row.conrelid =
                       to_regclass('public.' || required_check.table_name)
               AND constraint_row.conname = required_check.constraint_name
               AND constraint_row.contype = 'c'
               AND constraint_row.convalidated
               AND pg_get_constraintdef(constraint_row.oid)
                       LIKE ALL(required_check.definition_patterns)
        )
   )               AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_data_planes', 'mail_data_planes_reject_update_delete', 'reject_mail_data_plane_mutation', 27),
             ('mail_data_planes', 'mail_data_planes_reject_truncate', 'reject_mail_data_plane_mutation', 34),
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_validate_insert', 'validate_mail_data_plane_origin_binding_insert', 7),
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_reject_update_delete', 'reject_mail_data_plane_origin_binding_mutation', 27),
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_reject_truncate', 'reject_mail_data_plane_origin_binding_mutation', 34),
             ('mail_domains', 'mail_domains_guard_data_plane_identity', 'guard_mail_domain_data_plane_identity', 19),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_guard_data_plane', 'guard_mail_engine_binding_data_plane', 19),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_validate_data_plane', 'validate_inbound_mail_engine_event_data_plane', 7),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_validate_data_plane', 'validate_outbound_mail_engine_event_data_plane', 7),
             ('agent_mail_admissions', 'agent_mail_admissions_validate_data_plane', 'validate_agent_mail_admission_data_plane', 7),
             ('agent_mail_admissions', 'agent_mail_admissions_validate_insert', 'validate_agent_mail_admission_insert', 7),
             ('agent_mail_admissions', 'agent_mail_admissions_materialize_recipients', 'materialize_agent_mail_admission_recipients', 5),
             ('agent_mail_admissions', 'agent_mail_admissions_reject_update', 'reject_agent_mail_admission_mutation', 19),
             ('agent_mail_admissions', 'agent_mail_admissions_reject_delete', 'reject_agent_mail_admission_mutation', 11),
             ('agent_mail_admissions', 'agent_mail_admissions_reject_truncate', 'reject_agent_mail_admission_mutation', 34),
             ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_update', 'reject_agent_mail_admission_mutation', 19),
             ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_delete', 'reject_agent_mail_admission_mutation', 11),
             ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_truncate', 'reject_agent_mail_admission_mutation', 34),
             ('outbox_events', 'outbox_events_validate_data_plane_route', 'validate_outbox_event_data_plane_route', 7),
             ('outbox_events', 'outbox_events_guard_identity', 'guard_outbox_event_identity', 19),
             ('mail_drafts', 'mail_drafts_validate_insert', 'validate_mail_draft_insert', 7),
             ('mail_drafts', 'mail_drafts_validate_update', 'validate_mail_draft_update', 19),
             ('mail_submission_intents', 'mail_submission_intents_validate_insert', 'validate_mail_submission_intent_insert', 7),
             ('mail_submission_intents', 'mail_submission_intents_validate_update', 'validate_mail_submission_intent_update', 19),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_validate_insert', 'validate_mail_delivery_snapshot_insert', 7)
         ) AS required_trigger(table_name, trigger_name, function_name, trigger_type)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_trigger AS trigger
             WHERE trigger.tgrelid =
                       to_regclass('public.' || required_trigger.table_name)
               AND trigger.tgname = required_trigger.trigger_name
               AND NOT trigger.tgisinternal
               AND trigger.tgenabled = 'O'
               AND trigger.tgtype = required_trigger.trigger_type
               AND trigger.tgnargs = 0
               AND trigger.tgqual IS NULL
               AND trigger.tgfoid = to_regprocedure(
                   'public.' || required_trigger.function_name || '()'
               )
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_domains', 'mail_domains_guard_data_plane_identity', ARRAY['mail_data_plane_assigned_at', 'mail_data_plane_id', 'mail_engine_domain_ref']::text[]),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_guard_data_plane', ARRAY['mail_data_plane_id']::text[]),
             ('mail_drafts', 'mail_drafts_validate_insert', ARRAY[]::text[]),
             ('mail_drafts', 'mail_drafts_validate_update', ARRAY[]::text[]),
             ('mail_submission_intents', 'mail_submission_intents_validate_insert', ARRAY[]::text[]),
             ('mail_submission_intents', 'mail_submission_intents_validate_update', ARRAY[]::text[]),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_validate_insert', ARRAY[]::text[]),
             ('agent_mail_admissions', 'agent_mail_admissions_validate_data_plane', ARRAY[]::text[]),
             ('agent_mail_admissions', 'agent_mail_admissions_validate_insert', ARRAY[]::text[]),
             ('agent_mail_admissions', 'agent_mail_admissions_materialize_recipients', ARRAY[]::text[]),
             ('agent_mail_admissions', 'agent_mail_admissions_reject_update', ARRAY[]::text[]),
             ('agent_mail_admissions', 'agent_mail_admissions_reject_delete', ARRAY[]::text[]),
             ('agent_mail_admissions', 'agent_mail_admissions_reject_truncate', ARRAY[]::text[]),
             ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_update', ARRAY[]::text[]),
             ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_delete', ARRAY[]::text[]),
             ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_truncate', ARRAY[]::text[]),
             ('outbox_events', 'outbox_events_validate_data_plane_route', ARRAY[]::text[]),
             ('outbox_events', 'outbox_events_guard_identity', ARRAY[]::text[])
         ) AS required_trigger_columns(table_name, trigger_name, column_names)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_trigger AS trigger
             WHERE trigger.tgrelid =
                       to_regclass('public.' || required_trigger_columns.table_name)
               AND trigger.tgname = required_trigger_columns.trigger_name
               AND ARRAY(
                   SELECT attribute.attname::text
                     FROM unnest(trigger.tgattr::smallint[]) AS trigger_column(attnum)
                     JOIN pg_attribute AS attribute
                       ON attribute.attrelid = trigger.tgrelid
                      AND attribute.attnum = trigger_column.attnum
                    WHERE NOT attribute.attisdropped
                    ORDER BY attribute.attname
               ) = required_trigger_columns.column_names
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_origin_shape_check'),
             ('mail_domains', 'mail_domains_data_plane_assignment_check'),
             ('mail_domains', 'mail_domains_engine_ref_data_plane_check'),
             ('mail_domains', 'mail_domains_projection_data_plane_check'),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_fkey'),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_account_unique'),
             ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_fkey'),
             ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_account_unique'),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_fkey'),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_event_unique'),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_message_unique'),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_fkey'),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_event_unique'),
             ('mail_drafts', 'mail_drafts_data_plane_fkey'),
             ('mail_drafts', 'mail_drafts_data_plane_email_unique'),
             ('mail_submission_intents', 'mail_submission_intents_data_plane_fkey'),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_fkey'),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_source_unique'),
             ('agent_mail_admissions', 'agent_mail_admissions_data_plane_fkey'),
             ('outbox_events', 'outbox_events_data_plane_fkey'),
             ('outbox_events', 'outbox_events_required_data_plane_route_check'),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_check'),
             ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_account_unique'),
             ('mail_engine_account_owners', 'mail_engine_account_owners_source_check'),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_check'),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_event_unique'),
             ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_message_unique'),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_check'),
             ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_event_unique'),
             ('mail_drafts', 'mail_drafts_engine_source_check'),
             ('mail_drafts', 'mail_drafts_engine_email_unique'),
             ('mail_submission_intents', 'mail_submission_intents_engine_source_check'),
             ('mail_delivery_snapshots', 'mail_delivery_snapshots_source_check')
         ) AS required_constraint(table_name, constraint_name)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_constraint AS constraint_row
             WHERE constraint_row.conrelid =
                       to_regclass('public.' || required_constraint.table_name)
               AND constraint_row.conname = required_constraint.constraint_name
               AND constraint_row.convalidated
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_domains_data_plane_engine_ref_unique', 'mail_domains', true, ARRAY['mail_data_plane_id', 'mail_engine_domain_ref']::text[], 'mail_engine_domain_refISNOTNULL'),
             ('outbox_unpublished_data_plane_idx', 'outbox_events', false, ARRAY['mail_data_plane_id', 'next_attempt_at', 'occurred_at', 'id']::text[], 'published_atISNULL')
         ) AS required_index(index_name, table_name, is_unique, column_names, predicate)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_index AS index_row
             WHERE index_row.indexrelid =
                       to_regclass('public.' || required_index.index_name)
               AND index_row.indrelid =
                       to_regclass('public.' || required_index.table_name)
               AND index_row.indisunique = required_index.is_unique
               AND index_row.indisvalid
               AND index_row.indisready
               AND ARRAY(
                   SELECT attribute.attname::text
                     FROM unnest(index_row.indkey::smallint[])
                          WITH ORDINALITY AS key_column(attnum, position)
                     JOIN pg_attribute AS attribute
                       ON attribute.attrelid = index_row.indrelid
                      AND attribute.attnum = key_column.attnum
                    WHERE key_column.position <= index_row.indnkeyatts
                    ORDER BY key_column.position
               ) = required_index.column_names
               AND trim(both '()' from regexp_replace(
                       pg_get_expr(index_row.indpred, index_row.indrelid),
                       '\s+',
                       '',
                       'g'
                   )) = required_index.predicate
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_domains', 'mail_data_plane_id'),
             ('mail_domains', 'mail_data_plane_assigned_at'),
             ('outbox_events', 'mail_data_plane_id')
         ) AS nullable_column(table_name, column_name)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_attribute AS attribute
             WHERE attribute.attrelid =
                       to_regclass('public.' || nullable_column.table_name)
               AND attribute.attname = nullable_column.column_name
               AND NOT attribute.attnotnull
               AND NOT attribute.atthasdef
               AND NOT attribute.attisdropped
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('mail_engine_mailbox_bindings'),
             ('mail_engine_account_owners'),
             ('inbound_mail_engine_events'),
             ('outbound_mail_engine_events'),
             ('mail_drafts'),
             ('mail_submission_intents'),
             ('mail_delivery_snapshots'),
             ('agent_mail_admissions')
         ) AS legacy_default(table_name)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_attribute AS attribute
              JOIN pg_attrdef AS default_row
                ON default_row.adrelid = attribute.attrelid
               AND default_row.adnum = attribute.attnum
             WHERE attribute.attrelid =
                       to_regclass('public.' || legacy_default.table_name)
               AND attribute.attname = 'mail_data_plane_id'
               AND attribute.atttypid = 'uuid'::regtype
               AND attribute.attnotnull
               AND NOT attribute.attisdropped
               AND pg_get_expr(default_row.adbin, default_row.adrelid) =
                       '''018f305d-3d0f-7b2a-9d68-7f4a3f6e2101''::uuid'
        )
   )
   AND NOT EXISTS (
       SELECT 1
         FROM (VALUES
             ('validate_mail_draft_insert', ARRAY['%binding.mail_data_plane_id = NEW.mail_data_plane_id%', '%inbound.mail_data_plane_id = NEW.mail_data_plane_id%']::text[]),
             ('validate_mail_draft_update', ARRAY['%NEW.mail_data_plane_id <> OLD.mail_data_plane_id%']::text[]),
             ('validate_mail_submission_intent_insert', ARRAY['%draft.mail_data_plane_id = NEW.mail_data_plane_id%']::text[]),
             ('validate_mail_submission_intent_update', ARRAY['%NEW.mail_data_plane_id <> OLD.mail_data_plane_id%']::text[]),
             ('validate_mail_delivery_snapshot_insert', ARRAY['%submission.mail_data_plane_id = NEW.mail_data_plane_id%']::text[]),
             ('validate_agent_mail_admission_data_plane', ARRAY['%intent.mail_data_plane_id = NEW.mail_data_plane_id%', '%draft.mail_data_plane_id = NEW.mail_data_plane_id%']::text[])
         ) AS required_function(function_name, definition_patterns)
        WHERE NOT EXISTS (
            SELECT 1
              FROM pg_proc AS function_row
             WHERE function_row.oid = to_regprocedure(
                       'public.' || required_function.function_name || '()'
                   )
               AND pg_get_functiondef(function_row.oid)
                       LIKE ALL(required_function.definition_patterns)
        )
   )
   AND EXISTS (
       SELECT 1
         FROM pg_proc AS function_row
        WHERE function_row.oid =
                  to_regprocedure('public.validate_agent_mail_admission_insert()')
          AND regexp_replace(pg_get_functiondef(function_row.oid), '\s+', '', 'g')
                  LIKE '%ANDprior.mail_data_plane_id=NEW.mail_data_plane_id%'
          AND regexp_replace(pg_get_functiondef(function_row.oid), '\s+', '', 'g')
                  LIKE '%ANDadmission.mail_data_plane_id=NEW.mail_data_plane_id%'
          AND regexp_replace(pg_get_functiondef(function_row.oid), '\s+', '', 'g')
                  LIKE $legacy_thread_digest$%current_thread_ref:=digest(convert_to(NEW.organization_id::text||':'||NEW.agent_id::text||':'||current_draft.engine_thread_ref,'UTF8'),'sha256');%$legacy_thread_digest$
   )
   AND NOT EXISTS (
       SELECT 1
         FROM mail_domains
        WHERE (mail_data_plane_id IS NULL) <>
                  (mail_data_plane_assigned_at IS NULL)
           OR (mail_engine_domain_ref IS NOT NULL AND mail_data_plane_id IS NULL)
           OR (
               mail_engine_projection_status = 'ready'
               AND (
                   mail_data_plane_id IS NULL
                   OR mail_data_plane_assigned_at IS NULL
                   OR mail_engine_domain_ref IS NULL
               )
           )
   )
   AND NOT EXISTS (
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
   )               AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.mail_domains'::regclass
          AND attname = 'mail_engine_projection_status'
          AND attnotnull
          AND NOT attisdropped
   )
   AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.mail_domains'::regclass
          AND attname = 'ownership_challenge_version'
          AND attnotnull
          AND NOT attisdropped
   )
   AND to_regclass('public.delegations') IS NOT NULL
   AND to_regclass('public.policies') IS NOT NULL
   AND to_regclass('public.policy_versions') IS NOT NULL
   AND to_regclass('public.approval_requests') IS NOT NULL
   AND to_regclass('public.approval_steps') IS NOT NULL
   AND to_regclass('public.audit_event_catalog') IS NOT NULL
   AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.mailbox_memberships'::regclass
          AND attname = 'organization_id'
          AND NOT attisdropped
   )
   AND to_regclass('public.audit_events') IS NOT NULL
   AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.audit_events'::regclass
          AND attname = 'chain_sequence'
          AND attnotnull
          AND NOT attisdropped
   )
   AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.audit_checkpoints'::regclass
          AND attname = 'signed_payload'
          AND attnotnull
          AND NOT attisdropped
   )
   AND to_regclass('public.audit_archives') IS NOT NULL
   AND to_regclass('public.compliance_cases') IS NOT NULL
   AND to_regclass('public.legal_holds') IS NOT NULL
   AND to_regclass('public.outbox_events') IS NOT NULL
   AND to_regclass('public.mail_engine_mailbox_bindings') IS NOT NULL
   AND to_regclass('public.mail_engine_account_owners') IS NOT NULL
   AND EXISTS (
       SELECT 1
         FROM pg_class AS owner_table
         JOIN pg_namespace AS owner_namespace
           ON owner_namespace.oid = owner_table.relnamespace
        WHERE owner_namespace.nspname = 'public'
          AND owner_table.relname = 'mail_engine_account_owners'
          AND owner_table.relkind = 'r'
          AND EXISTS (
              SELECT 1
                FROM pg_constraint AS owner_primary_key
               WHERE owner_primary_key.conrelid = owner_table.oid
                 AND owner_primary_key.contype = 'p'
                 AND ARRAY(
                     SELECT attribute.attname::text
                       FROM unnest(owner_primary_key.conkey)
                            WITH ORDINALITY
                            AS key_column(attnum, position)
                       JOIN pg_attribute AS attribute
                         ON attribute.attrelid = owner_table.oid
                        AND attribute.attnum = key_column.attnum
                      WHERE NOT attribute.attisdropped
                      ORDER BY key_column.position
                 ) = ARRAY['source', 'account_ref']::text[]
          )
          AND EXISTS (
              SELECT 1
                FROM pg_constraint AS owner_data_plane_unique
               WHERE owner_data_plane_unique.conrelid = owner_table.oid
                 AND owner_data_plane_unique.contype = 'u'
                 AND owner_data_plane_unique.conname =
                     'mail_engine_account_owners_data_plane_account_unique'
                 AND ARRAY(
                     SELECT attribute.attname::text
                       FROM unnest(owner_data_plane_unique.conkey)
                            WITH ORDINALITY
                            AS key_column(attnum, position)
                       JOIN pg_attribute AS attribute
                         ON attribute.attrelid = owner_table.oid
                        AND attribute.attnum = key_column.attnum
                      WHERE NOT attribute.attisdropped
                      ORDER BY key_column.position
                 ) = ARRAY['mail_data_plane_id', 'account_ref']::text[]
          )
   )
   AND EXISTS (
       SELECT 1
         FROM pg_trigger AS account_owner_trigger
        WHERE account_owner_trigger.tgrelid =
                  'public.mail_engine_mailbox_bindings'::regclass
          AND account_owner_trigger.tgname =
                  'mail_engine_mailbox_bindings_enforce_account_owner'
          AND NOT account_owner_trigger.tgisinternal
          AND account_owner_trigger.tgenabled = 'O'
          AND account_owner_trigger.tgtype = 23
          AND account_owner_trigger.tgnargs = 0
          AND account_owner_trigger.tgqual IS NULL
          AND account_owner_trigger.tgfoid =
              'public.enforce_mail_engine_account_owner()'::regprocedure
          AND ARRAY(
              SELECT attribute.attname::text
                FROM unnest(account_owner_trigger.tgattr::smallint[])
                     AS trigger_column(attnum)
                JOIN pg_attribute AS attribute
                  ON attribute.attrelid = account_owner_trigger.tgrelid
                 AND attribute.attnum = trigger_column.attnum
               WHERE NOT attribute.attisdropped
               ORDER BY attribute.attname
          ) = ARRAY[
              'account_ref',
              'mail_data_plane_id',
              'mailbox_id',
              'organization_id',
              'source'
          ]::text[]
    )
    AND EXISTS (
        SELECT 1
          FROM pg_trigger AS owner_mutation_trigger
         WHERE owner_mutation_trigger.tgrelid =
                   'public.mail_engine_account_owners'::regclass
           AND owner_mutation_trigger.tgname =
                   'mail_engine_account_owners_reject_update_delete'
           AND NOT owner_mutation_trigger.tgisinternal
           AND owner_mutation_trigger.tgenabled = 'O'
           AND owner_mutation_trigger.tgtype = 27
           AND owner_mutation_trigger.tgnargs = 0
           AND cardinality(
                   owner_mutation_trigger.tgattr::smallint[]
               ) = 0
           AND owner_mutation_trigger.tgqual IS NULL
           AND owner_mutation_trigger.tgfoid =
               'public.reject_mail_engine_account_owner_mutation()'::regprocedure
    )
    AND EXISTS (
        SELECT 1
          FROM pg_trigger AS owner_truncate_trigger
         WHERE owner_truncate_trigger.tgrelid =
                   'public.mail_engine_account_owners'::regclass
           AND owner_truncate_trigger.tgname =
                   'mail_engine_account_owners_reject_truncate'
           AND NOT owner_truncate_trigger.tgisinternal
           AND owner_truncate_trigger.tgenabled = 'O'
           AND owner_truncate_trigger.tgtype = 34
           AND owner_truncate_trigger.tgnargs = 0
           AND cardinality(
                   owner_truncate_trigger.tgattr::smallint[]
               ) = 0
           AND owner_truncate_trigger.tgqual IS NULL
           AND owner_truncate_trigger.tgfoid =
               'public.reject_mail_engine_account_owner_mutation()'::regprocedure
    )
    AND EXISTS (
        SELECT 1
          FROM pg_trigger AS owner_insert_guard
          JOIN pg_constraint AS owner_insert_constraint
            ON owner_insert_constraint.oid =
                   owner_insert_guard.tgconstraint
         WHERE owner_insert_guard.tgrelid =
                   'public.mail_engine_account_owners'::regclass
           AND owner_insert_guard.tgname =
                   'mail_engine_account_owners_require_current_binding'
           AND NOT owner_insert_guard.tgisinternal
           AND owner_insert_guard.tgenabled = 'O'
           AND owner_insert_guard.tgtype = 5
           AND owner_insert_guard.tgnargs = 0
           AND cardinality(
                   owner_insert_guard.tgattr::smallint[]
               ) = 0
           AND owner_insert_guard.tgqual IS NULL
           AND owner_insert_guard.tgdeferrable
           AND owner_insert_guard.tginitdeferred
           AND owner_insert_guard.tgfoid =
               'public.require_current_binding_for_account_owner()'::regprocedure
           AND owner_insert_constraint.conrelid =
                   owner_insert_guard.tgrelid
           AND owner_insert_constraint.conname =
                   owner_insert_guard.tgname
           AND owner_insert_constraint.contype = 't'
           AND owner_insert_constraint.condeferrable
           AND owner_insert_constraint.condeferred
    )
    AND NOT EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
          LEFT JOIN mail_engine_account_owners AS owner
            ON owner.mail_data_plane_id = binding.mail_data_plane_id
           AND owner.source = binding.source
           AND owner.account_ref = binding.account_ref
         WHERE owner.account_ref IS NULL
            OR owner.organization_id
                   IS DISTINCT FROM binding.organization_id
            OR owner.mailbox_id IS DISTINCT FROM binding.mailbox_id
    )
    AND to_regclass('public.inbound_mail_engine_events') IS NOT NULL
   AND EXISTS (
       SELECT 1
         FROM pg_trigger
        WHERE tgrelid = 'public.inbound_mail_engine_events'::regclass
          AND tgname = 'inbound_mail_engine_events_reject_update_delete'
          AND NOT tgisinternal
   )
   AND to_regclass('public.webhook_subscriptions') IS NOT NULL
   AND to_regclass('public.webhook_deliveries') IS NOT NULL
   AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.webhook_subscriptions'::regclass
          AND attname = 'version'
          AND attnotnull
          AND NOT attisdropped
   )
   AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.webhook_deliveries'::regclass
          AND attname = 'error_code'
          AND NOT attisdropped
   )
   AND EXISTS (
       SELECT 1
         FROM pg_trigger
        WHERE tgrelid = 'public.webhook_deliveries'::regclass
          AND tgname = 'webhook_deliveries_reject_update_delete'
          AND NOT tgisinternal
   )
   AND EXISTS (
       SELECT 1
         FROM audit_event_catalog
        WHERE action = 'agent.webhook_delivery.replayed'
          AND resource_type = 'agent.webhook_delivery'
          AND risk = 'critical'
          AND privileged
   )
   AND EXISTS (
       SELECT 1
         FROM audit_event_catalog
        WHERE action = 'agent.event_cursor.read'
          AND resource_type = 'agent.event_cursor'
          AND risk = 'medium'
          AND privileged
   )
   AND EXISTS (
       SELECT 1
         FROM audit_event_catalog
        WHERE action = 'agent.event_stream.opened'
          AND resource_type = 'agent.event_stream'
          AND risk = 'medium'
          AND privileged
   )
   AND EXISTS (
       SELECT 1
         FROM audit_event_catalog
        WHERE action = 'audit.events.queried'
          AND resource_type = 'audit.query'
          AND risk = 'high'
          AND privileged
   )
   AND EXISTS (
       SELECT 1
         FROM audit_event_catalog
        WHERE action = 'mail.submissions.queried'
          AND resource_type = 'mailbox'
          AND risk = 'medium'
          AND privileged
   )
   AND EXISTS (
       SELECT 1
         FROM pg_index AS sent_directory_index
         JOIN pg_class AS sent_directory_relation
           ON sent_directory_relation.oid = sent_directory_index.indexrelid
         JOIN pg_namespace AS sent_directory_schema
           ON sent_directory_schema.oid = sent_directory_relation.relnamespace
        WHERE sent_directory_schema.nspname = 'public'
          AND sent_directory_relation.relname =
              'mail_submission_intents_human_sent_directory_idx'
          AND sent_directory_index.indisvalid
          AND sent_directory_index.indisready
   )
   AND EXISTS (
       SELECT 1
         FROM audit_event_catalog
        WHERE action = 'mail_engine.mailbox_binding.updated'
          AND resource_type = 'mail_engine.mailbox_binding'
          AND risk = 'high'
          AND privileged
   )
   AND EXISTS (
       SELECT 1
         FROM pg_attribute
        WHERE attrelid = 'public.outbox_events'::regclass
          AND attname = 'next_attempt_at'
          AND attnotnull
          AND NOT attisdropped
   )
