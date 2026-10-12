import {
  Button,
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
import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";
import { AdminErrorNotice } from "./AdminErrorNotice";
import { AdminListDetail } from "./AdminListDetail";
import { ControlPlaneClient, isUnknownWriteFailure } from "./api";
import type { translator } from "./i18n";
import type {
  AuditEvent,
  AuditEventFilters,
  AuditEventSearchPage,
  AuditExportArtifact,
  AuditExportGrant,
  AuditOutcome,
  Locale,
} from "./types";

type Translate = ReturnType<typeof translator>;

interface AuditExportCommand {
  archiveId: string;
  approvalRequestId: string;
}

interface AuditExportEvidence {
  command: AuditExportCommand;
  grant: AuditExportGrant;
}

type AuditExportFailure =
  | { kind: "authorize"; cause: unknown; command: AuditExportCommand }
  | {
    kind: "download";
    cause: unknown;
    grantId: string;
  };

export interface AuditDraftFilters {
  from: string;
  to: string;
  action: string;
  actor: string;
  resourceType: string;
  resourceId: string;
  outcome: AuditOutcome | "";
  requestId: string;
}

type AuditFieldValidation = "resource" | "time";

/**
 * Script focus does not match `:focus-visible` unless this option is set.
 * The checked-in DOM lib omits `focusVisible`.
 */
function focusWithVisibleRing(element: HTMLInputElement | null) {
  if (!element) return;
  const options: FocusOptions & { focusVisible: boolean } = { focusVisible: true };
  element.focus(options);
}

const MAX_WINDOW_MS = 31 * 24 * 60 * 60 * 1000;

function localInput(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function preset(hours: number): Pick<AuditDraftFilters, "from" | "to"> {
  const to = new Date();
  return {
    from: localInput(new Date(to.getTime() - hours * 60 * 60 * 1000)),
    to: localInput(to),
  };
}

function initialDraft(): AuditDraftFilters {
  return {
    ...preset(24),
    action: "",
    actor: "",
    resourceType: "",
    resourceId: "",
    outcome: "",
    requestId: "",
  };
}

export function auditFiltersFromDraft(draft: AuditDraftFilters): AuditEventFilters | null {
  const from = new Date(draft.from);
  const to = new Date(draft.to);
  if (
    !Number.isFinite(from.getTime()) ||
    !Number.isFinite(to.getTime()) ||
    from > to ||
    to.getTime() - from.getTime() > MAX_WINDOW_MS
  ) {
    return null;
  }
  return {
    occurred_from: from.toISOString(),
    occurred_to: to.toISOString(),
    action: draft.action.trim() || undefined,
    actor_principal_id: draft.actor.trim() || undefined,
    resource_type: draft.resourceType.trim() || undefined,
    resource_id: draft.resourceId.trim() || undefined,
    outcome: draft.outcome || undefined,
    request_id: draft.requestId.trim() || undefined,
  };
}

function formatDate(value: string, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(new Date(value));
}

function shortId(value: string): string {
  return value.length > 22 ? `${value.slice(0, 10)}…${value.slice(-8)}` : value;
}

function OutcomeBadge({ outcome, t }: { outcome: AuditOutcome; t: Translate }) {
  const tone = outcome === "success" ? "positive" : outcome === "failure" ? "critical" : "warning";
  return <StatusIndicator tone={tone} label={t(outcome)} />;
}

function ActorIdentity({ kind, t }: { kind: AuditEvent["actor_kind"]; t: Translate }) {
  const label = kind === "human"
    ? t("identityHuman")
    : kind === "agent"
      ? t("identityAgent")
      : t("identityService");

  return <IdentityBadge kind={kind} label={label} />;
}

function OptionalEvidence({ label, value }: { label: string; value: string | null }) {
  if (!value) return null;
  return <><dt>{label}</dt><dd className="mono">{value}</dd></>;
}

interface AuditQueryFilterFact {
  id: string;
  label: string;
  value: string;
}

/**
 * Builds the visible filter boundary from the exact request contract. The
 * search response does not echo filters, so this deliberately reflects only
 * the active request that was sent and never derives facets from its items.
 */
function auditQueryFilterFacts(
  filters: AuditEventFilters,
  t: Translate,
): AuditQueryFilterFact[] {
  const facts: AuditQueryFilterFact[] = [];
  if (filters.action) facts.push({ id: "action", label: t("action"), value: filters.action });
  if (filters.actor_principal_id) {
    facts.push({ id: "actor", label: t("actor"), value: filters.actor_principal_id });
  }
  if (filters.resource_type) {
    facts.push({ id: "resource-type", label: t("resourceType"), value: filters.resource_type });
  }
  if (filters.resource_id) {
    facts.push({ id: "resource-id", label: t("resourceId"), value: filters.resource_id });
  }
  if (filters.outcome) facts.push({ id: "outcome", label: t("outcome"), value: t(filters.outcome) });
  if (filters.request_id) {
    facts.push({ id: "request-id", label: t("requestFilter"), value: filters.request_id });
  }
  return facts;
}

export default function AuditView({
  client,
  locale,
  t,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: Translate;
}) {
  const initial = useMemo(initialDraft, []);
  const [draft, setDraft] = useState<AuditDraftFilters>(initial);
  const [activeFilters, setActiveFilters] = useState<AuditEventFilters>(() => auditFiltersFromDraft(initial)!);
  const [lockedWindow, setLockedWindow] = useState<{ from: string; to: string }>();
  const [page, setPage] = useState<AuditEventSearchPage>();
  const [cursorHistory, setCursorHistory] = useState<(string | undefined)[]>([undefined]);
  const [selected, setSelected] = useState<AuditEvent>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>();
  const [validationError, setValidationError] = useState<AuditFieldValidation>();
  const validationMessageId = useId();
  const resourceTypeRef = useRef<HTMLInputElement>(null);
  const fromRef = useRef<HTMLInputElement>(null);
  const advancedFiltersRef = useRef<HTMLDetailsElement>(null);
  const [exportArchiveId, setExportArchiveId] = useState("");
  const [exportApprovalId, setExportApprovalId] = useState("");
  const [exportEvidence, setExportEvidence] = useState<AuditExportEvidence>();
  const [uncertainExport, setUncertainExport] = useState<AuditExportCommand>();
  const [exportFailure, setExportFailure] = useState<AuditExportFailure>();
  const [exportGrantReconciliation, setExportGrantReconciliation] = useState<AuditExportGrant>();
  const [exportReconciliationError, setExportReconciliationError] = useState<unknown>();
  const [exportClock, setExportClock] = useState(() => Date.now());
  const [authorizingExport, setAuthorizingExport] = useState(false);
  const [downloadingArtifact, setDownloadingArtifact] = useState<AuditExportArtifact>();
  const [reconcilingExportGrant, setReconcilingExportGrant] = useState(false);
  const requestSequence = useRef(0);
  const exportAuthorizationInFlight = useRef(false);
  const exportRevision = useRef(0);
  const cursor = cursorHistory.at(-1);
  const validationMessage = validationError === "resource"
    ? t("resourceRequired")
    : validationError === "time"
      ? t("invalidTimeRange")
      : undefined;
  const queryFilterFacts = useMemo(
    () => auditQueryFilterFacts(activeFilters, t),
    [activeFilters, t],
  );
  const currentExportGrant = exportEvidence
    && exportEvidence.command.archiveId === exportArchiveId.trim()
    && exportEvidence.command.approvalRequestId === exportApprovalId.trim()
    ? exportEvidence.grant
    : undefined;
  const exportExpired = currentExportGrant
    ? exportClock >= Date.parse(currentExportGrant.expires_at)
    : false;

  useEffect(() => {
    if (!currentExportGrant || exportExpired) return;
    const remaining = Date.parse(currentExportGrant.expires_at) - Date.now();
    const timer = window.setTimeout(
      () => setExportClock(Date.now()),
      Math.min(Math.max(remaining + 1, 1), 2_147_483_647),
    );
    return () => window.clearTimeout(timer);
  }, [currentExportGrant?.audit_export_grant_id, currentExportGrant?.expires_at, exportClock]);

  async function load() {
    const sequence = requestSequence.current + 1;
    requestSequence.current = sequence;
    setLoading(true);
    setError(undefined);
    try {
      const result = await client.searchAuditEvents({
        ...activeFilters,
        occurred_from: lockedWindow?.from ?? activeFilters.occurred_from,
        occurred_to: lockedWindow?.to ?? activeFilters.occurred_to,
      }, { cursor, limit: 50 });
      if (sequence !== requestSequence.current) return;
      setPage(result);
      setSelected((current) => current
        ? result.items.find((item) => item.event_id === current.event_id)
        : undefined);
      setLockedWindow({ from: result.occurred_from, to: result.occurred_to });
    } catch (cause) {
      if (sequence !== requestSequence.current) return;
      setError(cause);
    } finally {
      if (sequence === requestSequence.current) setLoading(false);
    }
  }

  useEffect(() => {
    void load();
    return () => {
      requestSequence.current += 1;
    };
  }, [activeFilters, cursor]);

  function prepareQueryChange({ resetWindow = false } = {}) {
    requestSequence.current += 1;
    setLoading(true);
    setPage(undefined);
    setSelected(undefined);
    setError(undefined);
    if (resetWindow) setLockedWindow(undefined);
  }

  function update<Key extends keyof AuditDraftFilters>(key: Key, value: AuditDraftFilters[Key]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  function runSearch(event: FormEvent) {
    event.preventDefault();
    if (draft.resourceId.trim() && !draft.resourceType.trim()) {
      setValidationError("resource");
      window.requestAnimationFrame(() => {
        // The target field cannot take focus while its <details> is closed.
        if (advancedFiltersRef.current) advancedFiltersRef.current.open = true;
        focusWithVisibleRing(resourceTypeRef.current);
      });
      return;
    }
    const filters = auditFiltersFromDraft(draft);
    if (!filters) {
      setValidationError("time");
      window.requestAnimationFrame(() => focusWithVisibleRing(fromRef.current));
      return;
    }
    setValidationError(undefined);
    prepareQueryChange({ resetWindow: true });
    setCursorHistory([undefined]);
    setActiveFilters(filters);
  }

  function setPreset(hours: number) {
    setDraft((current) => ({ ...current, ...preset(hours) }));
  }

  function clearOptionalFilters() {
    setDraft((current) => ({
      ...current,
      action: "",
      actor: "",
      resourceType: "",
      resourceId: "",
      outcome: "",
      requestId: "",
    }));
  }

  function invalidateExportEvidence() {
    exportRevision.current += 1;
    setExportEvidence(undefined);
    setExportFailure(undefined);
    setExportGrantReconciliation(undefined);
    setExportReconciliationError(undefined);
    setReconcilingExportGrant(false);
  }

  function updateExportArchiveId(value: string) {
    invalidateExportEvidence();
    setExportArchiveId(value);
  }

  function updateExportApprovalId(value: string) {
    invalidateExportEvidence();
    setExportApprovalId(value);
  }

  async function authorizeExport(command: AuditExportCommand) {
    if (exportAuthorizationInFlight.current) return;
    exportAuthorizationInFlight.current = true;
    setAuthorizingExport(true);
    setExportFailure(undefined);
    setExportGrantReconciliation(undefined);
    setExportReconciliationError(undefined);
    setReconcilingExportGrant(false);
    try {
      const grant = await client.authorizeAuditExport(
        command.archiveId,
        command.approvalRequestId,
      );
      exportRevision.current += 1;
      setExportEvidence({ command, grant });
      setUncertainExport(undefined);
      setExportClock(Date.now());
    } catch (cause) {
      setExportFailure({ kind: "authorize", cause, command });
      if (isUnknownWriteFailure(cause)) setUncertainExport(command);
    } finally {
      exportAuthorizationInFlight.current = false;
      setAuthorizingExport(false);
    }
  }

  function submitExport(event: FormEvent) {
    event.preventDefault();
    if (uncertainExport) return;
    const command = {
      archiveId: exportArchiveId.trim(),
      approvalRequestId: exportApprovalId.trim(),
    };
    if (!command.archiveId || !command.approvalRequestId) return;
    void authorizeExport(command);
  }

  /**
   * Retrieves the independently verified artifact for the operator. Every
   * retrieval is recorded server-side, so only one request may be in flight.
   */
  async function downloadArtifact(artifact: AuditExportArtifact) {
    const grant = currentExportGrant;
    if (!grant || exportExpired || downloadingArtifact !== undefined) return;
    const revision = exportRevision.current;
    setDownloadingArtifact(artifact);
    setExportFailure(undefined);
    setExportGrantReconciliation(undefined);
    setExportReconciliationError(undefined);
    try {
      const { blob, fileName } = await client.downloadAuditExportArtifact(
        grant.audit_export_grant_id,
        artifact,
      );
      if (
        revision !== exportRevision.current
        || Date.now() >= Date.parse(grant.expires_at)
      ) return;
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = fileName;
      link.click();
      URL.revokeObjectURL(url);
    } catch (cause) {
      if (revision === exportRevision.current) {
        setExportFailure({
          kind: "download",
          cause,
          grantId: grant.audit_export_grant_id,
        });
      }
    } finally {
      setDownloadingArtifact(undefined);
    }
  }

  /**
   * A download request writes an immutable ledger row before bytes return.
   * Its outcome therefore must never be retried from the browser. Once its
   * grant ID is known, the only safe follow-up is to read that exact grant.
   * The read does not infer delivery, and deliberately does not unlock either
   * artifact button.
   */
  async function reconcileDownloadGrant() {
    const failure = exportFailure;
    const grant = currentExportGrant;
    if (
      reconcilingExportGrant
      || failure?.kind !== "download"
      || !grant
      || grant.audit_export_grant_id !== failure.grantId
    ) return;

    const revision = exportRevision.current;
    setReconcilingExportGrant(true);
    setExportReconciliationError(undefined);
    try {
      const reconciled = await client.getAuditExportGrant(failure.grantId);
      if (revision !== exportRevision.current) return;
      if (
        reconciled.archive_id !== grant.archive_id
        || reconciled.approval_request_id !== grant.approval_request_id
        || reconciled.requested_by !== grant.requested_by
      ) {
        throw new Error("audit_export_grant_reconciliation_mismatch");
      }
      setExportGrantReconciliation(reconciled);
    } catch (cause) {
      if (revision === exportRevision.current) setExportReconciliationError(cause);
    } finally {
      if (revision === exportRevision.current) setReconcilingExportGrant(false);
    }
  }

  /**
   * The authorization POST may have committed despite the browser losing its
   * response. Reconcile only the immutable archive/approval relationship;
   * never replay the authorization request.
   */
  async function reconcileAuthorizationGrant() {
    const command = uncertainExport;
    if (!command || reconcilingExportGrant) return;

    const revision = exportRevision.current;
    setReconcilingExportGrant(true);
    setExportReconciliationError(undefined);
    try {
      const reconciled = await client.getAuditExportGrantByApproval(
        command.archiveId,
        command.approvalRequestId,
      );
      if (revision !== exportRevision.current) return;
      if (
        reconciled.archive_id !== command.archiveId
        || reconciled.approval_request_id !== command.approvalRequestId
      ) {
        throw new Error("audit_export_authorization_reconciliation_mismatch");
      }
      setExportEvidence({ command, grant: reconciled });
      setUncertainExport(undefined);
      setExportFailure(undefined);
      setExportGrantReconciliation(undefined);
      setExportClock(Date.now());
    } catch (cause) {
      if (revision === exportRevision.current) setExportReconciliationError(cause);
    } finally {
      if (revision === exportRevision.current) setReconcilingExportGrant(false);
    }
  }

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1>{t("auditTitle")}</h1>
          <p>{t("auditHint")}</p>
        </div>
      </div>
      {page ? (
        <ContextBar
          className="audit-query-context"
          facts={[
            {
              id: "query-id",
              label: t("queryId"),
              value: <code>{page.query_id}</code>,
            },
            {
              id: "occurred-window",
              label: t("timeRange"),
              value: <>{formatDate(page.occurred_from, locale)} – {formatDate(page.occurred_to, locale)}</>,
            },
            {
              id: "applied-filters",
              label: t("auditAppliedFilters"),
              value: queryFilterFacts.length ? (
                <span className="audit-query-filter-summary">
                  {queryFilterFacts.map((fact) => (
                    <span className="audit-query-filter" key={fact.id}>
                      <span>{fact.label}</span>
                      <code>{fact.value}</code>
                    </span>
                  ))}
                </span>
              ) : t("auditNoAdditionalFilters"),
            },
            {
              id: "result-scope",
              label: t("auditResultScope"),
              value: <>
                <span className="mono">{page.items.length}</span> · {page.has_more
                  ? t("auditMoreResultsAvailable")
                  : t("auditLastResultPage")}
              </>,
            },
          ]}
          title={t("auditQueryContext")}
        />
      ) : null}
      <Panel className="audit-filters">
        <form aria-label={t("auditQueryConditions")} onSubmit={runSearch}>
          <div className="audit-filter-heading">
            <div>
              <strong>{t("auditQueryConditions")}</strong>
              <p>{t("auditQueryConditionsHint")}</p>
            </div>
            <div className="preset-buttons" aria-label={t("timeRange")}>
              <Button type="button" variant="quiet" onClick={() => setPreset(1)}>{t("lastHour")}</Button>
              <Button type="button" variant="quiet" onClick={() => setPreset(24)}>{t("lastDay")}</Button>
              <Button type="button" variant="quiet" onClick={() => setPreset(24 * 7)}>{t("lastWeek")}</Button>
              <Button type="button" variant="quiet" onClick={() => setPreset(24 * 30)}>{t("lastMonth")}</Button>
            </div>
          </div>
          <div className="audit-time-grid">
            <Field label={t("from")}><Input ref={fromRef} type="datetime-local" value={draft.from} aria-invalid={validationError === "time" || undefined} aria-describedby={validationError === "time" ? validationMessageId : undefined} onChange={(event) => update("from", event.target.value)} required /></Field>
            <Field label={t("to")}><Input type="datetime-local" value={draft.to} aria-invalid={validationError === "time" || undefined} aria-describedby={validationError === "time" ? validationMessageId : undefined} onChange={(event) => update("to", event.target.value)} required /></Field>
            <Field label={t("outcome")}><Select value={draft.outcome} onChange={(event) => update("outcome", event.target.value as AuditOutcome | "")}>
              <option value="">{t("all")}</option><option value="success">{t("success")}</option><option value="failure">{t("failure")}</option><option value="denied">{t("denied")}</option>
            </Select></Field>
          </div>
          <details className="advanced-filters" ref={advancedFiltersRef}>
            <summary>{t("advancedFilters")}</summary>
            <div className="advanced-filter-grid">
              <Field label={t("action")} hint={t("auditActionFilterHint")}><Input value={draft.action} onChange={(event) => update("action", event.target.value)} maxLength={100} pattern="[a-z][a-z0-9_.]+" /></Field>
              <Field label={t("actor")} hint={t("auditActorFilterHint")}><Input value={draft.actor} onChange={(event) => update("actor", event.target.value)} /></Field>
              <Field label={t("resourceType")} hint={t("auditResourceTypeFilterHint")}><Input ref={resourceTypeRef} value={draft.resourceType} aria-invalid={validationError === "resource" || undefined} aria-describedby={validationError === "resource" ? validationMessageId : undefined} onChange={(event) => update("resourceType", event.target.value)} maxLength={100} pattern="[a-z][a-z0-9_.]+" /></Field>
              <Field label={t("resourceId")}><Input value={draft.resourceId} onChange={(event) => update("resourceId", event.target.value)} maxLength={512} /></Field>
              <Field label={t("requestFilter")}><Input value={draft.requestId} onChange={(event) => update("requestId", event.target.value)} maxLength={200} /></Field>
            </div>
          </details>
          {validationMessage ? <div className="notice notice--error" id={validationMessageId} role="alert"><p>{validationMessage}</p></div> : null}
          <div className="audit-filter-actions">
            <Button type="button" variant="quiet" onClick={clearOptionalFilters}>{t("clear")}</Button>
            <Button type="submit">{t("search")}</Button>
          </div>
        </form>
      </Panel>

      {error ? <AdminErrorNotice error={error} retry={() => void load()} t={t} /> : null}
      {loading ? <Spinner label={t("loading")} /> : null}
      {page?.items.length ? (
        <AdminListDetail
          backLabel={t("backToAuditEvents")}
          className="audit-results"
          detailBody={selected ? (
            <AuditEventDetail event={selected} locale={locale} t={t} />
          ) : null}
          detailClassName="audit-detail"
          detailEmpty={<EmptyState title={t("details")} description={t("selectRow")} />}
          detailHeader={<p className="eyebrow">{t("eventId")}</p>}
          detailTitle={selected ? shortId(selected.event_id) : t("details")}
          detailTrailing={selected ? (
            <OutcomeBadge outcome={selected.outcome} t={t} />
          ) : null}
          listLabel={t("auditResultsQueue")}
          renderList={(registerTrigger) => (
            <>
              <div className="audit-results-queue__heading">
                <div>
                  <p className="eyebrow">{t("auditResultsQueue")}</p>
                  <h2>{t("auditResultsQueue")}</h2>
                  <p>{t("auditResultsQueueHint")}</p>
                </div>
              </div>
              <div className="table-wrap">
                <table>
                  <caption className="sr-only">{t("auditResultsTableCaption")}</caption>
                  <thead><tr><th>{t("action")}</th><th>{t("resourceType")}</th><th>{t("actor")}</th><th>{t("outcome")}</th><th>{t("occurredAt")}</th></tr></thead>
                  <tbody>{page.items.map((item) => (
                    <tr key={item.event_id} className={selected?.event_id === item.event_id ? "is-selected" : ""} onClick={() => setSelected(item)}>
                      <td>
                        <button
                          ref={registerTrigger(item.event_id)}
                          aria-current={selected?.event_id === item.event_id ? "true" : undefined}
                          className="row-button mono"
                          onClick={(event) => {
                            event.stopPropagation();
                            setSelected(item);
                          }}
                        >
                          {item.action}
                        </button>
                      </td>
                      <td><span className="mono">{item.resource_type}</span><small className="table-subline mono">{shortId(item.resource_id)}</small></td>
                      <td>
                        <span className="mono">{shortId(item.actor_principal_id)}</span>
                        <small className="table-subline"><ActorIdentity kind={item.actor_kind} t={t} /></small>
                      </td>
                      <td><OutcomeBadge outcome={item.outcome} t={t} /></td>
                      <td>{formatDate(item.occurred_at, locale)}</td>
                    </tr>
                  ))}</tbody>
                </table>
              </div>
              {(cursorHistory.length > 1 || page.has_more) ? <nav className="pagination" aria-label={t("pagination")}>
                <Button variant="secondary" disabled={cursorHistory.length <= 1} onClick={() => { prepareQueryChange(); setCursorHistory((items) => items.slice(0, -1)); }}>{t("previous")}</Button>
                <Button variant="secondary" disabled={!page.has_more || !page.next_cursor} onClick={() => { if (page.next_cursor) { prepareQueryChange(); setCursorHistory((items) => [...items, page.next_cursor ?? undefined]); } }}>{t("next")}</Button>
              </nav> : null}
            </>
          )}
          selectionKey={selected?.event_id ?? null}
          onBack={() => setSelected(undefined)}
        />
      ) : page ? (
        <EmptyState title={t("noAuditEvents")} description={t("noAuditEventsHint")} />
      ) : null}

      <Panel className="audit-export-panel">
        <details>
          <summary>
            <strong>{t("exportAuthorization")}</strong>
            <span className="audit-export-summary-hint">
              <span className="audit-export-risk">{t("exportHighRiskGate")} · </span>
              {t("exportAuthorizationHint")}
            </span>
          </summary>
          <form onSubmit={submitExport}>
            <p className="audit-export-boundary">{t("exportSecurityBoundary")}</p>
            <div className="audit-export-grid">
              <Field label={t("archiveId")} hint={t("auditArchiveIdHint")}>
                <Input
                  disabled={authorizingExport || uncertainExport !== undefined}
                  value={exportArchiveId}
                  onChange={(event) => updateExportArchiveId(event.target.value)}
                  required
                />
              </Field>
              <Field label={t("approvalId")} hint={t("approvalRequestIdHint")}>
                <Input
                  disabled={authorizingExport || uncertainExport !== undefined}
                  value={exportApprovalId}
                  onChange={(event) => updateExportApprovalId(event.target.value)}
                  required
                />
              </Field>
              <Button type="submit" disabled={authorizingExport || uncertainExport !== undefined}>
                {authorizingExport ? t("loading") : t("authorizeExport")}
              </Button>
            </div>
          </form>
          {exportFailure ? <AdminErrorNotice error={exportFailure.cause} t={t} /> : null}
          {uncertainExport ? (
            <UnknownWriteLock
              aria-label={t("exportAuthorizationOutcomeUnknown")}
              description={t("exportAuthorizationOutcomeUnknown")}
              evidence={
                <>
                  <p><span>{t("archiveId")}:</span> <code>{uncertainExport.archiveId}</code></p>
                  <p><span>{t("approvalId")}:</span> <code>{uncertainExport.approvalRequestId}</code></p>
                </>
              }
              reconciliationAction={
                <Button
                  type="button"
                  variant="secondary"
                  disabled={reconcilingExportGrant}
                  onClick={() => void reconcileAuthorizationGrant()}
                >
                  {reconcilingExportGrant ? t("loading") : t("reconcileApprovalState")}
                </Button>
              }
              title={t("operationFailed")}
            />
          ) : null}
          {exportFailure?.kind === "download" && currentExportGrant ? (
            <UnknownWriteLock
              aria-label={t("exportDownloadOutcomeUnknown")}
              description={t("exportDownloadOutcomeUnknown")}
              evidence={
                <>
                  <p><span>{t("grantId")}:</span> <code>{currentExportGrant.audit_export_grant_id}</code></p>
                  <p><span>{t("archiveId")}:</span> <code>{currentExportGrant.archive_id}</code></p>
                  <p><span>{t("approvalId")}:</span> <code>{currentExportGrant.approval_request_id}</code></p>
                </>
              }
              reconciliationAction={
                <Button
                  type="button"
                  variant="secondary"
                  disabled={reconcilingExportGrant}
                  onClick={() => void reconcileDownloadGrant()}
                >
                  {reconcilingExportGrant ? t("loading") : t("reconcileApprovalState")}
                </Button>
              }
              title={t("operationFailed")}
            />
          ) : null}
          {exportReconciliationError ? (
            <AdminErrorNotice error={exportReconciliationError} t={t} />
          ) : null}
          {exportGrantReconciliation ? (
            <div className="notice" role="status" aria-label={t("approvalReconciliationReadOnly")}>
              <div>
                <strong>{t("approvalReconciliationReadOnly")}</strong>
                <p><span>{t("grantId")}:</span> <code>{exportGrantReconciliation.audit_export_grant_id}</code></p>
                <p><span>{t("expiresAt")}:</span> {formatDate(exportGrantReconciliation.expires_at, locale)}</p>
              </div>
            </div>
          ) : null}
          {currentExportGrant ? (
            <div
              className="notice audit-export-result"
              role="status"
              aria-label={exportExpired
                ? t("exportAuthorizationExpired")
                : t("exportAuthorized")}
            >
              <div>
                <strong>{exportExpired ? t("exportAuthorizationExpired") : t("exportAuthorized")}</strong>
                <p><span>{t("archiveId")}:</span> <code>{currentExportGrant.archive_id}</code></p>
                <p><span>{t("approvalId")}:</span> <code>{currentExportGrant.approval_request_id}</code></p>
                <p><span>{t("grantId")}:</span> <code>{currentExportGrant.audit_export_grant_id}</code></p>
                <p><span>{t("expiresAt")}:</span> {formatDate(currentExportGrant.expires_at, locale)}</p>
                <small>{t("exportDownloadHint")}</small>
                <div className="audit-export-downloads">
                  <Button
                    type="button"
                    disabled={exportExpired || downloadingArtifact !== undefined || exportFailure?.kind === "download"}
                    onClick={() => void downloadArtifact("events")}
                  >
                    {downloadingArtifact === "events" ? t("loading") : t("downloadEvents")}
                  </Button>
                  <Button
                    type="button"
                    variant="quiet"
                    disabled={exportExpired || downloadingArtifact !== undefined || exportFailure?.kind === "download"}
                    onClick={() => void downloadArtifact("manifest")}
                  >
                    {downloadingArtifact === "manifest" ? t("loading") : t("downloadManifest")}
                  </Button>
                </div>
              </div>
            </div>
          ) : null}
        </details>
      </Panel>
    </>
  );
}

function AuditEventDetail({ event, locale, t }: { event: AuditEvent; locale: Locale; t: Translate }) {
  return (
    <div className="audit-evidence-detail">
      <section aria-labelledby={`audit-event-facts-${event.event_id}`} className="audit-event-facts">
        <div className="audit-detail-section-heading">
          <h3 id={`audit-event-facts-${event.event_id}`}>{t("auditEventFacts")}</h3>
          <p>{t("auditEventFactsHint")}</p>
        </div>
        <dl>
          <dt>{t("eventId")}</dt><dd className="mono">{event.event_id}</dd>
          <dt>{t("action")}</dt><dd className="mono">{event.action}</dd>
          <dt>{t("resourceType")}</dt><dd className="mono">{event.resource_type}</dd>
          <dt>{t("resourceId")}</dt><dd className="mono">{event.resource_id}</dd>
          <dt>{t("actor")}</dt>
          <dd>
            <span className="mono">{event.actor_principal_id}</span>{" "}
            <ActorIdentity kind={event.actor_kind} t={t} />
          </dd>
          <OptionalEvidence label={t("subject")} value={event.subject_principal_id} />
          <OptionalEvidence label={t("onBehalfOf")} value={event.on_behalf_of_principal_id} />
          <dt>{t("outcome")}</dt><dd><OutcomeBadge outcome={event.outcome} t={t} /></dd>
          <dt>{t("occurredAt")}</dt><dd>{formatDate(event.occurred_at, locale)}</dd>
          <dt>{t("receivedAt")}</dt><dd>{formatDate(event.received_at, locale)}</dd>
        </dl>
      </section>
      <EvidenceInspector className="evidence-section" summary={t("hashEvidence")}>
        <dl>
          <dt>{t("chainSequence")}</dt><dd>{event.chain_sequence}</dd>
          <dt>{t("hashVersion")}</dt><dd>{event.hash_version}</dd>
          <OptionalEvidence label={t("previousHash")} value={event.previous_hash} />
          <OptionalEvidence label={t("eventHash")} value={event.event_hash} />
          <dt>{t("signatureBatch")}</dt><dd className="mono">{event.signature_batch_id ?? t("notSignedYet")}</dd>
        </dl>
      </EvidenceInspector>
      <EvidenceInspector className="evidence-section" summary={t("auditMinimalContext")}>
        <dl>
          <OptionalEvidence label={t("requestId")} value={event.request_id} />
          <OptionalEvidence label={t("traceId")} value={event.trace_id} />
          <OptionalEvidence label={t("sourceIp")} value={event.source_ip} />
          <OptionalEvidence label={t("clientId")} value={event.client_id} />
          <OptionalEvidence label={t("credentialId")} value={event.credential_id} />
          <OptionalEvidence label={t("policy")} value={event.policy_version_id ?? event.policy_id} />
          <OptionalEvidence label={t("approvalContext")} value={event.approval_request_id} />
          <OptionalEvidence label={t("ticket")} value={event.ticket_reference} />
          <OptionalEvidence label={t("environment")} value={event.environment} />
          <OptionalEvidence label={t("region")} value={event.region} />
        </dl>
      </EvidenceInspector>
      <EvidenceInspector className="evidence-section" summary={t("sanitizedDetails")}>
        <pre>{JSON.stringify(event.details, null, 2)}</pre>
      </EvidenceInspector>
    </div>
  );
}
