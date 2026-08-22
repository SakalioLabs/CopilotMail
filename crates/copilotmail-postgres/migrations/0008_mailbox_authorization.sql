CREATE UNIQUE INDEX mailboxes_id_organization_idx
    ON mailboxes(id, organization_id);

ALTER TABLE mailbox_memberships
    ADD COLUMN organization_id uuid;

UPDATE mailbox_memberships AS membership
   SET organization_id = mailbox.organization_id
  FROM mailboxes AS mailbox
 WHERE mailbox.id = membership.mailbox_id;

ALTER TABLE mailbox_memberships
    ALTER COLUMN organization_id SET NOT NULL,
    ADD CONSTRAINT mailbox_memberships_organization_fk
        FOREIGN KEY(organization_id) REFERENCES organizations(id),
    ADD CONSTRAINT mailbox_memberships_mailbox_tenant_fk
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id) ON DELETE CASCADE,
    ADD CONSTRAINT mailbox_memberships_principal_tenant_fk
        FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT mailbox_memberships_creator_tenant_fk
        FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id);

CREATE INDEX mailbox_memberships_resolution_idx
    ON mailbox_memberships(principal_id, organization_id, mailbox_id, valid_from, valid_until);

ALTER TABLE delegations
    ADD CONSTRAINT delegations_mailbox_tenant_fk
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id) ON DELETE CASCADE,
    ADD CONSTRAINT delegations_grantor_tenant_fk
        FOREIGN KEY(grantor_principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT delegations_grantee_tenant_fk
        FOREIGN KEY(grantee_principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT delegations_scopes_check CHECK (
        cardinality(scopes) BETWEEN 1 AND 3
        AND scopes <@ ARRAY[
            'mailbox.read',
            'mailbox.send',
            'mailbox.send_as'
        ]::text[]
    );

CREATE INDEX delegations_resolution_idx
    ON delegations(grantee_principal_id, organization_id, mailbox_id, valid_from, valid_until)
    WHERE revoked_at IS NULL;

CREATE UNIQUE INDEX delegations_active_grant_idx
    ON delegations(grantor_principal_id, grantee_principal_id, mailbox_id)
    WHERE revoked_at IS NULL;
