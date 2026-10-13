import {
  AddIcon,
  Badge,
  Button,
  ConfirmDialog,
  ContextBar,
  CloseIcon,
  Dialog,
  EmptyComplianceArt,
  EmptyState,
  EvidenceInspector,
  Field,
  IconButton,
  Input,
  Select,
  SelectRecordArt,
  Spinner,
  StatusIndicator,
  Textarea,
  UnknownWriteLock,
} from "@copilotmail/design-system";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type FormEvent,
  type RefObject,
  type SetStateAction,
} from "react";
import { AdminErrorNotice } from "./AdminErrorNotice";
import { AdminListDetail } from "./AdminListDetail";
import { ControlPlaneClient, isUnknownWriteFailure } from "./api";
import { formatNullableDate as formatDate, shortId } from "./format";
import { translator } from "./i18n";
import type {
  CaseStatus,
  ComplianceCase,
  HoldStatus,
  LegalHold,
  Locale,
  Page,
} from "./types";



function caseHoldStatusTone(status: CaseStatus | HoldStatus): "warning" | "neutral" {
  return status === "open" || status === "active" ? "warning" : "neutral";
}

function StatusBadge({ status, t }: { status: CaseStatus | HoldStatus; t: ReturnType<typeof translator> }) {
  const tone = caseHoldStatusTone(status);
  return <Badge tone={tone}>{t(status)}</Badge>;
}

export interface QueryScopedCommit<T> {
  item: T;
  queryKey: string;
}

export type CommittedCaseEvidence = {
  kind: "created" | "closed";
  item: ComplianceCase;
};

export type CommittedHoldEvidence = {
  kind: "applied" | "released";
  item: LegalHold;
};

export interface CaseCommitStore {
  terminal: Map<string, QueryScopedCommit<ComplianceCase>>;
  evidence?: CommittedCaseEvidence;
  setEvidence: Dispatch<SetStateAction<CommittedCaseEvidence | undefined>>;
}

export interface HoldCommitStore {
  terminal: Map<string, QueryScopedCommit<LegalHold>>;
  evidence?: CommittedHoldEvidence;
  setEvidence: Dispatch<SetStateAction<CommittedHoldEvidence | undefined>>;
}

function reconcileCommittedPage<T>(
  page: Page<T>,
  committed: ReadonlyMap<string, QueryScopedCommit<T>>,
  keyOf: (item: T) => string,
  includes: (item: T) => boolean,
  queryKey: string,
): Page<T> {
  const seen = new Set<string>();
  const items = page.items.flatMap((item) => {
    const key = keyOf(item);
    seen.add(key);
    const value = committed.get(key)?.item ?? item;
    return includes(value) ? [value] : [];
  });
  for (const evidence of committed.values()) {
    const key = keyOf(evidence.item);
    if (
      evidence.queryKey === queryKey
      && !seen.has(key)
      && includes(evidence.item)
    ) {
      seen.add(key);
      items.push(evidence.item);
    }
  }
  return {
    ...page,
    items,
  };
}

export function Pagination({
  canGoBack,
  canGoNext,
  disabled,
  label,
  onBack,
  onNext,
  t,
}: {
  canGoBack: boolean;
  canGoNext: boolean;
  disabled?: boolean;
  label?: string;
  onBack: () => void;
  onNext: () => void;
  t: ReturnType<typeof translator>;
}) {
  if (!canGoBack && !canGoNext) return null;
  return (
    <nav className="pagination" aria-label={label ?? t("pagination")}>
      <Button variant="secondary" disabled={!canGoBack || disabled} onClick={onBack}>{t("previous")}</Button>
      <Button variant="secondary" disabled={!canGoNext || disabled} onClick={onNext}>{t("next")}</Button>
    </nav>
  );
}

interface CreateCaseCommand {
  external_reference: string;
  title: string;
  purpose: string;
}

interface ApplyHoldCommand {
  archiveId: string;
  caseId: string;
  approvalId: string;
  reason: string;
}

function CaseForm({
  firstFieldRef,
  submitting,
  onSubmit,
  onClose,
  t,
}: {
  firstFieldRef: RefObject<HTMLInputElement | null>;
  submitting: boolean;
  onSubmit: (input: CreateCaseCommand) => Promise<void>;
  onClose: () => void;
  t: ReturnType<typeof translator>;
}) {
  const [externalReference, setExternalReference] = useState("");
  const [title, setTitle] = useState("");
  const [purpose, setPurpose] = useState("");
  const [error, setError] = useState<unknown>();
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (submitting) return;
    if (!externalReference.trim() || !title.trim() || !purpose.trim()) {
      setError(new Error(t("fieldRequired")));
      return;
    }
    setError(undefined);
    try {
      await onSubmit({
        external_reference: externalReference.trim(),
        title: title.trim(),
        purpose: purpose.trim(),
      });
    } catch (cause) {
      setError(cause);
    }
  }
  return (
    <form onSubmit={submit}>
      <div className="dialog__body form-grid">
        {error ? <AdminErrorNotice error={error} t={t} /> : null}
        <Field label={t("externalReference")}><Input ref={firstFieldRef} disabled={submitting} value={externalReference} onChange={(event) => setExternalReference(event.target.value)} required maxLength={160} /></Field>
        <Field label={t("title")}><Input disabled={submitting} value={title} onChange={(event) => setTitle(event.target.value)} required maxLength={200} /></Field>
        <Field label={t("purpose")}><Textarea disabled={submitting} value={purpose} onChange={(event) => setPurpose(event.target.value)} required maxLength={2000} rows={4} /></Field>
      </div>
      <div className="dialog__actions">
        <Button type="button" variant="secondary" disabled={submitting} onClick={onClose}>{t("cancel")}</Button>
        <Button type="submit" disabled={submitting}>{t("create")}</Button>
      </div>
    </form>
  );
}

function HoldForm({
  firstFieldRef,
  initialCaseId,
  submitting,
  onSubmit,
  onClose,
  t,
}: {
  firstFieldRef: RefObject<HTMLInputElement | null>;
  initialCaseId?: string;
  submitting: boolean;
  onSubmit: (input: ApplyHoldCommand) => Promise<void>;
  onClose: () => void;
  t: ReturnType<typeof translator>;
}) {
  const [archiveId, setArchiveId] = useState("");
  const [caseId, setCaseId] = useState(initialCaseId ?? "");
  const [approvalId, setApprovalId] = useState("");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<unknown>();
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (submitting) return;
    if (![archiveId, caseId, approvalId, reason].every((value) => value.trim())) {
      setError(new Error(t("fieldRequired")));
      return;
    }
    setError(undefined);
    try {
      await onSubmit({
        archiveId: archiveId.trim(),
        caseId: caseId.trim(),
        approvalId: approvalId.trim(),
        reason: reason.trim(),
      });
    } catch (cause) {
      setError(cause);
    }
  }
  return (
    <form onSubmit={submit}>
      <div className="dialog__body form-grid">
        {error ? <AdminErrorNotice error={error} t={t} /> : null}
        <div className="notice"><p>{t("approvalHint")}</p></div>
        <Field label={t("archiveId")}><Input ref={firstFieldRef} disabled={submitting} value={archiveId} onChange={(event) => setArchiveId(event.target.value)} required /></Field>
        <Field label={t("caseId")}><Input disabled={submitting} value={caseId} onChange={(event) => setCaseId(event.target.value)} required /></Field>
        <Field label={t("approvalId")}><Input disabled={submitting} value={approvalId} onChange={(event) => setApprovalId(event.target.value)} required /></Field>
        <Field label={t("reason")}><Textarea disabled={submitting} value={reason} onChange={(event) => setReason(event.target.value)} required maxLength={2000} rows={3} /></Field>
      </div>
      <div className="dialog__actions">
        <Button type="button" variant="secondary" disabled={submitting} onClick={onClose}>{t("cancel")}</Button>
        <Button type="submit" disabled={submitting}>{t("applyHold")}</Button>
      </div>
    </form>
  );
}

type ConfirmActionProps = {
  client: ControlPlaneClient;
  locale: Locale;
  onClose: () => void;
  returnFocusRef?: RefObject<HTMLElement | null>;
  t: ReturnType<typeof translator>;
} & (
  | {
    kind: "close-case";
    item: ComplianceCase;
    onDone: (item: ComplianceCase) => void;
  }
  | {
    kind: "release-hold";
    item: LegalHold;
    onDone: (item: LegalHold) => void;
  }
);

function ConfirmAction(props: ConfirmActionProps) {
  const [approvalId, setApprovalId] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [reconciling, setReconciling] = useState(false);
  const [error, setError] = useState<unknown>();
  async function converge(frozenApprovalId: string): Promise<boolean> {
    try {
      if (props.kind === "close-case") {
        const snapshot = await props.client.getCase(props.item.case_id);
        if (snapshot.case_id === props.item.case_id && snapshot.status === "closed") {
          props.onDone(snapshot);
          return true;
        }
      } else {
        const snapshot = await props.client.getHold(props.item.legal_hold_id);
        if (
          snapshot.legal_hold_id === props.item.legal_hold_id
          && snapshot.status === "released"
          && snapshot.release_approval_request_id === frozenApprovalId
        ) {
          props.onDone(snapshot);
          return true;
        }
      }
    } catch {
      // A failed or stale reconciliation read is not proof that the write did
      // not commit. The command remains frozen until a read proves its state.
    }
    return false;
  }
  async function reconcileUnknownWrite() {
    if (submitting || reconciling || !uncertain) return;
    setReconciling(true);
    const reconciled = await converge(approvalId.trim());
    // `onDone` closes this dialog on success. On a missing, stale, or failed
    // read, deliberately keep the command frozen and permit another read.
    if (!reconciled) setReconciling(false);
  }
  async function submit() {
    if (submitting) return;
    const frozenApprovalId = approvalId.trim();
    if (props.kind === "release-hold" && !frozenApprovalId) {
      setError(new Error(props.t("fieldRequired")));
      return;
    }
    setSubmitting(true);
    setError(undefined);
    try {
      if (props.kind === "close-case") {
        const result = await props.client.closeCase(props.item.case_id);
        props.onDone(result);
      } else {
        const result = await props.client.releaseHold(
          props.item.legal_hold_id,
          frozenApprovalId,
        );
        props.onDone(result);
      }
    } catch (cause) {
      if (isUnknownWriteFailure(cause)) {
        if (await converge(frozenApprovalId)) return;
        setUncertain(true);
      }
      setError(cause);
      setSubmitting(false);
    }
  }
  const isRelease = props.kind === "release-hold";
  return (
    <ConfirmDialog
      cancelLabel={props.t("cancel")}
      busy={submitting || uncertain}
      closeLabel={props.t("closeDialog")}
      confirmLabel={isRelease ? props.t("confirmRelease") : props.t("confirmClose")}
      description={isRelease ? props.t("approvalHint") : props.t("closeBlockedHint")}
      returnFocusRef={props.returnFocusRef}
      title={isRelease ? props.t("releaseHold") : props.t("closeCase")}
      onCancel={() => {
        if (!uncertain) props.onClose();
      }}
      onConfirm={() => void submit()}
    >
      <section
        aria-labelledby="case-hold-confirmation-facts"
        className="case-hold-confirmation"
      >
        <h2 id="case-hold-confirmation-facts">{props.t("confirmationFacts")}</h2>
        {props.kind === "close-case" ? (
          <>
            <dl className="case-hold-fact-list">
              <dt>{props.t("caseId")}</dt>
              <dd><code>{props.item.case_id}</code></dd>
              <dt>{props.t("externalReference")}</dt>
              <dd>{props.item.external_reference}</dd>
              <dt>{props.t("status")}</dt>
              <dd><StatusIndicator label={props.t(props.item.status)} tone={caseHoldStatusTone(props.item.status)} /></dd>
              <dt>{props.t("createdBy")}</dt>
              <dd><code>{props.item.created_by}</code></dd>
              <dt>{props.t("createdAt")}</dt>
              <dd>{formatDate(props.item.created_at, props.locale)}</dd>
            </dl>
            <p className="case-hold-confirmation__impact">
              <strong>{props.t("impact")}</strong>
              {props.t("caseCloseImpact")}
            </p>
          </>
        ) : (
          <>
            <dl className="case-hold-fact-list">
              <dt>{props.t("legalHoldId")}</dt>
              <dd><code>{props.item.legal_hold_id}</code></dd>
              <dt>{props.t("archiveId")}</dt>
              <dd><code>{props.item.archive_id}</code></dd>
              <dt>{props.t("caseId")}</dt>
              <dd><code>{props.item.case_id}</code></dd>
              <dt>{props.t("approval")}</dt>
              <dd><code>{props.item.apply_approval_request_id}</code></dd>
              <dt>{props.t("createdBy")}</dt>
              <dd><code>{props.item.created_by}</code></dd>
              <dt>{props.t("createdAt")}</dt>
              <dd>{formatDate(props.item.created_at, props.locale)}</dd>
            </dl>
            <p className="case-hold-confirmation__reason">
              <strong>{props.t("reason")}</strong>
              {props.item.reason}
            </p>
            <p className="case-hold-confirmation__impact">
              <strong>{props.t("impact")}</strong>
              {props.t("holdReleaseImpact")}
            </p>
          </>
        )}
      </section>
      <div className="form-grid">
        {error ? <AdminErrorNotice error={error} t={props.t} /> : null}
        {uncertain ? (
          <UnknownWriteLock
            aria-label={props.t("caseHoldWriteOutcomeUnknown")}
            description={props.t("caseHoldWriteOutcomeUnknownHint")}
            evidence={(
              <>
                <p>
                  <span>{props.kind === "close-case" ? props.t("caseId") : props.t("legalHoldId")}:</span>
                  {" "}
                  <code>{props.kind === "close-case" ? props.item.case_id : props.item.legal_hold_id}</code>
                </p>
                {isRelease ? (
                  <p>
                    <span>{props.t("approvalId")}:</span> <code>{approvalId.trim()}</code>
                  </p>
                ) : null}
              </>
            )}
            reconciliationAction={(
              <Button
                type="button"
                variant="secondary"
                disabled={submitting || reconciling}
                onClick={() => void reconcileUnknownWrite()}
              >
                {reconciling ? props.t("loading") : props.t("reconcileApprovalState")}
              </Button>
            )}
            title={props.t("caseHoldWriteOutcomeUnknown")}
          />
        ) : null}
        {isRelease ? (
          <Field label={props.t("approvalId")}><Input disabled={submitting || uncertain} value={approvalId} onChange={(event) => setApprovalId(event.target.value)} required /></Field>
        ) : null}
      </div>
    </ConfirmDialog>
  );
}

function CaseDetailWorkspace({
  item,
  locale,
  onCloseCase,
  onOpenHolds,
  t,
}: {
  item: ComplianceCase;
  locale: Locale;
  onCloseCase: () => void;
  onOpenHolds: () => void;
  t: ReturnType<typeof translator>;
}) {
  return (
    <article className="case-hold-workspace case-detail-workspace">
      <ContextBar
        actions={(
          <Button type="button" variant="secondary" onClick={onOpenHolds}>
            {t("viewHolds")}
          </Button>
        )}
        className="case-hold-context"
        facts={[
          { id: "case-id", label: t("caseId"), value: <code>{item.case_id}</code> },
          { id: "external-reference", label: t("externalReference"), value: item.external_reference },
          { id: "created-at", label: t("createdAt"), value: formatDate(item.created_at, locale) },
        ]}
        headingLevel={3}
        status={<StatusIndicator label={t(item.status)} tone={caseHoldStatusTone(item.status)} />}
        title={t("caseContext")}
      />

      <section aria-labelledby="case-facts-heading" className="case-hold-facts">
        <h3 id="case-facts-heading">{t("caseFacts")}</h3>
        <dl className="case-hold-fact-list">
          <dt>{t("purpose")}</dt>
          <dd>{item.purpose}</dd>
        </dl>
      </section>

      <section aria-labelledby="case-decision-heading" className="case-hold-decision">
        <h3 id="case-decision-heading">{t("caseCurrentDecision")}</h3>
        {item.status === "open" ? (
          <>
            <p>{t("closeBlockedHint")}</p>
            <div className="detail-actions">
              <Button type="button" onClick={onCloseCase}>{t("closeCase")}</Button>
            </div>
          </>
        ) : <p>{t("caseNoFurtherDecision")}</p>}
      </section>

      <EvidenceInspector
        className="case-hold-evidence"
        defaultOpen={false}
        summary={t("caseEvidence")}
      >
        <dl className="case-hold-fact-list">
          <dt>{t("caseId")}</dt>
          <dd><code>{item.case_id}</code></dd>
          <dt>{t("createdBy")}</dt>
          <dd><code>{item.created_by}</code></dd>
          <dt>{t("createdAt")}</dt>
          <dd>{formatDate(item.created_at, locale)}</dd>
          {item.closed_by ? <><dt>{t("closedBy")}</dt><dd><code>{item.closed_by}</code></dd></> : null}
          {item.closed_at ? <><dt>{t("closedAt")}</dt><dd>{formatDate(item.closed_at, locale)}</dd></> : null}
        </dl>
      </EvidenceInspector>
    </article>
  );
}

function HoldDetailWorkspace({
  item,
  locale,
  onOpenCase,
  onReleaseHold,
  t,
}: {
  item: LegalHold;
  locale: Locale;
  onOpenCase?: () => void;
  onReleaseHold: () => void;
  t: ReturnType<typeof translator>;
}) {
  return (
    <article className="case-hold-workspace hold-detail-workspace">
      <ContextBar
        actions={onOpenCase ? (
          <Button type="button" variant="secondary" onClick={onOpenCase}>
            {t("viewCase")}
          </Button>
        ) : undefined}
        className="case-hold-context"
        facts={[
          { id: "hold-id", label: t("legalHoldId"), value: <code>{item.legal_hold_id}</code> },
          { id: "archive-id", label: t("archiveId"), value: <code>{item.archive_id}</code> },
          { id: "case-id", label: t("caseId"), value: <code>{item.case_id}</code> },
          { id: "created-at", label: t("createdAt"), value: formatDate(item.created_at, locale) },
        ]}
        headingLevel={3}
        status={<StatusIndicator label={t(item.status)} tone={caseHoldStatusTone(item.status)} />}
        title={t("legalHoldContext")}
      />

      <section aria-labelledby="hold-facts-heading" className="case-hold-facts">
        <h3 id="hold-facts-heading">{t("legalHoldFacts")}</h3>
        <dl className="case-hold-fact-list">
          <dt>{t("reason")}</dt>
          <dd>{item.reason}</dd>
        </dl>
      </section>

      <section aria-labelledby="hold-decision-heading" className="case-hold-decision">
        <h3 id="hold-decision-heading">{t("legalHoldCurrentDecision")}</h3>
        {item.status === "active" ? (
          <>
            <p>{t("approvalHint")}</p>
            <div className="detail-actions">
              <Button type="button" variant="danger" onClick={onReleaseHold}>{t("releaseHold")}</Button>
            </div>
          </>
        ) : <p>{t("holdNoFurtherDecision")}</p>}
      </section>

      <EvidenceInspector
        className="case-hold-evidence"
        defaultOpen={false}
        summary={t("legalHoldEvidence")}
      >
        <dl className="case-hold-fact-list">
          <dt>{t("legalHoldId")}</dt>
          <dd><code>{item.legal_hold_id}</code></dd>
          <dt>{t("archiveId")}</dt>
          <dd><code>{item.archive_id}</code></dd>
          <dt>{t("caseId")}</dt>
          <dd><code>{item.case_id}</code></dd>
          <dt>{t("approval")}</dt>
          <dd><code>{item.apply_approval_request_id}</code></dd>
          <dt>{t("createdBy")}</dt>
          <dd><code>{item.created_by}</code></dd>
          <dt>{t("createdAt")}</dt>
          <dd>{formatDate(item.created_at, locale)}</dd>
          {item.release_approval_request_id ? <><dt>{t("releaseApproval")}</dt><dd><code>{item.release_approval_request_id}</code></dd></> : null}
          {item.released_by ? <><dt>{t("releasedBy")}</dt><dd><code>{item.released_by}</code></dd></> : null}
          {item.released_at ? <><dt>{t("releasedAt")}</dt><dd>{formatDate(item.released_at, locale)}</dd></> : null}
        </dl>
      </EvidenceInspector>
    </article>
  );
}

export function CasesView({
  client,
  commitStore,
  initialCaseId,
  clearInitialCase,
  locale,
  t,
  notify,
  openHolds,
}: {
  client: ControlPlaneClient;
  commitStore?: CaseCommitStore;
  initialCaseId?: string;
  clearInitialCase?: () => void;
  locale: Locale;
  t: ReturnType<typeof translator>;
  notify: (message: string) => void;
  openHolds: (caseId: string) => void;
}) {
  const [status, setStatus] = useState<CaseStatus | "">("open");
  const [page, setPage] = useState<Page<ComplianceCase>>();
  const [cursorHistory, setCursorHistory] = useState<(string | undefined)[]>([undefined]);
  const [error, setError] = useState<unknown>();
  const [exactCaseError, setExactCaseError] = useState<unknown>();
  const [exactCaseLoading, setExactCaseLoading] = useState(false);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<ComplianceCase>();
  const [dialog, setDialog] = useState<"create">();
  const [creatingCase, setCreatingCase] = useState<CreateCaseCommand>();
  const [uncertainCaseCreate, setUncertainCaseCreate] = useState<CreateCaseCommand>();
  const [closingCase, setClosingCase] = useState<QueryScopedCommit<ComplianceCase>>();
  const [localCommittedCaseEvidence, setLocalCommittedCaseEvidence] = useState<CommittedCaseEvidence>();
  const createCaseInFlight = useRef(false);
  const requestSequence = useRef(0);
  const exactCaseReadSequence = useRef(0);
  const exactCaseFocusId = useRef<string | undefined>(undefined);
  const localCommittedCases = useRef(new Map<string, QueryScopedCommit<ComplianceCase>>());
  const committedCases = commitStore
    ? { current: commitStore.terminal }
    : localCommittedCases;
  const committedCaseEvidence = commitStore?.evidence ?? localCommittedCaseEvidence;
  const setCommittedCaseEvidence = commitStore?.setEvidence ?? setLocalCommittedCaseEvidence;
  const createCaseButtonRef = useRef<HTMLButtonElement>(null);
  const createCaseFirstFieldRef = useRef<HTMLInputElement>(null);
  const cursor = cursorHistory.at(-1);
  const caseQueryKey = JSON.stringify([status, cursor ?? null]);
  const includesCase = (item: ComplianceCase) => !status || item.status === status;
  const load = async () => {
    const sequence = requestSequence.current + 1;
    requestSequence.current = sequence;
    setLoading(true);
    setError(undefined);
    try {
      const response = await client.listCases({ status: status || undefined, cursor, limit: 25 });
      if (sequence !== requestSequence.current) return;
      const result = reconcileCommittedPage(
        response,
        committedCases.current,
        (item) => item.case_id,
        includesCase,
        caseQueryKey,
      );
      setPage(result);
      setSelected((current) => {
        // A Case reached from a Legal Hold is proven only by its own exact
        // detail read. A cursor page must not erase or replace that evidence.
        if (current?.case_id === exactCaseFocusId.current) return current;
        return current
          ? result.items.find((item) => item.case_id === current.case_id)
          : undefined;
      });
    } catch (cause) {
      if (sequence !== requestSequence.current) return;
      setError(cause);
    } finally {
      if (sequence === requestSequence.current) setLoading(false);
    }
  };
  useEffect(() => {
    void load();
    return () => {
      requestSequence.current += 1;
    };
  }, [status, cursor]);
  const loadExactLinkedCase = useCallback(async (caseId: string) => {
    const sequence = exactCaseReadSequence.current + 1;
    exactCaseReadSequence.current = sequence;
    exactCaseFocusId.current = caseId;
    setExactCaseLoading(true);
    setExactCaseError(undefined);
    try {
      const exactCase = await client.getCase(caseId);
      if (sequence !== exactCaseReadSequence.current) return;
      // ControlPlaneClient already binds the path ID to this DTO. Keep the
      // local check at the page boundary so this route can never infer a Case
      // from a broad list response.
      if (exactCase.case_id !== caseId) throw new Error("invalid_exact_case_read");
      setSelected(exactCase);
    } catch (cause) {
      if (sequence === exactCaseReadSequence.current) setExactCaseError(cause);
    } finally {
      if (sequence === exactCaseReadSequence.current) setExactCaseLoading(false);
    }
  }, [client]);
  useEffect(() => {
    if (!initialCaseId) return;
    void loadExactLinkedCase(initialCaseId);
    return () => {
      exactCaseReadSequence.current += 1;
    };
  }, [initialCaseId, loadExactLinkedCase]);
  function clearExactCaseFocus() {
    exactCaseReadSequence.current += 1;
    exactCaseFocusId.current = undefined;
    setExactCaseLoading(false);
    setExactCaseError(undefined);
    clearInitialCase?.();
  }
  function prepareQueryChange() {
    requestSequence.current += 1;
    clearExactCaseFocus();
    setLoading(true);
    setPage(undefined);
    setSelected(undefined);
    setError(undefined);
  }
  function changeStatus(value: string) {
    prepareQueryChange();
    setStatus(value as CaseStatus | "");
    setCursorHistory([undefined]);
  }
  function acceptCreatedCase(committed: ComplianceCase) {
    if (committed.status === "closed") {
      committedCases.current.set(committed.case_id, {
        item: committed,
        queryKey: "reconciled-create",
      });
    }
    setCommittedCaseEvidence({ kind: "created", item: committed });
    setUncertainCaseCreate(undefined);
    setDialog(undefined);
    notify(t("caseCreated"));
    if (cursor !== undefined || cursorHistory.length > 1) {
      prepareQueryChange();
      setCursorHistory([undefined]);
    } else {
      void load();
    }
  }
  async function createCase(input: CreateCaseCommand) {
    if (createCaseInFlight.current) return;
    const request = { ...input };
    createCaseInFlight.current = true;
    setCreatingCase(request);
    try {
      const committed = await client.createCase(request);
      if (
        !committed.case_id
        || committed.status !== "open"
        || committed.external_reference !== request.external_reference
        || committed.title !== request.title
        || committed.purpose !== request.purpose
      ) throw new Error("invalid_create_case_response");

      // Creation proves the write, but not membership on any cursor page.
      acceptCreatedCase(committed);
    } catch (cause) {
      if (isUnknownWriteFailure(cause)) {
        // The creation response did not disclose a case ID. Listing candidate
        // cases cannot prove which record, if any, belongs to this POST, so
        // do not turn a broad query into a false reconciliation result.
        setUncertainCaseCreate(request);
      }
      throw cause;
    } finally {
      createCaseInFlight.current = false;
      setCreatingCase(undefined);
    }
  }
  const caseEvidenceLabel = committedCaseEvidence?.kind === "created"
    ? t("caseCreated")
    : t("caseClosed");
  return (
    <>
      <div className="page-heading">
        <div><p className="eyebrow">{t("adminCenter")}</p><h1>{t("cases")}</h1></div>
        <Button ref={createCaseButtonRef} onClick={() => setDialog("create")}><AddIcon aria-hidden size={17} />{t("newCase")}</Button>
      </div>
      <div className="toolbar">
        <Field label={t("status")}>
          <Select value={status} onChange={(event) => changeStatus(event.target.value)}>
            <option value="">{t("all")}</option><option value="open">{t("open")}</option><option value="closed">{t("closed")}</option>
          </Select>
        </Field>
        <Button variant="quiet" onClick={() => void load()}>{t("refresh")}</Button>
      </div>
      {committedCaseEvidence ? (
        <div className="notice" role="status" aria-label={caseEvidenceLabel}>
          <p><strong>{caseEvidenceLabel}</strong> · {committedCaseEvidence.item.title}</p>
          <code>{committedCaseEvidence.item.case_id}</code>
          <StatusBadge status={committedCaseEvidence.item.status} t={t} />
        </div>
      ) : null}
      {exactCaseLoading ? <Spinner label={t("loadingLinkedCase")} /> : null}
      {exactCaseError && initialCaseId ? (
        <AdminErrorNotice
          error={exactCaseError}
          retry={() => void loadExactLinkedCase(initialCaseId)}
          t={t}
        />
      ) : null}
      {error ? <AdminErrorNotice error={error} retry={() => void load()} t={t} /> : null}
      {loading ? <Spinner label={t("loading")} /> : null}
      {page?.items.length || selected ? (
        <AdminListDetail
          backLabel={t("backToCases")}
          detailBody={selected ? (
            <CaseDetailWorkspace
              item={selected}
              locale={locale}
              t={t}
              onCloseCase={() => setClosingCase({ item: selected, queryKey: caseQueryKey })}
              onOpenHolds={() => openHolds(selected.case_id)}
            />
          ) : null}
          detailClassName="case-hold-detail"
          detailEmpty={<EmptyState art={<SelectRecordArt />} title={t("details")} description={t("selectRow")} />}
          detailHeader={<p className="eyebrow">{t("details")}</p>}
          detailTitle={selected?.title ?? t("details")}
          detailTrailing={selected ? <StatusBadge status={selected.status} t={t} /> : null}
          listLabel={t("cases")}
          selectionKey={selected?.case_id ?? null}
          onBack={() => {
            clearExactCaseFocus();
            setSelected(undefined);
          }}
          renderList={(registerTrigger) => (
            page?.items.length ? <>
              <div className="table-wrap">
                <table>
                  <thead><tr><th>{t("title")}</th><th>{t("externalReference")}</th><th>{t("status")}</th><th>{t("createdAt")}</th></tr></thead>
                  <tbody>
                    {page.items.map((item) => (
                      <tr key={item.case_id} className={selected?.case_id === item.case_id ? "is-selected" : ""} onClick={() => { clearExactCaseFocus(); setSelected(item); }}>
                        <td><button ref={registerTrigger(item.case_id)} aria-current={selected?.case_id === item.case_id ? "true" : undefined} className="row-button" onClick={() => { clearExactCaseFocus(); setSelected(item); }}>{item.title}</button></td>
                        <td>{item.external_reference}</td><td><StatusBadge status={item.status} t={t} /></td><td>{formatDate(item.created_at, locale)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <Pagination canGoBack={cursorHistory.length > 1} canGoNext={Boolean(page.has_more && page.next_cursor)}
                onBack={() => { prepareQueryChange(); setCursorHistory((items) => items.slice(0, -1)); }}
                onNext={() => { if (page.next_cursor) { prepareQueryChange(); setCursorHistory((items) => [...items, page.next_cursor ?? undefined]); } }} t={t} />
            </> : page ? <EmptyState art={<EmptyComplianceArt />} title={t("noCases")} description={t("noCasesHint")} /> : <Spinner label={t("loading")} />
          )}
        />
      ) : page ? <EmptyState art={<EmptyComplianceArt />} title={t("noCases")} description={t("noCasesHint")} /> : null}
      {dialog === "create" ? (
        <Dialog
          busy={Boolean(creatingCase || uncertainCaseCreate)}
          closeLabel={t("closeDialog")}
          dismissDisabled={Boolean(creatingCase || uncertainCaseCreate)}
          initialFocusRef={createCaseFirstFieldRef}
          returnFocusRef={createCaseButtonRef}
          title={t("newCase")}
          onClose={() => {
            if (!creatingCase && !uncertainCaseCreate) setDialog(undefined);
          }}
        >
          {uncertainCaseCreate ? (
            <UnknownWriteLock
              aria-label={t("writeOutcomeUnknown")}
              description={t("writeOutcomeExactReadUnavailable")}
              title={t("writeOutcomeUnknown")}
            />
          ) : null}
          <CaseForm
            firstFieldRef={createCaseFirstFieldRef}
            submitting={Boolean(creatingCase || uncertainCaseCreate)}
            t={t}
            onClose={() => {
              if (!creatingCase && !uncertainCaseCreate) setDialog(undefined);
            }}
            onSubmit={createCase}
          />
        </Dialog>
      ) : null}
      {closingCase ? (
        <ConfirmAction
          kind="close-case"
          client={client}
          item={closingCase.item}
          locale={locale}
          returnFocusRef={createCaseButtonRef}
          t={t}
          onClose={() => setClosingCase(undefined)}
          onDone={(committed) => {
            if (
              committed.case_id !== closingCase.item.case_id
              || committed.status !== "closed"
            ) throw new Error("invalid_close_case_response");
            committedCases.current.set(committed.case_id, {
              item: committed,
              queryKey: closingCase.queryKey,
            });
            setCommittedCaseEvidence({ kind: "closed", item: committed });
            setPage((current) => current && reconcileCommittedPage(
              current,
              committedCases.current,
              (item) => item.case_id,
              includesCase,
              caseQueryKey,
            ));
            setSelected((current) => {
              const visibleInQuery = closingCase.queryKey === caseQueryKey
                && includesCase(committed);
              if (visibleInQuery && (!current || current.case_id === committed.case_id)) {
                return committed;
              }
              return current?.case_id === committed.case_id ? undefined : current;
            });
            setClosingCase(undefined);
            notify(t("caseClosed"));
            void load();
          }}
        />
      ) : null}
    </>
  );
}

export function HoldsView({
  client,
  commitStore,
  openCase,
  locale,
  t,
  notify,
  initialCaseId,
  clearInitialCase,
}: {
  client: ControlPlaneClient;
  commitStore?: HoldCommitStore;
  openCase?: (caseId: string) => void;
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
  const [dialog, setDialog] = useState<"apply">();
  const [applyingHold, setApplyingHold] = useState<ApplyHoldCommand>();
  const [uncertainHoldApply, setUncertainHoldApply] = useState<ApplyHoldCommand>();
  const [releasingHold, setReleasingHold] = useState<QueryScopedCommit<LegalHold>>();
  const [localCommittedHoldEvidence, setLocalCommittedHoldEvidence] = useState<CommittedHoldEvidence>();
  const applyHoldInFlight = useRef(false);
  const requestSequence = useRef(0);
  const localCommittedHolds = useRef(new Map<string, QueryScopedCommit<LegalHold>>());
  const committedHolds = commitStore
    ? { current: commitStore.terminal }
    : localCommittedHolds;
  const committedHoldEvidence = commitStore?.evidence ?? localCommittedHoldEvidence;
  const setCommittedHoldEvidence = commitStore?.setEvidence ?? setLocalCommittedHoldEvidence;
  const applyHoldButtonRef = useRef<HTMLButtonElement>(null);
  const applyHoldFirstFieldRef = useRef<HTMLInputElement>(null);
  const cursor = cursorHistory.at(-1);
  const holdQueryKey = JSON.stringify([status, caseId, cursor ?? null]);
  const includesHold = (item: LegalHold) => (
    (!status || item.status === status)
    && (!caseId || item.case_id === caseId)
  );
  const load = async () => {
    const sequence = requestSequence.current + 1;
    requestSequence.current = sequence;
    setLoading(true); setError(undefined);
    try {
      const response = await client.listHolds({ status: status || undefined, case_id: caseId || undefined, cursor, limit: 25 });
      if (sequence !== requestSequence.current) return;
      const result = reconcileCommittedPage(
        response,
        committedHolds.current,
        (item) => item.legal_hold_id,
        includesHold,
        holdQueryKey,
      );
      setPage(result);
      setSelected((current) => current
        ? result.items.find((item) => item.legal_hold_id === current.legal_hold_id)
        : undefined);
    } catch (cause) {
      if (sequence !== requestSequence.current) return;
      setError(cause);
    } finally {
      if (sequence === requestSequence.current) setLoading(false);
    }
  };
  useEffect(() => {
    void load();
    return () => {
      requestSequence.current += 1;
    };
  }, [status, caseId, cursor]);
  function prepareQueryChange() {
    requestSequence.current += 1;
    setLoading(true);
    setPage(undefined);
    setSelected(undefined);
    setError(undefined);
  }
  function resetPaging() {
    prepareQueryChange();
    setCursorHistory([undefined]);
  }
  function acceptAppliedHold(committed: LegalHold) {
    if (committed.status === "released") {
      committedHolds.current.set(committed.legal_hold_id, {
        item: committed,
        queryKey: "reconciled-apply",
      });
    }
    setCommittedHoldEvidence({ kind: "applied", item: committed });
    setUncertainHoldApply(undefined);
    setDialog(undefined);
    notify(t("holdApplied"));
    if (cursor !== undefined || cursorHistory.length > 1) {
      prepareQueryChange();
      setCursorHistory([undefined]);
    } else {
      void load();
    }
  }
  async function applyHold(input: ApplyHoldCommand) {
    if (applyHoldInFlight.current) return;
    const request = { ...input };
    applyHoldInFlight.current = true;
    setApplyingHold(request);
    try {
      const committed = await client.applyHold(request.archiveId, {
        case_id: request.caseId,
        approval_request_id: request.approvalId,
        reason: request.reason,
      });
      if (
        !committed.legal_hold_id
        || committed.status !== "active"
        || committed.archive_id !== request.archiveId
        || committed.case_id !== request.caseId
        || committed.apply_approval_request_id !== request.approvalId
        || committed.reason !== request.reason
      ) throw new Error("invalid_apply_hold_response");

      // Applying proves the write, but not cursor membership.
      acceptAppliedHold(committed);
    } catch (cause) {
      if (isUnknownWriteFailure(cause)) {
        // No legal-hold ID was returned. A case-filtered list can contain
        // similar holds, but is not an exact read target for this mutation.
        setUncertainHoldApply(request);
      }
      throw cause;
    } finally {
      applyHoldInFlight.current = false;
      setApplyingHold(undefined);
    }
  }
  const holdEvidenceLabel = committedHoldEvidence?.kind === "applied"
    ? t("holdApplied")
    : t("holdReleased");
  return (
    <>
      <div className="page-heading"><div><p className="eyebrow">{t("adminCenter")}</p><h1>{t("holds")}</h1></div><Button ref={applyHoldButtonRef} onClick={() => setDialog("apply")}><AddIcon aria-hidden size={17} />{t("applyHold")}</Button></div>
      <div className="toolbar">
        <Field label={t("status")}><Select value={status} onChange={(event) => { setStatus(event.target.value as HoldStatus | ""); resetPaging(); }}><option value="">{t("all")}</option><option value="active">{t("active")}</option><option value="released">{t("released")}</option></Select></Field>
        {caseId ? <div className="filter-chip"><span>{t("filteredByCase")}: <span className="mono">{shortId(caseId)}</span></span><IconButton className="filter-chip__clear" icon={<CloseIcon />} label={t("clearFilter")} onClick={() => { setCaseId(""); clearInitialCase(); resetPaging(); }} /></div> : null}
        <Button variant="quiet" onClick={() => void load()}>{t("refresh")}</Button>
      </div>
      {committedHoldEvidence ? (
        <div className="notice" role="status" aria-label={holdEvidenceLabel}>
          <p><strong>{holdEvidenceLabel}</strong> · {committedHoldEvidence.item.archive_id}</p>
          <code>{committedHoldEvidence.item.legal_hold_id}</code>
          <StatusBadge status={committedHoldEvidence.item.status} t={t} />
        </div>
      ) : null}
      {error ? <AdminErrorNotice error={error} retry={() => void load()} t={t} /> : null}
      {loading ? <Spinner label={t("loading")} /> : null}
      {page?.items.length ? (
        <AdminListDetail
          backLabel={t("backToHolds")}
          detailBody={selected ? (
            <HoldDetailWorkspace
              item={selected}
              locale={locale}
              t={t}
              onOpenCase={openCase ? () => openCase(selected.case_id) : undefined}
              onReleaseHold={() => setReleasingHold({ item: selected, queryKey: holdQueryKey })}
            />
          ) : null}
          detailClassName="case-hold-detail"
          detailEmpty={<EmptyState art={<SelectRecordArt />} title={t("details")} description={t("selectRow")} />}
          detailHeader={<p className="eyebrow">{t("details")}</p>}
          detailTitle={selected ? <span className="mono">{shortId(selected.legal_hold_id)}</span> : t("details")}
          detailTrailing={selected ? <StatusBadge status={selected.status} t={t} /> : null}
          listLabel={t("holds")}
          selectionKey={selected?.legal_hold_id ?? null}
          onBack={() => setSelected(undefined)}
          renderList={(registerTrigger) => (
            <>
              <div className="table-wrap"><table><thead><tr><th>{t("archive")}</th><th>{t("reason")}</th><th>{t("status")}</th><th>{t("createdAt")}</th></tr></thead><tbody>
                {page.items.map((item) => <tr key={item.legal_hold_id} className={selected?.legal_hold_id === item.legal_hold_id ? "is-selected" : ""} onClick={() => setSelected(item)}><td><button ref={registerTrigger(item.legal_hold_id)} aria-current={selected?.legal_hold_id === item.legal_hold_id ? "true" : undefined} className="row-button mono" onClick={() => setSelected(item)}>{shortId(item.archive_id)}</button></td><td>{item.reason}</td><td><StatusBadge status={item.status} t={t} /></td><td>{formatDate(item.created_at, locale)}</td></tr>)}
              </tbody></table></div>
              <Pagination canGoBack={cursorHistory.length > 1} canGoNext={Boolean(page.has_more && page.next_cursor)} onBack={() => { prepareQueryChange(); setCursorHistory((items) => items.slice(0, -1)); }} onNext={() => { if (page.next_cursor) { prepareQueryChange(); setCursorHistory((items) => [...items, page.next_cursor ?? undefined]); } }} t={t} />
            </>
          )}
        />
      ) : page ? <EmptyState art={<EmptyComplianceArt />} title={t("noHolds")} description={t("noHoldsHint")} /> : null}
      {dialog === "apply" ? (
        <Dialog
          busy={Boolean(applyingHold || uncertainHoldApply)}
          closeLabel={t("closeDialog")}
          dismissDisabled={Boolean(applyingHold || uncertainHoldApply)}
          initialFocusRef={applyHoldFirstFieldRef}
          returnFocusRef={applyHoldButtonRef}
          title={t("applyHold")}
          onClose={() => {
            if (!applyingHold && !uncertainHoldApply) setDialog(undefined);
          }}
        >
          {uncertainHoldApply ? (
            <UnknownWriteLock
              aria-label={t("writeOutcomeUnknown")}
              description={t("writeOutcomeExactReadUnavailable")}
              title={t("writeOutcomeUnknown")}
            />
          ) : null}
          <HoldForm
            firstFieldRef={applyHoldFirstFieldRef}
            initialCaseId={caseId || undefined}
            submitting={Boolean(applyingHold || uncertainHoldApply)}
            t={t}
            onClose={() => {
              if (!applyingHold && !uncertainHoldApply) setDialog(undefined);
            }}
            onSubmit={applyHold}
          />
        </Dialog>
      ) : null}
      {releasingHold ? (
        <ConfirmAction
          kind="release-hold"
          client={client}
          item={releasingHold.item}
          locale={locale}
          returnFocusRef={applyHoldButtonRef}
          t={t}
          onClose={() => setReleasingHold(undefined)}
          onDone={(committed) => {
            if (
              committed.legal_hold_id !== releasingHold.item.legal_hold_id
              || committed.status !== "released"
            ) throw new Error("invalid_release_hold_response");
            committedHolds.current.set(committed.legal_hold_id, {
              item: committed,
              queryKey: releasingHold.queryKey,
            });
            setCommittedHoldEvidence({ kind: "released", item: committed });
            setPage((current) => current && reconcileCommittedPage(
              current,
              committedHolds.current,
              (item) => item.legal_hold_id,
              includesHold,
              holdQueryKey,
            ));
            setSelected((current) => {
              const visibleInQuery = releasingHold.queryKey === holdQueryKey
                && includesHold(committed);
              if (visibleInQuery && (!current || current.legal_hold_id === committed.legal_hold_id)) {
                return committed;
              }
              return current?.legal_hold_id === committed.legal_hold_id ? undefined : current;
            });
            setReleasingHold(undefined);
            notify(t("holdReleased"));
            void load();
          }}
        />
      ) : null}
    </>
  );
}
