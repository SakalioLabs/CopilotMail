import {
  Badge,
  Button,
  EmptyState,
  Field,
  Input,
  Panel,
  Select,
  Spinner,
} from "@copilotmail/design-system";
import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { ApiError, ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type { Locale, Page, Role, RoleAssignment } from "./types";

type T = ReturnType<typeof translator>;

function formatDate(value: string | null, locale: Locale): string {
  if (!value) return "—";
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function shortId(value: string): string {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
}

function errorText(error: unknown): string {
  if (error instanceof ApiError) {
    return `${error.body.code}: ${error.body.message} · ${error.body.request_id}`;
  }
  return error instanceof Error ? error.message : "unexpected_error";
}

function initialExpiry(): string {
  const value = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000);
  value.setSeconds(0, 0);
  const local = new Date(value.getTime() - value.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
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
  const [principalId, setPrincipalId] = useState("");
  const [roleId, setRoleId] = useState("");
  const [validUntil, setValidUntil] = useState(initialExpiry);
  const [approvalId, setApprovalId] = useState("");
  const [loading, setLoading] = useState(true);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<unknown>();

  const cursor = cursors.at(-1);
  const load = useCallback(async () => {
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
      setRoles(roleResult.items);
      setPage(assignmentResult);
      if (!roleId && roleResult.items.length) {
        setRoleId(roleResult.items[0]!.role_id);
      }
    } catch (cause) {
      setError(cause);
    } finally {
      setLoading(false);
    }
  }, [appliedPrincipalFilter, client, cursor, includeInactive, roleId]);

  useEffect(() => {
    void load();
  }, [load]);

  const approvalResource = useMemo(() => {
    if (!principalId.trim() || !roleId || !validUntil) return "";
    const expires = new Date(validUntil);
    if (Number.isNaN(expires.getTime())) return "";
    return `v1:${principalId.trim()}:${roleId}:${expires.getTime()}`;
  }, [principalId, roleId, validUntil]);

  async function grant(event: FormEvent) {
    event.preventDefault();
    if (!principalId.trim() || !roleId || !validUntil || !approvalId.trim()) return;
    setActing(true);
    setError(undefined);
    try {
      await client.grantRoleAssignment({
        principal_id: principalId.trim(),
        role_id: roleId,
        valid_until: new Date(validUntil).toISOString(),
        approval_request_id: approvalId.trim(),
      });
      setApprovalId("");
      setCursors([undefined]);
      await load();
    } catch (cause) {
      setError(cause);
    } finally {
      setActing(false);
    }
  }

  async function revoke(assignment: RoleAssignment) {
    if (!window.confirm(t("confirmRevokeRole"))) return;
    setActing(true);
    setError(undefined);
    try {
      await client.revokeRoleAssignment(assignment.role_assignment_id);
      await load();
    } catch (cause) {
      setError(cause);
    } finally {
      setActing(false);
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

      {error ? <div className="notice notice--error" role="alert">{errorText(error)}</div> : null}

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
              </article>
            ))}
          </div>
        )}
      </Panel>

      <Panel title={t("grantRole")}>
        <form className="role-grant-form" onSubmit={grant}>
          <Field label={t("principalId")}>
            <Input value={principalId} onChange={(event) => setPrincipalId(event.target.value)} required />
          </Field>
          <Field label={t("role")}>
            <Select value={roleId} onChange={(event) => setRoleId(event.target.value)} required>
              {roles.map((role) => (
                <option key={role.role_id} value={role.role_id}>{role.display_name}</option>
              ))}
            </Select>
          </Field>
          <Field label={t("validUntil")}>
            <Input type="datetime-local" value={validUntil} onChange={(event) => setValidUntil(event.target.value)} required />
          </Field>
          <Field label={t("approvalId")}>
            <Input value={approvalId} onChange={(event) => setApprovalId(event.target.value)} required />
          </Field>
          <div className="role-approval-resource">
            <small>{t("roleApprovalResource")}</small>
            <code>{approvalResource || "—"}</code>
            <p>{t("roleApprovalHint")}</p>
          </div>
          <Button type="submit" disabled={acting || !approvalResource}>{t("grantRole")}</Button>
        </form>
      </Panel>

      <Panel title={t("roleAssignments")}>
        <form
          className="role-filter"
          onSubmit={(event) => {
            event.preventDefault();
            setCursors([undefined]);
            setAppliedPrincipalFilter(principalFilter.trim());
          }}
        >
          <Field label={t("principalFilter")}>
            <Input value={principalFilter} onChange={(event) => setPrincipalFilter(event.target.value)} />
          </Field>
          <label className="checkbox-field">
            <input
              type="checkbox"
              checked={includeInactive}
              onChange={(event) => {
                setIncludeInactive(event.target.checked);
                setCursors([undefined]);
              }}
            />
            {t("includeInactive")}
          </label>
          <Button type="submit" variant="secondary">{t("applyFilters")}</Button>
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
                  <p>{shortId(assignment.principal_id)} · {assignment.principal_kind}</p>
                </div>
                <div>
                  <strong>{assignment.role_display_name}</strong>
                  <p>{formatDate(assignment.valid_from, locale)} → {formatDate(assignment.valid_until, locale)}</p>
                </div>
                <Badge tone={assignment.active ? "positive" : "neutral"}>
                  {assignment.active ? t("activeRole") : t("inactiveRole")}
                </Badge>
                {assignment.active ? (
                  <Button variant="secondary" disabled={acting} onClick={() => void revoke(assignment)}>
                    {t("revokeRole")}
                  </Button>
                ) : null}
              </article>
            ))}
          </div>
        ) : null}
        <nav className="pagination" aria-label="Role assignment pagination">
          <Button
            variant="secondary"
            disabled={cursors.length === 1 || loading}
            onClick={() => setCursors((value) => value.slice(0, -1))}
          >
            {t("previous")}
          </Button>
          <Button
            variant="secondary"
            disabled={!page?.has_more || !page.next_cursor || loading}
            onClick={() => page?.next_cursor && setCursors((value) => [...value, page.next_cursor!])}
          >
            {t("next")}
          </Button>
        </nav>
      </Panel>
    </section>
  );
}
