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
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { ApiError, ControlPlaneClient } from "./api";
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

function ErrorNotice({ error, retry, t }: { error: unknown; retry: () => void; t: Translate }) {
  const body = error instanceof ApiError ? error.body : undefined;
  const message = body ? `${body.code}: ${body.message}` : error instanceof Error ? error.message : "unexpected_error";
  return (
    <div className="notice notice--error" role="alert">
      <div><strong>{t("loadFailed")}</strong><p>{message}</p>{body?.request_id ? <small>{t("requestId")}: {body.request_id}</small> : null}</div>
      <Button variant="secondary" onClick={retry}>{t("retry")}</Button>
    </div>
  );
}

function OutcomeBadge({ outcome, t }: { outcome: AuditOutcome; t: Translate }) {
  const tone = outcome === "success" ? "positive" : outcome === "failure" ? "critical" : "warning";
  return <Badge tone={tone}>{t(outcome)}</Badge>;
}

function OptionalEvidence({ label, value }: { label: string; value: string | null }) {
  if (!value) return null;
  return <><dt>{label}</dt><dd className="mono">{value}</dd></>;
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
  const [validationError, setValidationError] = useState<string>();
  const [exportArchiveId, setExportArchiveId] = useState("");
  const [exportApprovalId, setExportApprovalId] = useState("");
  const [exportGrant, setExportGrant] = useState<AuditExportGrant>();
  const [exportError, setExportError] = useState<unknown>();
  const [authorizingExport, setAuthorizingExport] = useState(false);
  const [downloadingArtifact, setDownloadingArtifact] = useState<AuditExportArtifact>();
  const cursor = cursorHistory.at(-1);

  async function load() {
    setLoading(true);
    setError(undefined);
    try {
      const result = await client.searchAuditEvents({
        ...activeFilters,
        occurred_from: lockedWindow?.from ?? activeFilters.occurred_from,
        occurred_to: lockedWindow?.to ?? activeFilters.occurred_to,
      }, { cursor, limit: 50 });
      setPage(result);
      setLockedWindow({ from: result.occurred_from, to: result.occurred_to });
    } catch (cause) {
      setError(cause);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void load(); }, [activeFilters, cursor]);

  function update<Key extends keyof AuditDraftFilters>(key: Key, value: AuditDraftFilters[Key]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  function runSearch(event: FormEvent) {
    event.preventDefault();
    if (draft.resourceId.trim() && !draft.resourceType.trim()) {
      setValidationError(t("resourceRequired"));
      return;
    }
    const filters = auditFiltersFromDraft(draft);
    if (!filters) {
      setValidationError(t("invalidTimeRange"));
      return;
    }
    setValidationError(undefined);
    setSelected(undefined);
    setPage(undefined);
    setLockedWindow(undefined);
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

  async function authorizeExport(event?: FormEvent) {
    event?.preventDefault();
    setAuthorizingExport(true);
    setExportError(undefined);
    try {
      const grant = await client.authorizeAuditExport(
        exportArchiveId.trim(),
        exportApprovalId.trim(),
      );
      setExportGrant(grant);
    } catch (cause) {
      setExportError(cause);
    } finally {
      setAuthorizingExport(false);
    }
  }

  /**
   * Streams the verified artifact to the operator. Every retrieval is recorded
   * server-side, so the button is disabled while one is in flight.
   */
  async function downloadArtifact(artifact: AuditExportArtifact) {
    if (!exportGrant) {
      return;
    }
    setDownloadingArtifact(artifact);
    setExportError(undefined);
    try {
      const { blob, fileName } = await client.downloadAuditExportArtifact(
        exportGrant.audit_export_grant_id,
        artifact,
      );
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = fileName;
      link.click();
      URL.revokeObjectURL(url);
    } catch (cause) {
      setExportError(cause);
    } finally {
      setDownloadingArtifact(undefined);
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
      <Panel className="audit-export-panel">
        <details>
          <summary>
            <strong>{t("exportAuthorization")}</strong>
            <span>{t("exportAuthorizationHint")}</span>
          </summary>
          <form onSubmit={(event) => void authorizeExport(event)}>
            <div className="audit-export-grid">
              <Field label={t("archiveId")}>
                <Input
                  value={exportArchiveId}
                  onChange={(event) => setExportArchiveId(event.target.value)}
                  placeholder="UUID"
                  required
                />
              </Field>
              <Field label={t("approvalId")}>
                <Input
                  value={exportApprovalId}
                  onChange={(event) => setExportApprovalId(event.target.value)}
                  placeholder="UUID"
                  required
                />
              </Field>
              <Button type="submit" disabled={authorizingExport}>
                {authorizingExport ? t("loading") : t("authorizeExport")}
              </Button>
            </div>
          </form>
          {exportError ? (
            <ErrorNotice
              error={exportError}
              retry={() => void authorizeExport()}
              t={t}
            />
          ) : null}
          {exportGrant ? (
            <div className="notice audit-export-result" role="status">
              <div>
                <strong>{t("exportAuthorized")}</strong>
                <p><span>{t("grantId")}:</span> <code>{exportGrant.audit_export_grant_id}</code></p>
                <p><span>{t("expiresAt")}:</span> {formatDate(exportGrant.expires_at, locale)}</p>
                <small>{t("exportDownloadHint")}</small>
                <div className="audit-export-downloads">
                  <Button
                    type="button"
                    disabled={downloadingArtifact !== undefined}
                    onClick={() => void downloadArtifact("events")}
                  >
                    {downloadingArtifact === "events" ? t("loading") : t("downloadEvents")}
                  </Button>
                  <Button
                    type="button"
                    variant="quiet"
                    disabled={downloadingArtifact !== undefined}
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
      <Panel className="audit-filters">
        <form onSubmit={runSearch}>
          <div className="audit-filter-heading">
            <strong>{t("filters")}</strong>
            <div className="preset-buttons" aria-label={t("timeRange")}>
              <Button type="button" variant="quiet" onClick={() => setPreset(1)}>{t("lastHour")}</Button>
              <Button type="button" variant="quiet" onClick={() => setPreset(24)}>{t("lastDay")}</Button>
              <Button type="button" variant="quiet" onClick={() => setPreset(24 * 7)}>{t("lastWeek")}</Button>
              <Button type="button" variant="quiet" onClick={() => setPreset(24 * 30)}>{t("lastMonth")}</Button>
            </div>
          </div>
          <div className="audit-time-grid">
            <Field label={t("from")}><Input type="datetime-local" value={draft.from} onChange={(event) => update("from", event.target.value)} required /></Field>
            <Field label={t("to")}><Input type="datetime-local" value={draft.to} onChange={(event) => update("to", event.target.value)} required /></Field>
            <Field label={t("outcome")}><Select value={draft.outcome} onChange={(event) => update("outcome", event.target.value as AuditOutcome | "")}>
              <option value="">{t("all")}</option><option value="success">{t("success")}</option><option value="failure">{t("failure")}</option><option value="denied">{t("denied")}</option>
            </Select></Field>
          </div>
          <details className="advanced-filters">
            <summary>{t("advancedFilters")}</summary>
            <div className="advanced-filter-grid">
              <Field label={t("action")}><Input value={draft.action} onChange={(event) => update("action", event.target.value)} maxLength={100} pattern="[a-z][a-z0-9_.]+" placeholder="mail_domain.activated" /></Field>
              <Field label={t("actor")}><Input value={draft.actor} onChange={(event) => update("actor", event.target.value)} placeholder="UUID" /></Field>
              <Field label={t("resourceType")}><Input value={draft.resourceType} onChange={(event) => update("resourceType", event.target.value)} maxLength={100} pattern="[a-z][a-z0-9_.]+" placeholder="mail_domain" /></Field>
              <Field label={t("resourceId")}><Input value={draft.resourceId} onChange={(event) => update("resourceId", event.target.value)} maxLength={512} /></Field>
              <Field label={t("requestFilter")}><Input value={draft.requestId} onChange={(event) => update("requestId", event.target.value)} maxLength={200} /></Field>
            </div>
          </details>
          {validationError ? <div className="notice notice--error" role="alert"><p>{validationError}</p></div> : null}
          <div className="audit-filter-actions">
            <Button type="button" variant="quiet" onClick={clearOptionalFilters}>{t("clear")}</Button>
            <Button type="submit">{t("search")}</Button>
          </div>
        </form>
      </Panel>

      {page ? (
        <div className="query-evidence">
          <span><strong>{t("queryId")}:</strong> <code>{page.query_id}</code></span>
          <span><strong>{t("activeWindow")}:</strong> {formatDate(page.occurred_from, locale)} – {formatDate(page.occurred_to, locale)}</span>
        </div>
      ) : null}
      {error ? <ErrorNotice error={error} retry={() => void load()} t={t} /> : null}
      {loading ? <Spinner label={t("loading")} /> : page?.items.length ? (
        <div className="master-detail audit-results">
          <Panel className="list-panel">
            <div className="table-wrap">
              <table>
                <thead><tr><th>{t("action")}</th><th>{t("resourceType")}</th><th>{t("actor")}</th><th>{t("outcome")}</th><th>{t("occurredAt")}</th></tr></thead>
                <tbody>{page.items.map((item) => (
                  <tr key={item.event_id} className={selected?.event_id === item.event_id ? "is-selected" : ""} onClick={() => setSelected(item)}>
                    <td><button className="row-button mono" onClick={() => setSelected(item)}>{item.action}</button></td>
                    <td><span className="mono">{item.resource_type}</span><small className="table-subline mono">{shortId(item.resource_id)}</small></td>
                    <td><span className="mono">{shortId(item.actor_principal_id)}</span><small className="table-subline">{item.actor_kind}</small></td>
                    <td><OutcomeBadge outcome={item.outcome} t={t} /></td>
                    <td>{formatDate(item.occurred_at, locale)}</td>
                  </tr>
                ))}</tbody>
              </table>
            </div>
            {(cursorHistory.length > 1 || page.has_more) ? <nav className="pagination" aria-label="Pagination">
              <Button variant="secondary" disabled={cursorHistory.length <= 1} onClick={() => { setSelected(undefined); setCursorHistory((items) => items.slice(0, -1)); }}>{t("previous")}</Button>
              <Button variant="secondary" disabled={!page.has_more || !page.next_cursor} onClick={() => { if (page.next_cursor) { setSelected(undefined); setCursorHistory((items) => [...items, page.next_cursor ?? undefined]); } }}>{t("next")}</Button>
            </nav> : null}
          </Panel>
          <Panel className="detail-panel audit-detail">
            {selected ? <AuditEventDetail event={selected} locale={locale} t={t} /> : <EmptyState title={t("details")} description={t("selectRow")} />}
          </Panel>
        </div>
      ) : (
        <EmptyState title={t("noAuditEvents")} description={t("noAuditEventsHint")} />
      )}
    </>
  );
}

function AuditEventDetail({ event, locale, t }: { event: AuditEvent; locale: Locale; t: Translate }) {
  return (
    <>
      <div className="detail-panel__heading">
        <div><p className="eyebrow">{t("eventId")}</p><h2 className="mono">{shortId(event.event_id)}</h2></div>
        <OutcomeBadge outcome={event.outcome} t={t} />
      </div>
      <dl>
        <dt>{t("action")}</dt><dd className="mono">{event.action}</dd>
        <dt>{t("resourceType")}</dt><dd className="mono">{event.resource_type}</dd>
        <dt>{t("resourceId")}</dt><dd className="mono">{event.resource_id}</dd>
        <dt>{t("actor")}</dt><dd className="mono">{event.actor_principal_id} · {event.actor_kind}</dd>
        <OptionalEvidence label={t("subject")} value={event.subject_principal_id} />
        <OptionalEvidence label={t("onBehalfOf")} value={event.on_behalf_of_principal_id} />
        <dt>{t("occurredAt")}</dt><dd>{formatDate(event.occurred_at, locale)}</dd>
        <dt>{t("receivedAt")}</dt><dd>{formatDate(event.received_at, locale)}</dd>
      </dl>
      <details className="evidence-section" open>
        <summary>{t("hashEvidence")}</summary>
        <dl>
          <dt>{t("chainSequence")}</dt><dd>{event.chain_sequence}</dd>
          <dt>{t("hashVersion")}</dt><dd>{event.hash_version}</dd>
          <OptionalEvidence label={t("previousHash")} value={event.previous_hash} />
          <OptionalEvidence label={t("eventHash")} value={event.event_hash} />
          <dt>{t("signatureBatch")}</dt><dd className="mono">{event.signature_batch_id ?? t("notSignedYet")}</dd>
        </dl>
      </details>
      <details className="evidence-section">
        <summary>{t("context")}</summary>
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
      </details>
      <details className="evidence-section">
        <summary>{t("sanitizedDetails")}</summary>
        <pre>{JSON.stringify(event.details, null, 2)}</pre>
      </details>
    </>
  );
}
