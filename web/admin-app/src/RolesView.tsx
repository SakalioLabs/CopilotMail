import {
  Badge,
  Button,
  ConfirmDialog,
  ContextBar,
  EmptyState,
  EvidenceInspector,
  Field,
  IdentityBadge,
  Input,
  Panel,
  Select,
  Spinner,
  StatusIndicator,
  UnknownWriteLock,
} from "@copilotmail/design-system";
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { AdminErrorNotice } from "./AdminErrorNotice";
import { ControlPlaneClient, isUnknownWriteFailure } from "./api";
import { translator } from "./i18n";
import type { Locale, Page, Role, RoleAssignment } from "./types";

type T = ReturnType<typeof translator>;

function formatDate(value: string, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

/**
 * `valid_until` is the only nullable date in the role-assignment DTO.  A
 * null value is a positive authorization fact (the assignment has no expiry),
 * not missing evidence, so it must never fall back to a visual placeholder.
 */
function formatAssignmentExpiry(value: string | null, locale: Locale, t: T): string {
  return value === null ? t("noExpiry") : formatDate(value, locale);
}

function shortId(value: string): string {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
}

function PrincipalIdentity({ kind, t }: { kind: RoleAssignment["principal_kind"]; t: T }) {
  const label = kind === "human"
    ? t("identityHuman")
    : kind === "agent"
      ? t("identityAgent")
      : t("identityService");

  return <IdentityBadge kind={kind} label={label} />;
}

function initialExpiry(): string {
  const value = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000);
  value.setSeconds(0, 0);
  const local = new Date(value.getTime() - value.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

interface GrantReview {
  principalId: string;
  role: Role;
  validUntil: string;
  approvalRequestId: string;
  approvalResource: string;
}

type UnknownRoleWrite =
  | {
    kind: "grant";
    principalId: string;
    role: Role;
    validUntil: string;
    approvalRequestId: string;
    approvalResource: string;
  }
  | {
    kind: "revoke";
    assignment: RoleAssignment;
  };

function AssignmentEvidence({
  assignment,
  locale,
  t,
}: {
  assignment: RoleAssignment;
  locale: Locale;
  t: T;
}) {
  return (
    <EvidenceInspector
      className="role-assignment-evidence"
      summary={t("roleAssignmentEvidence")}
    >
      <dl className="role-evidence-facts">
        <dt>{t("roleAssignmentId")}</dt>
        <dd><code>{assignment.role_assignment_id}</code></dd>
        <dt>{t("principalId")}</dt>
        <dd><code>{assignment.principal_id}</code></dd>
        <dt>{t("roleId")}</dt>
        <dd><code>{assignment.role_id}</code></dd>
        <dt>{t("roleCreatedBy")}</dt>
        <dd><code>{assignment.created_by}</code></dd>
        <dt>{t("roleCreatedAt")}</dt>
        <dd>{formatDate(assignment.created_at, locale)}</dd>
        {assignment.approval_request_id ? (
          <>
            <dt>{t("approvalId")}</dt>
            <dd><code>{assignment.approval_request_id}</code></dd>
          </>
        ) : null}
        {assignment.revoked_at ? (
          <>
            <dt>{t("revokedAt")}</dt>
            <dd>{formatDate(assignment.revoked_at, locale)}</dd>
          </>
        ) : null}
      </dl>
    </EvidenceInspector>
  );
}

function RoleEvidence({ role, locale, t }: { role: Role; locale: Locale; t: T }) {
  return (
    <EvidenceInspector className="role-catalog-evidence" summary={t("roleCatalogEvidence")}>
      <dl className="role-evidence-facts">
        <dt>{t("roleId")}</dt>
        <dd><code>{role.role_id}</code></dd>
        <dt>{t("organizationId")}</dt>
        <dd><code>{role.organization_id}</code></dd>
        <dt>{t("roleCreatedAt")}</dt>
        <dd>{formatDate(role.created_at, locale)}</dd>
      </dl>
    </EvidenceInspector>
  );
}

export default function RolesView({
  client,
  locale,
  t,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: T;
}) {
  const [roles, setRoles] = useState<Role[]>([]);
  const [page, setPage] = useState<Page<RoleAssignment>>();
  const [cursors, setCursors] = useState<Array<string | undefined>>([undefined]);
  const [principalFilter, setPrincipalFilter] = useState("");
  const [appliedPrincipalFilter, setAppliedPrincipalFilter] = useState("");
  const [includeInactive, setIncludeInactive] = useState(false);
  const [queryRevision, setQueryRevision] = useState(0);
  const [principalId, setPrincipalId] = useState("");
  const [roleId, setRoleId] = useState("");
  const [validUntil, setValidUntil] = useState(initialExpiry);
  const [approvalId, setApprovalId] = useState("");
  const [loading, setLoading] = useState(true);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<unknown>();
  const [pendingGrant, setPendingGrant] = useState<GrantReview>();
  const [pendingRevoke, setPendingRevoke] = useState<RoleAssignment>();
  const [confirmedGrant, setConfirmedGrant] = useState<RoleAssignment>();
  const [unknownWrite, setUnknownWrite] = useState<UnknownRoleWrite>();
  const grantInFlightRef = useRef(false);
  const revokeInFlightRef = useRef(false);
  const listRequestSequence = useRef(0);
  const revokeRequestSequence = useRef(0);
  const committedRevocationsRef = useRef(new Map<string, RoleAssignment>());
  const assignmentFilterRef = useRef<HTMLInputElement>(null);
  const focusFilterAfterRevokeRef = useRef(false);

  const cursor = cursors.at(-1);
  const load = useCallback(async () => {
    const sequence = listRequestSequence.current + 1;
    listRequestSequence.current = sequence;
    setLoading(true);
    setError(undefined);
    try {
      const [roleResult, assignmentResult] = await Promise.all([
        client.listRoles(),
        client.listRoleAssignments({
          principal_id: appliedPrincipalFilter || undefined,
          include_inactive: includeInactive,
          cursor,
          limit: 50,
        }),
      ]);
      if (sequence !== listRequestSequence.current) return;
      const committedRevocations = committedRevocationsRef.current;
      const reconciledAssignments = includeInactive
        ? assignmentResult.items.map((assignment) => (
          committedRevocations.get(assignment.role_assignment_id) ?? assignment
        ))
        : assignmentResult.items.filter((assignment) => (
          !committedRevocations.has(assignment.role_assignment_id)
        ));
      setRoles(roleResult.items);
      setPage({ ...assignmentResult, items: reconciledAssignments });
      setRoleId((current) => current || roleResult.items[0]?.role_id || "");
    } catch (cause) {
      if (sequence === listRequestSequence.current) setError(cause);
    } finally {
      if (sequence === listRequestSequence.current) setLoading(false);
    }
  }, [appliedPrincipalFilter, client, cursor, includeInactive, queryRevision]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => () => {
    listRequestSequence.current += 1;
    revokeRequestSequence.current += 1;
  }, []);

  useEffect(() => {
    if (!pendingRevoke && focusFilterAfterRevokeRef.current) {
      focusFilterAfterRevokeRef.current = false;
      assignmentFilterRef.current?.focus();
    }
  }, [pendingRevoke]);

  function prepareAssignmentQueryChange() {
    listRequestSequence.current += 1;
    setLoading(true);
    setPage(undefined);
    setError(undefined);
  }

  const approvalResource = useMemo(() => {
    if (!principalId.trim() || !roleId || !validUntil) return "";
    const expires = new Date(validUntil);
    if (Number.isNaN(expires.getTime())) return "";
    return `v1:${principalId.trim()}:${roleId}:${expires.getTime()}`;
  }, [principalId, roleId, validUntil]);

  const selectedRole = useMemo(
    () => roles.find((role) => role.role_id === roleId),
    [roleId, roles],
  );
  const contextOrganizationId = roles[0]?.organization_id
    ?? page?.items[0]?.organization_id;
  const writeFrozen = unknownWrite !== undefined;

  const contextFacts = [
    ...(contextOrganizationId ? [{
      id: "organization",
      label: t("organizationId"),
      value: <code>{contextOrganizationId}</code>,
    }] : []),
    {
      id: "principal-filter",
      label: t("roleContextPrincipal"),
      value: appliedPrincipalFilter
        ? <code>{appliedPrincipalFilter}</code>
        : t("roleContextAllPrincipals"),
    },
    {
      id: "activity-filter",
      label: t("roleContextActivity"),
      value: includeInactive
        ? t("includeInactive")
        : t("roleContextActiveOnly"),
    },
    ...(page ? [{
      id: "returned-assignments",
      label: t("roleContextReturnedAssignments"),
      value: String(page.items.length),
    }] : []),
  ];

  function reviewGrant(event: FormEvent) {
    event.preventDefault();
    if (
      writeFrozen
      || grantInFlightRef.current
      || revokeInFlightRef.current
      || !principalId.trim()
      || !selectedRole
      || !validUntil
      || !approvalId.trim()
      || !approvalResource
    ) return;
    setError(undefined);
    setPendingGrant({
      principalId: principalId.trim(),
      role: selectedRole,
      validUntil: new Date(validUntil).toISOString(),
      approvalRequestId: approvalId.trim(),
      approvalResource,
    });
  }

  async function grant() {
    const review = pendingGrant;
    if (
      !review
      || writeFrozen
      ||
      grantInFlightRef.current
      || revokeInFlightRef.current
    ) return;
    grantInFlightRef.current = true;
    setActing(true);
    setError(undefined);
    try {
      const granted = await client.grantRoleAssignment({
        principal_id: review.principalId,
        role_id: review.role.role_id,
        valid_until: review.validUntil,
        approval_request_id: review.approvalRequestId,
      });
      setConfirmedGrant(granted);
      setPrincipalId("");
      setApprovalId("");
      setValidUntil(initialExpiry());
      // A refresh must be scheduled by the latest render. Calling this event
      // handler's captured load function could bind an obsolete filter/cursor
      // after the operator has already selected a newer query.
      listRequestSequence.current += 1;
      setLoading(true);
      setCursors([undefined]);
      setQueryRevision((value) => value + 1);
    } catch (cause) {
      if (isUnknownWriteFailure(cause)) {
        setUnknownWrite({
          kind: "grant",
          principalId: review.principalId,
          role: review.role,
          validUntil: review.validUntil,
          approvalRequestId: review.approvalRequestId,
          approvalResource: review.approvalResource,
        });
      } else {
        setError(cause);
      }
    } finally {
      grantInFlightRef.current = false;
      setActing(false);
      setPendingGrant(undefined);
    }
  }

  async function revoke() {
    const assignment = pendingRevoke;
    if (
      !assignment
      || writeFrozen
      || revokeInFlightRef.current
      || grantInFlightRef.current
    ) return;
    const sequence = revokeRequestSequence.current + 1;
    revokeRequestSequence.current = sequence;
    revokeInFlightRef.current = true;
    setActing(true);
    setError(undefined);
    try {
      const revoked = await client.revokeRoleAssignment(
        assignment.role_assignment_id,
      );
      if (sequence !== revokeRequestSequence.current) return;
      committedRevocationsRef.current.set(
        assignment.role_assignment_id,
        revoked,
      );
      setPage((current) => current ? {
        ...current,
        items: includeInactive
          ? current.items.map((item) => (
            item.role_assignment_id === assignment.role_assignment_id
              ? revoked
              : item
          ))
          : current.items.filter((item) => (
            item.role_assignment_id !== assignment.role_assignment_id
          )),
      } : current);
      focusFilterAfterRevokeRef.current = true;
      await load();
    } catch (cause) {
      if (sequence !== revokeRequestSequence.current) return;
      if (isUnknownWriteFailure(cause)) {
        setUnknownWrite({ kind: "revoke", assignment });
      } else {
        setError(cause);
      }
    } finally {
      revokeInFlightRef.current = false;
      if (sequence === revokeRequestSequence.current) {
        setActing(false);
        setPendingRevoke(undefined);
      }
    }
  }

  return (
    <section className="view-stack" aria-labelledby="roles-title">
      <header className="view-heading">
        <div>
          <p className="eyebrow">{t("authorizationGovernance")}</p>
          <h1 id="roles-title">{t("rolesTitle")}</h1>
          <p>{t("rolesHint")}</p>
        </div>
      </header>

      {error ? <AdminErrorNotice error={error} t={t} /> : null}

      {page ? (
        <ContextBar
          className="roles-context"
          facts={contextFacts}
          title={t("rolesCurrentContext")}
        />
      ) : null}

      {unknownWrite ? (
        <UnknownWriteLock
          aria-label={t("roleWriteOutcomeUnknown")}
          className="role-unknown-write"
          description={unknownWrite.kind === "grant"
            ? t("roleGrantOutcomeUnknown")
            : t("roleRevokeOutcomeUnknown")}
          evidence={unknownWrite.kind === "grant" ? (
            <dl className="role-write-evidence">
              <dt>{t("principalId")}</dt>
              <dd><code>{unknownWrite.principalId}</code></dd>
              <dt>{t("role")}</dt>
              <dd>{unknownWrite.role.display_name}</dd>
              <dt>{t("validUntil")}</dt>
              <dd>{formatDate(unknownWrite.validUntil, locale)}</dd>
              <dt>{t("approvalId")}</dt>
              <dd><code>{unknownWrite.approvalRequestId}</code></dd>
              <dt>{t("roleApprovalResource")}</dt>
              <dd><code>{unknownWrite.approvalResource}</code></dd>
            </dl>
          ) : (
            <dl className="role-write-evidence">
              <dt>{t("roleAssignmentId")}</dt>
              <dd><code>{unknownWrite.assignment.role_assignment_id}</code></dd>
              <dt>{t("principalId")}</dt>
              <dd><code>{unknownWrite.assignment.principal_id}</code></dd>
              <dt>{t("role")}</dt>
              <dd>{unknownWrite.assignment.role_display_name}</dd>
            </dl>
          )}
          title={t("roleWriteOutcomeUnknown")}
        />
      ) : null}

      {confirmedGrant ? (
        <div className="notice" role="status" aria-label={t("roleGrantConfirmed")}>
          <div>
            <strong>{t("roleGrantConfirmed")}</strong>
            <p>{t("roleGrantConfirmedHint")}</p>
            <p>
              <span>{t("principal")}: </span>{confirmedGrant.principal_display_name}{" · "}
              <span>{t("role")}:</span> {confirmedGrant.role_display_name}{" · "}
              <span>{t("validUntil")}:</span>{" "}
              {formatAssignmentExpiry(confirmedGrant.valid_until, locale, t)}
            </p>
            <AssignmentEvidence assignment={confirmedGrant} locale={locale} t={t} />
          </div>
          <StatusIndicator
            tone={confirmedGrant.active ? "positive" : "neutral"}
            label={confirmedGrant.active ? t("activeRole") : t("inactiveRole")}
          />
        </div>
      ) : null}

      <Panel title={t("roleCatalog")}>
        {loading && !roles.length ? <Spinner label={t("loading")} /> : (
          <div className="role-catalog">
            {roles.map((role) => (
              <article className="role-card" key={role.role_id}>
                <div className="role-card__title">
                  <strong>{role.display_name}</strong>
                  <Badge tone="neutral">{role.system_role}</Badge>
                </div>
                <p>{role.permissions.join(" · ")}</p>
                <RoleEvidence locale={locale} role={role} t={t} />
              </article>
            ))}
          </div>
        )}
      </Panel>

      <Panel title={t("roleGrantDecision")}>
        <p className="role-decision-intro">{t("roleGrantDecisionHint")}</p>
        <form className="role-grant-form" onSubmit={reviewGrant}>
          <Field label={t("principalId")}>
            <Input
              disabled={acting || writeFrozen}
              value={principalId}
              onChange={(event) => setPrincipalId(event.target.value)}
              required
            />
          </Field>
          <Field label={t("role")}>
            <Select
              disabled={acting || writeFrozen}
              value={roleId}
              onChange={(event) => setRoleId(event.target.value)}
              required
            >
              {roles.map((role) => (
                <option key={role.role_id} value={role.role_id}>{role.display_name}</option>
              ))}
            </Select>
          </Field>
          <Field label={t("validUntil")}>
            <Input
              disabled={acting || writeFrozen}
              type="datetime-local"
              value={validUntil}
              onChange={(event) => setValidUntil(event.target.value)}
              required
            />
          </Field>
          <Field label={t("approvalId")}>
            <Input
              disabled={acting || writeFrozen}
              value={approvalId}
              onChange={(event) => setApprovalId(event.target.value)}
              required
            />
          </Field>
          <div className="role-approval-resource">
            <small>{t("roleApprovalResource")}</small>
            <code>{approvalResource || t("roleApprovalResourcePending")}</code>
            <p>{t("roleApprovalHint")}</p>
          </div>
          <Button type="submit" disabled={acting || writeFrozen || !approvalResource}>
            {t("reviewRoleGrant")}
          </Button>
        </form>
      </Panel>

      <Panel title={t("roleAssignments")}>
        <form
          className="role-filter"
          onSubmit={(event) => {
            event.preventDefault();
            if (acting || writeFrozen) return;
            prepareAssignmentQueryChange();
            setCursors([undefined]);
            setAppliedPrincipalFilter(principalFilter.trim());
            setQueryRevision((value) => value + 1);
          }}
        >
          <Field label={t("principalFilter")}>
            <Input
              ref={assignmentFilterRef}
              disabled={acting || writeFrozen}
              value={principalFilter}
              onChange={(event) => setPrincipalFilter(event.target.value)}
            />
          </Field>
          <label className="checkbox-field">
            <input
              type="checkbox"
              disabled={acting || writeFrozen}
              checked={includeInactive}
              onChange={(event) => {
                prepareAssignmentQueryChange();
                setIncludeInactive(event.target.checked);
                setCursors([undefined]);
              }}
            />
            {t("includeInactive")}
          </label>
          <Button type="submit" variant="secondary" disabled={acting || writeFrozen}>
            {t("applyFilters")}
          </Button>
        </form>

        {loading && !page ? <Spinner label={t("loading")} /> : null}
        {!loading && page?.items.length === 0 ? (
          <EmptyState title={t("noRoleAssignments")} description={t("noRoleAssignmentsHint")} />
        ) : null}
        {page?.items.length ? (
          <div className="role-assignment-list">
            {page.items.map((assignment) => (
              <article className="role-assignment-row" key={assignment.role_assignment_id}>
                <div>
                  <strong>{assignment.principal_display_name}</strong>
                  <p>
                    <span className="mono">{shortId(assignment.principal_id)}</span>{" "}
                    <PrincipalIdentity kind={assignment.principal_kind} t={t} />
                  </p>
                </div>
                <div>
                  <strong>{assignment.role_display_name}</strong>
                  <p>
                    {formatDate(assignment.valid_from, locale)} →{" "}
                    {formatAssignmentExpiry(assignment.valid_until, locale, t)}
                  </p>
                </div>
                <StatusIndicator
                  tone={assignment.active ? "positive" : "neutral"}
                  label={assignment.active ? t("activeRole") : t("inactiveRole")}
                />
                {assignment.active && !writeFrozen ? (
                  <Button
                    variant="secondary"
                    disabled={acting}
                    onClick={() => setPendingRevoke(assignment)}
                  >
                    {t("revokeRole")}
                  </Button>
                ) : null}
                <AssignmentEvidence assignment={assignment} locale={locale} t={t} />
              </article>
            ))}
          </div>
        ) : null}
        {cursors.length > 1 || Boolean(page?.has_more && page.next_cursor) ? (
          <nav className="pagination" aria-label={t("roleAssignmentsPagination")}>
            <Button
              variant="secondary"
              disabled={cursors.length === 1 || loading || acting || writeFrozen}
              onClick={() => {
                prepareAssignmentQueryChange();
                setCursors((value) => value.slice(0, -1));
              }}
            >
              {t("previous")}
            </Button>
            <Button
              variant="secondary"
              disabled={!page?.has_more || !page.next_cursor || loading || acting || writeFrozen}
              onClick={() => {
                if (!page?.next_cursor) return;
                prepareAssignmentQueryChange();
                setCursors((value) => [...value, page.next_cursor!]);
              }}
            >
              {t("next")}
            </Button>
          </nav>
        ) : null}
      </Panel>

      {pendingGrant ? (
        <ConfirmDialog
          busy={acting}
          cancelLabel={t("cancel")}
          closeLabel={t("closeDialog")}
          confirmLabel={t("grantRole")}
          confirmVariant="primary"
          description={t("confirmGrantRole")}
          title={t("reviewRoleGrant")}
          onCancel={() => {
            if (!acting) setPendingGrant(undefined);
          }}
          onConfirm={() => void grant()}
        >
          <dl className="role-confirmation-facts">
            <dt>{t("principalId")}</dt>
            <dd><code>{pendingGrant.principalId}</code></dd>
            <dt>{t("role")}</dt>
            <dd>{pendingGrant.role.display_name} <Badge tone="neutral">{pendingGrant.role.system_role}</Badge></dd>
            <dt>{t("permissions")}</dt>
            <dd>{pendingGrant.role.permissions.join(" · ")}</dd>
            <dt>{t("validUntil")}</dt>
            <dd>{formatDate(pendingGrant.validUntil, locale)}</dd>
            <dt>{t("approvalId")}</dt>
            <dd><code>{pendingGrant.approvalRequestId}</code></dd>
            <dt>{t("roleApprovalResource")}</dt>
            <dd><code>{pendingGrant.approvalResource}</code></dd>
          </dl>
        </ConfirmDialog>
      ) : null}

      {pendingRevoke ? (
        <ConfirmDialog
          busy={acting}
          cancelLabel={t("cancel")}
          closeLabel={t("closeDialog")}
          confirmLabel={t("revokeRole")}
          description={t("confirmRevokeRole")}
          title={t("revokeRole")}
          onCancel={() => {
            if (!acting) setPendingRevoke(undefined);
          }}
          onConfirm={() => void revoke()}
        >
          <dl className="role-confirmation-facts">
            <dt>{t("roleAssignmentId")}</dt>
            <dd><code>{pendingRevoke.role_assignment_id}</code></dd>
            <dt>{t("principal")}</dt>
            <dd>
              <strong>{pendingRevoke.principal_display_name}</strong>{" "}
              <PrincipalIdentity kind={pendingRevoke.principal_kind} t={t} />
              <code>{pendingRevoke.principal_id}</code>
            </dd>
            <dt>{t("role")}</dt>
            <dd>{pendingRevoke.role_display_name} <Badge tone="neutral">{pendingRevoke.system_role}</Badge></dd>
            <dt>{t("validity")}</dt>
            <dd>
              {formatDate(pendingRevoke.valid_from, locale)} →{" "}
              {formatAssignmentExpiry(pendingRevoke.valid_until, locale, t)}
            </dd>
            {pendingRevoke.approval_request_id ? (
              <>
                <dt>{t("approvalId")}</dt>
                <dd><code>{pendingRevoke.approval_request_id}</code></dd>
              </>
            ) : null}
            <dt>{t("roleCreatedBy")}</dt>
            <dd><code>{pendingRevoke.created_by}</code></dd>
          </dl>
        </ConfirmDialog>
      ) : null}
    </section>
  );
}
