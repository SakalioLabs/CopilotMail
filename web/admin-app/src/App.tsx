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
import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { ApiError, ControlPlaneClient } from "./api";
import AuditView from "./AuditView";
import ApprovalView from "./ApprovalView";
import RetentionView from "./RetentionView";
import RolesView from "./RolesView";
import { loadSession, logout, startLogin } from "./auth";
import { configurationProblems, readRuntimeConfig } from "./config";
import { translator } from "./i18n";
import type {
  CaseStatus,
  ComplianceCase,
  HoldStatus,
  LegalHold,
  Locale,
  Page,
} from "./types";

type View = "overview" | "cases" | "holds" | "approvals" | "roles" | "audit" | "retention";
type AuthState =
  | { kind: "checking" }
  | { kind: "signed-out" }
  | { kind: "authenticated" }
  | { kind: "error"; message: string };

const config = readRuntimeConfig();

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

function describeError(error: unknown): { message: string; requestId?: string } {
  if (error instanceof ApiError) {
    return { message: `${error.body.code}: ${error.body.message}`, requestId: error.body.request_id };
  }
  return { message: error instanceof Error ? error.message : "unexpected_error" };
}

function Dialog({
  title,
  closeLabel,
  onClose,
  children,
}: {
  title: string;
  closeLabel: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const headingId = useMemo(() => crypto.randomUUID(), []);
  const dialogRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialogRef.current?.querySelector<HTMLElement>("input, button, select, textarea")?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusable = [...dialogRef.current.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex='-1'])",
      )];
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable.at(-1)!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previous?.focus();
    };
  }, [onClose]);
  return (
    <div className="dialog-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby={headingId} ref={dialogRef}>
        <div className="dialog__header">
          <h2 id={headingId}>{title}</h2>
          <Button variant="quiet" onClick={onClose} aria-label={closeLabel}>×</Button>
        </div>
        {children}
      </div>
    </div>
  );
}

function ErrorNotice({
  error,
  retry,
  t,
}: {
  error: unknown;
  retry?: () => void;
  t: ReturnType<typeof translator>;
}) {
  const detail = describeError(error);
  return (
    <div className="notice notice--error" role="alert">
      <div>
        <strong>{t("loadFailed")}</strong>
        <p>{detail.message}</p>
        {detail.requestId ? <small>{t("requestId")}: {detail.requestId}</small> : null}
      </div>
      {retry ? <Button variant="secondary" onClick={retry}>{t("retry")}</Button> : null}
    </div>
  );
}

function StatusBadge({ status, t }: { status: CaseStatus | HoldStatus; t: ReturnType<typeof translator> }) {
  const tone = status === "open" || status === "active" ? "warning" : "neutral";
  return <Badge tone={tone}>{t(status)}</Badge>;
}

function Pagination({
  canGoBack,
  canGoNext,
  onBack,
  onNext,
  t,
}: {
  canGoBack: boolean;
  canGoNext: boolean;
  onBack: () => void;
  onNext: () => void;
  t: ReturnType<typeof translator>;
}) {
  if (!canGoBack && !canGoNext) return null;
  return (
    <nav className="pagination" aria-label="Pagination">
      <Button variant="secondary" disabled={!canGoBack} onClick={onBack}>{t("previous")}</Button>
      <Button variant="secondary" disabled={!canGoNext} onClick={onNext}>{t("next")}</Button>
    </nav>
  );
}

function CaseForm({
  client,
  onDone,
  onClose,
  t,
}: {
  client: ControlPlaneClient;
  onDone: (item: ComplianceCase) => void;
  onClose: () => void;
  t: ReturnType<typeof translator>;
}) {
  const [externalReference, setExternalReference] = useState("");
  const [title, setTitle] = useState("");
  const [purpose, setPurpose] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<unknown>();
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!externalReference.trim() || !title.trim() || !purpose.trim()) {
      setError(new Error(t("fieldRequired")));
      return;
    }
    setSubmitting(true);
    setError(undefined);
    try {
      onDone(await client.createCase({
        external_reference: externalReference.trim(),
        title: title.trim(),
        purpose: purpose.trim(),
      }));
    } catch (cause) {
      setError(cause);
      setSubmitting(false);
    }
  }
  return (
    <form onSubmit={submit}>
      <div className="dialog__body form-grid">
        {error ? <ErrorNotice error={error} t={t} /> : null}
        <Field label={t("externalReference")}><Input value={externalReference} onChange={(event) => setExternalReference(event.target.value)} required maxLength={160} /></Field>
        <Field label={t("title")}><Input value={title} onChange={(event) => setTitle(event.target.value)} required maxLength={200} /></Field>
        <Field label={t("purpose")}><textarea className="control" value={purpose} onChange={(event) => setPurpose(event.target.value)} required maxLength={2000} rows={4} /></Field>
      </div>
      <div className="dialog__actions">
        <Button type="button" variant="secondary" onClick={onClose}>{t("cancel")}</Button>
        <Button type="submit" disabled={submitting}>{t("create")}</Button>
      </div>
    </form>
  );
}

function HoldForm({
  client,
  initialCaseId,
  onDone,
  onClose,
  t,
}: {
  client: ControlPlaneClient;
  initialCaseId?: string;
  onDone: (item: LegalHold) => void;
  onClose: () => void;
  t: ReturnType<typeof translator>;
}) {
  const [archiveId, setArchiveId] = useState("");
  const [caseId, setCaseId] = useState(initialCaseId ?? "");
  const [approvalId, setApprovalId] = useState("");
  const [reason, setReason] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<unknown>();
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (![archiveId, caseId, approvalId, reason].every((value) => value.trim())) {
      setError(new Error(t("fieldRequired")));
      return;
    }
    setSubmitting(true);
    setError(undefined);
    try {
      onDone(await client.applyHold(archiveId.trim(), {
        case_id: caseId.trim(),
        approval_request_id: approvalId.trim(),
        reason: reason.trim(),
      }));
    } catch (cause) {
      setError(cause);
      setSubmitting(false);
    }
  }
  return (
    <form onSubmit={submit}>
      <div className="dialog__body form-grid">
        {error ? <ErrorNotice error={error} t={t} /> : null}
        <div className="notice"><p>{t("approvalHint")}</p></div>
        <Field label={t("archiveId")}><Input value={archiveId} onChange={(event) => setArchiveId(event.target.value)} required /></Field>
        <Field label={t("caseId")}><Input value={caseId} onChange={(event) => setCaseId(event.target.value)} required /></Field>
        <Field label={t("approvalId")}><Input value={approvalId} onChange={(event) => setApprovalId(event.target.value)} required /></Field>
        <Field label={t("reason")}><textarea className="control" value={reason} onChange={(event) => setReason(event.target.value)} required maxLength={2000} rows={3} /></Field>
      </div>
      <div className="dialog__actions">
        <Button type="button" variant="secondary" onClick={onClose}>{t("cancel")}</Button>
        <Button type="submit" disabled={submitting}>{t("applyHold")}</Button>
      </div>
    </form>
  );
}

function ConfirmAction({
  kind,
  client,
  item,
  onDone,
  onClose,
  t,
}: {
  kind: "close-case" | "release-hold";
  client: ControlPlaneClient;
  item: ComplianceCase | LegalHold;
  onDone: () => void;
  onClose: () => void;
  t: ReturnType<typeof translator>;
}) {
  const [approvalId, setApprovalId] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<unknown>();
  async function submit(event: FormEvent) {
    event.preventDefault();
    setSubmitting(true);
    setError(undefined);
    try {
      if (kind === "close-case") {
        await client.closeCase((item as ComplianceCase).case_id);
      } else {
        if (!approvalId.trim()) throw new Error(t("fieldRequired"));
        await client.releaseHold((item as LegalHold).legal_hold_id, approvalId.trim());
      }
      onDone();
    } catch (cause) {
      setError(cause);
      setSubmitting(false);
    }
  }
  return (
    <form onSubmit={submit}>
      <div className="dialog__body form-grid">
        {error ? <ErrorNotice error={error} t={t} /> : null}
        <div className="notice"><p>{kind === "close-case" ? t("closeBlockedHint") : t("approvalHint")}</p></div>
        {kind === "release-hold" ? (
          <Field label={t("approvalId")}><Input value={approvalId} onChange={(event) => setApprovalId(event.target.value)} required /></Field>
        ) : null}
      </div>
      <div className="dialog__actions">
        <Button type="button" variant="secondary" onClick={onClose}>{t("cancel")}</Button>
        <Button type="submit" variant={kind === "release-hold" ? "danger" : "primary"} disabled={submitting}>
          {kind === "close-case" ? t("confirmClose") : t("confirmRelease")}
        </Button>
      </div>
    </form>
  );
}

function CasesView({
  client,
  locale,
  t,
  notify,
  openHolds,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: ReturnType<typeof translator>;
  notify: (message: string) => void;
  openHolds: (caseId: string) => void;
}) {
  const [status, setStatus] = useState<CaseStatus | "">("open");
  const [page, setPage] = useState<Page<ComplianceCase>>();
  const [cursorHistory, setCursorHistory] = useState<(string | undefined)[]>([undefined]);
  const [error, setError] = useState<unknown>();
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<ComplianceCase>();
  const [dialog, setDialog] = useState<"create" | "close">();
  const cursor = cursorHistory.at(-1);
  const load = async () => {
    setLoading(true);
    setError(undefined);
    try {
      setPage(await client.listCases({ status: status || undefined, cursor, limit: 25 }));
    } catch (cause) {
      setError(cause);
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => { void load(); }, [status, cursor]);
  function changeStatus(value: string) {
    setStatus(value as CaseStatus | "");
    setCursorHistory([undefined]);
    setSelected(undefined);
  }
  return (
    <>
      <div className="page-heading">
        <div><p className="eyebrow">{t("adminCenter")}</p><h1>{t("cases")}</h1></div>
        <Button onClick={() => setDialog("create")}>＋ {t("newCase")}</Button>
      </div>
      <div className="toolbar">
        <Field label={t("status")}>
          <Select value={status} onChange={(event) => changeStatus(event.target.value)}>
            <option value="">{t("all")}</option><option value="open">{t("open")}</option><option value="closed">{t("closed")}</option>
          </Select>
        </Field>
        <Button variant="quiet" onClick={() => void load()}>{t("refresh")}</Button>
      </div>
      {error ? <ErrorNotice error={error} retry={() => void load()} t={t} /> : null}
      {loading ? <Spinner label={t("loading")} /> : page?.items.length ? (
        <div className="master-detail">
          <Panel className="list-panel">
            <div className="table-wrap">
              <table>
                <thead><tr><th>{t("title")}</th><th>{t("externalReference")}</th><th>{t("status")}</th><th>{t("createdAt")}</th></tr></thead>
                <tbody>
                  {page.items.map((item) => (
                    <tr key={item.case_id} className={selected?.case_id === item.case_id ? "is-selected" : ""} onClick={() => setSelected(item)}>
                      <td><button className="row-button" onClick={() => setSelected(item)}>{item.title}</button></td>
                      <td>{item.external_reference}</td><td><StatusBadge status={item.status} t={t} /></td><td>{formatDate(item.created_at, locale)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <Pagination canGoBack={cursorHistory.length > 1} canGoNext={Boolean(page.has_more && page.next_cursor)}
              onBack={() => { setSelected(undefined); setCursorHistory((items) => items.slice(0, -1)); }}
              onNext={() => { if (page.next_cursor) { setSelected(undefined); setCursorHistory((items) => [...items, page.next_cursor ?? undefined]); } }} t={t} />
          </Panel>
          <Panel className="detail-panel" aria-live="polite">
            {selected ? (
              <>
                <div className="detail-panel__heading"><div><p className="eyebrow">{t("details")}</p><h2>{selected.title}</h2></div><StatusBadge status={selected.status} t={t} /></div>
                <dl><dt>{t("caseId")}</dt><dd className="mono">{selected.case_id}</dd><dt>{t("purpose")}</dt><dd>{selected.purpose}</dd><dt>{t("createdBy")}</dt><dd className="mono">{shortId(selected.created_by)}</dd><dt>{t("createdAt")}</dt><dd>{formatDate(selected.created_at, locale)}</dd>{selected.closed_at ? <><dt>{t("closedAt")}</dt><dd>{formatDate(selected.closed_at, locale)}</dd></> : null}</dl>
                <div className="detail-actions"><Button variant="secondary" onClick={() => openHolds(selected.case_id)}>{t("viewHolds")}</Button>{selected.status === "open" ? <Button onClick={() => setDialog("close")}>{t("closeCase")}</Button> : null}</div>
              </>
            ) : <EmptyState title={t("details")} description={t("selectRow")} />}
          </Panel>
        </div>
      ) : <EmptyState title={t("noCases")} description={t("noCasesHint")} action={<Button onClick={() => setDialog("create")}>{t("newCase")}</Button>} />}
      {dialog === "create" ? <Dialog title={t("newCase")} closeLabel={t("closeDialog")} onClose={() => setDialog(undefined)}><CaseForm client={client} t={t} onClose={() => setDialog(undefined)} onDone={() => { setDialog(undefined); notify(t("caseCreated")); setCursorHistory([undefined]); void load(); }} /></Dialog> : null}
      {dialog === "close" && selected ? <Dialog title={t("closeCase")} closeLabel={t("closeDialog")} onClose={() => setDialog(undefined)}><ConfirmAction kind="close-case" client={client} item={selected} t={t} onClose={() => setDialog(undefined)} onDone={() => { setDialog(undefined); setSelected(undefined); notify(t("caseClosed")); void load(); }} /></Dialog> : null}
    </>
  );
}

function HoldsView({
  client,
  locale,
  t,
  notify,
  initialCaseId,
  clearInitialCase,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: ReturnType<typeof translator>;
  notify: (message: string) => void;
  initialCaseId?: string;
  clearInitialCase: () => void;
}) {
  const [status, setStatus] = useState<HoldStatus | "">("active");
  const [caseId, setCaseId] = useState(initialCaseId ?? "");
  const [page, setPage] = useState<Page<LegalHold>>();
  const [cursorHistory, setCursorHistory] = useState<(string | undefined)[]>([undefined]);
  const [error, setError] = useState<unknown>();
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<LegalHold>();
  const [dialog, setDialog] = useState<"apply" | "release">();
  const cursor = cursorHistory.at(-1);
  const load = async () => {
    setLoading(true); setError(undefined);
    try { setPage(await client.listHolds({ status: status || undefined, case_id: caseId || undefined, cursor, limit: 25 })); }
    catch (cause) { setError(cause); }
    finally { setLoading(false); }
  };
  useEffect(() => { void load(); }, [status, caseId, cursor]);
  function resetPaging() { setCursorHistory([undefined]); setSelected(undefined); }
  return (
    <>
      <div className="page-heading"><div><p className="eyebrow">{t("adminCenter")}</p><h1>{t("holds")}</h1></div><Button onClick={() => setDialog("apply")}>＋ {t("applyHold")}</Button></div>
      <div className="toolbar">
        <Field label={t("status")}><Select value={status} onChange={(event) => { setStatus(event.target.value as HoldStatus | ""); resetPaging(); }}><option value="">{t("all")}</option><option value="active">{t("active")}</option><option value="released">{t("released")}</option></Select></Field>
        {caseId ? <div className="filter-chip"><span>{t("filteredByCase")}: <span className="mono">{shortId(caseId)}</span></span><button onClick={() => { setCaseId(""); clearInitialCase(); resetPaging(); }} aria-label={t("clearFilter")}>×</button></div> : null}
        <Button variant="quiet" onClick={() => void load()}>{t("refresh")}</Button>
      </div>
      {error ? <ErrorNotice error={error} retry={() => void load()} t={t} /> : null}
      {loading ? <Spinner label={t("loading")} /> : page?.items.length ? (
        <div className="master-detail">
          <Panel className="list-panel"><div className="table-wrap"><table><thead><tr><th>{t("archive")}</th><th>{t("reason")}</th><th>{t("status")}</th><th>{t("createdAt")}</th></tr></thead><tbody>
            {page.items.map((item) => <tr key={item.legal_hold_id} className={selected?.legal_hold_id === item.legal_hold_id ? "is-selected" : ""} onClick={() => setSelected(item)}><td><button className="row-button mono" onClick={() => setSelected(item)}>{shortId(item.archive_id)}</button></td><td>{item.reason}</td><td><StatusBadge status={item.status} t={t} /></td><td>{formatDate(item.created_at, locale)}</td></tr>)}
          </tbody></table></div><Pagination canGoBack={cursorHistory.length > 1} canGoNext={Boolean(page.has_more && page.next_cursor)} onBack={() => { setSelected(undefined); setCursorHistory((items) => items.slice(0, -1)); }} onNext={() => { if (page.next_cursor) { setSelected(undefined); setCursorHistory((items) => [...items, page.next_cursor ?? undefined]); } }} t={t} /></Panel>
          <Panel className="detail-panel">{selected ? <><div className="detail-panel__heading"><div><p className="eyebrow">{t("details")}</p><h2 className="mono">{shortId(selected.legal_hold_id)}</h2></div><StatusBadge status={selected.status} t={t} /></div><dl><dt>{t("archiveId")}</dt><dd className="mono">{selected.archive_id}</dd><dt>{t("caseId")}</dt><dd className="mono">{selected.case_id}</dd><dt>{t("reason")}</dt><dd>{selected.reason}</dd><dt>{t("approval")}</dt><dd className="mono">{selected.apply_approval_request_id}</dd><dt>{t("createdAt")}</dt><dd>{formatDate(selected.created_at, locale)}</dd>{selected.release_approval_request_id ? <><dt>{t("releaseApproval")}</dt><dd className="mono">{selected.release_approval_request_id}</dd><dt>{t("releasedAt")}</dt><dd>{formatDate(selected.released_at, locale)}</dd></> : null}</dl>{selected.status === "active" ? <div className="detail-actions"><Button variant="danger" onClick={() => setDialog("release")}>{t("releaseHold")}</Button></div> : null}</> : <EmptyState title={t("details")} description={t("selectRow")} />}</Panel>
        </div>
      ) : <EmptyState title={t("noHolds")} description={t("noHoldsHint")} action={<Button onClick={() => setDialog("apply")}>{t("applyHold")}</Button>} />}
      {dialog === "apply" ? <Dialog title={t("applyHold")} closeLabel={t("closeDialog")} onClose={() => setDialog(undefined)}><HoldForm client={client} initialCaseId={caseId || undefined} t={t} onClose={() => setDialog(undefined)} onDone={() => { setDialog(undefined); notify(t("holdApplied")); setCursorHistory([undefined]); void load(); }} /></Dialog> : null}
      {dialog === "release" && selected ? <Dialog title={t("releaseHold")} closeLabel={t("closeDialog")} onClose={() => setDialog(undefined)}><ConfirmAction kind="release-hold" client={client} item={selected} t={t} onClose={() => setDialog(undefined)} onDone={() => { setDialog(undefined); setSelected(undefined); notify(t("holdReleased")); void load(); }} /></Dialog> : null}
    </>
  );
}

function Overview({ client, t }: { client: ControlPlaneClient; t: ReturnType<typeof translator> }) {
  const [summary, setSummary] = useState<{ cases: string; holds: string }>();
  const [error, setError] = useState<unknown>();
  useEffect(() => {
    Promise.all([client.listCases({ status: "open", limit: 100 }), client.listHolds({ status: "active", limit: 100 })])
      .then(([cases, holds]) => setSummary({
        cases: `${cases.items.length}${cases.has_more ? "+" : ""}`,
        holds: `${holds.items.length}${holds.has_more ? "+" : ""}`,
      }))
      .catch(setError);
  }, [client]);
  return (
    <>
      <div className="page-heading"><div><p className="eyebrow">{t("adminCenter")}</p><h1>{t("overviewTitle")}</h1><p>{t("overviewHint")}</p></div></div>
      {error ? <ErrorNotice error={error} t={t} /> : null}
      <div className="summary-grid">
        <Panel><span className="summary-card__label">{t("openCases")}</span><strong>{summary?.cases ?? "—"}</strong></Panel>
        <Panel><span className="summary-card__label">{t("activeHolds")}</span><strong>{summary?.holds ?? "—"}</strong></Panel>
        <Panel><span className="summary-card__label">{t("systemBoundary")}</span><p>{t("systemBoundaryHint")}</p></Panel>
      </div>
    </>
  );
}

export default function App() {
  const [locale, setLocale] = useState<Locale>(() => navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en");
  const [auth, setAuth] = useState<AuthState>({ kind: "checking" });
  const [view, setView] = useState<View>("overview");
  const [caseFilter, setCaseFilter] = useState<string>();
  const [toast, setToast] = useState<string>();
  const t = useMemo(() => translator(locale), [locale]);
  const problems = configurationProblems(config);

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);
  useEffect(() => {
    if (problems.length) return;
    loadSession(config)
      .then((session) => setAuth(session ? { kind: "authenticated" } : { kind: "signed-out" }))
      .catch((error: Error) => setAuth({ kind: "error", message: error.message }));
  }, []);
  useEffect(() => {
    if (!toast) return;
    const timeout = window.setTimeout(() => setToast(undefined), 5000);
    return () => window.clearTimeout(timeout);
  }, [toast]);

  if (problems.length) {
    return <main className="centered"><Panel className="auth-card"><div className="brand-mark">C</div><h1>{t("configurationRequired")}</h1><p>{t("configurationHint")}</p><div className="notice"><strong>{t("missingFields")}</strong><code>{problems.join(", ")}</code></div><LocaleSwitch locale={locale} setLocale={setLocale} t={t} /></Panel></main>;
  }
  if (auth.kind === "checking") return <main className="centered"><Spinner label={t("loading")} /></main>;
  if (auth.kind !== "authenticated") {
    return <main className="centered"><Panel className="auth-card"><div className="brand-mark">C</div><p className="eyebrow">{t("product")}</p><h1>{t("secureSignIn")}</h1><p>{t("secureSignInHint")}</p>{auth.kind === "error" ? <ErrorNotice error={new Error(auth.message)} t={t} /> : null}<Button onClick={() => { try { startLogin(config); } catch (error) { setAuth({ kind: "error", message: error instanceof Error ? error.message : "login_failed" }); } }}>{t("signIn")}</Button><LocaleSwitch locale={locale} setLocale={setLocale} t={t} /></Panel></main>;
  }

  const client = new ControlPlaneClient(config.apiBaseUrl, config.organizationId);
  function navigate(next: View) { setView(next); if (next !== "holds") setCaseFilter(undefined); }
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark">C</div><div><strong>{t("product")}</strong><span>{t("adminCenter")}</span></div></div>
        <nav>{(["overview", "cases", "holds", "approvals", "roles", "audit", "retention"] as View[]).map((item) => <button key={item} className={view === item ? "is-active" : ""} onClick={() => navigate(item)}><span aria-hidden="true">{item === "overview" ? "⌂" : item === "cases" ? "▤" : item === "holds" ? "◇" : item === "approvals" ? "✓" : item === "roles" ? "♙" : item === "retention" ? "◫" : "⌁"}</span>{t(item)}</button>)}</nav>
        <div className="sidebar__footer"><span>{t("tenant")}</span><code>{shortId(config.organizationId)}</code><LocaleSwitch locale={locale} setLocale={setLocale} t={t} /><Button variant="quiet" onClick={() => { setAuth({ kind: "checking" }); logout(config).then(() => setAuth({ kind: "signed-out" })).catch((error: Error) => setAuth({ kind: "error", message: error.message })); }}>{t("signOut")}</Button></div>
      </aside>
      <main className="workspace">
        {view === "overview" ? <Overview client={client} t={t} /> : null}
        {view === "cases" ? <CasesView client={client} locale={locale} t={t} notify={setToast} openHolds={(id) => { setCaseFilter(id); setView("holds"); }} /> : null}
        {view === "holds" ? <HoldsView client={client} locale={locale} t={t} notify={setToast} initialCaseId={caseFilter} clearInitialCase={() => setCaseFilter(undefined)} /> : null}
        {view === "approvals" ? <ApprovalView client={client} locale={locale} t={t} /> : null}
        {view === "roles" ? <RolesView client={client} locale={locale} t={t} /> : null}
        {view === "audit" ? <AuditView client={client} locale={locale} t={t} /> : null}
        {view === "retention" ? <RetentionView client={client} locale={locale} t={t} /> : null}
      </main>
      <div className="sr-only" role="status" aria-live="polite">{toast}</div>
      {toast ? <div className="toast" aria-hidden="true">✓ {toast}</div> : null}
    </div>
  );
}

function LocaleSwitch({
  locale,
  setLocale,
  t,
}: {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: ReturnType<typeof translator>;
}) {
  return <label className="locale-switch"><span>{t("language")}</span><select value={locale} onChange={(event) => setLocale(event.target.value as Locale)}><option value="zh-CN">简体中文</option><option value="en">English</option></select></label>;
}
