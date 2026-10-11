import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  Field,
  Input,
  Panel,
  Select,
  Spinner,
  StatusIndicator,
  Textarea,
  ContextBar,
  EvidenceInspector,
  UnknownWriteLock,
} from "@copilotmail/design-system";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { AdminErrorNotice } from "./AdminErrorNotice";
import { AdminListDetail } from "./AdminListDetail";
import { ControlPlaneClient, isUnknownWriteFailure } from "./api";
import { translator } from "./i18n";
import type {
  ApprovalAction,
  ApprovalRequest,
  ApprovalRequestCreateInput,
  ApprovalRequestCreated,
  ApprovalRequestDetail,
  ApprovalStatus,
  ApprovalView as ApprovalViewKind,
  Locale,
  Page,
} from "./types";

type T = ReturnType<typeof translator>;

const APPROVAL_ACTIONS: readonly ApprovalAction[] = [
  "mail.send_as",
  "mail.send_on_behalf_of",
  "compliance.content_review",
  "audit.export",
  "compliance.retention_change",
  "authorization.privilege_elevation",
  "compliance.legal_hold",
  "security.break_glass",
  "mail.bulk_delete",
  "agent.production_mail",
  "agent.first_contact",
];
const APPROVAL_MAX_LIFETIME_MS = 30 * 24 * 60 * 60 * 1_000;
const CONTROL_CHARACTER = /[\u0000-\u001f\u007f-\u009f]/u;

interface ApprovalCreateDraft {
  policyKey: string;
  action: ApprovalAction | "";
  resourceKind: string;
  resourceId: string;
  reason: string;
  ticketReference: string;
  expiresAt: string;
}

interface ApprovalDecisionCommand {
  requestId: string;
  decision: "approved" | "rejected";
  reason: string;
  expectedApprovalCount: number;
  expectedRejectionCount: number;
  requiredApprovals: number;
}

interface ApprovalCancellationCommand {
  requestId: string;
}

interface ApprovalReconciliationEvidence {
  kind: "decision" | "cancellation";
  requestId: string;
  status: ApprovalStatus;
  approvalCount: number;
}

interface CreatedRequestSelectionIntent {
  requestId: string;
  view: "requested";
  status: "all";
  cursor: undefined;
}

function emptyApprovalCreateDraft(): ApprovalCreateDraft {
  return {
    policyKey: "",
    action: "",
    resourceKind: "",
    resourceId: "",
    reason: "",
    ticketReference: "",
    expiresAt: "",
  };
}

function localDateTimeValue(value: Date): string {
  const local = new Date(value.getTime() - value.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function expirationBounds(now = Date.now()): { min: string; max: string } {
  return {
    min: localDateTimeValue(new Date(now + 60_000)),
    max: localDateTimeValue(new Date(now + APPROVAL_MAX_LIFETIME_MS)),
  };
}

function validBoundedText(value: string, maximum: number): boolean {
  return value.length > 0
    && value.trim() === value
    && Array.from(value).length <= maximum
    && !CONTROL_CHARACTER.test(value);
}

function validateApprovalCreateDraft(
  draft: ApprovalCreateDraft,
  now: number,
  t: T,
): string | undefined {
  if (
    !draft.policyKey.trim()
    || !draft.action
    || !draft.resourceKind.trim()
    || !draft.resourceId.trim()
    || !draft.reason.trim()
    || !draft.expiresAt
  ) return t("approvalCreateRequired");
  if (
    draft.policyKey.length > 100
    || !/^[a-z0-9]+(?:[.][a-z0-9]+)*$/u.test(draft.policyKey)
  ) return t("approvalCreatePolicyInvalid");
  if (!APPROVAL_ACTIONS.includes(draft.action as ApprovalAction)) {
    return t("approvalCreateActionInvalid");
  }
  if (
    !validBoundedText(draft.resourceKind, 100)
    || !validBoundedText(draft.resourceId, 500)
    || !validBoundedText(draft.reason, 1_000)
    || (draft.ticketReference !== "" && !validBoundedText(draft.ticketReference, 500))
  ) return t("approvalCreateTextInvalid");
  const expiresAt = Date.parse(draft.expiresAt);
  if (
    !Number.isFinite(expiresAt)
    || expiresAt <= now
    || expiresAt > now + APPROVAL_MAX_LIFETIME_MS
  ) return t("approvalCreateExpirationInvalid");
  return undefined;
}

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

function statusTone(
  status: ApprovalStatus,
): "neutral" | "positive" | "warning" | "critical" {
  if (status === "approved") return "positive";
  if (status === "pending") return "warning";
  if (status === "rejected") return "critical";
  return "neutral";
}

function ApprovalImpactSummary({
  request,
  t,
}: {
  request: ApprovalRequest;
  t: T;
}) {
  return (
    <section aria-labelledby="approval-decision-impact" className="approval-impact-summary">
      <div className="approval-section-heading">
        <div>
          <h3 id="approval-decision-impact">{t("approvalDecisionImpact")}</h3>
          <p>{t("approvalDecisionImpactHint")}</p>
        </div>
        <StatusIndicator label={t(request.status)} tone={statusTone(request.status)} />
      </div>
      <dl className="approval-fact-list approval-impact-facts">
        <dt>{t("approvalAction")}</dt>
        <dd><code>{request.action}</code></dd>
        <dt>{t("progress")}</dt>
        <dd>{request.approval_count}/{request.required_approvals}</dd>
        <dt>{t("reason")}</dt>
        <dd>{request.reason}</dd>
      </dl>
    </section>
  );
}

function ApprovalEvidence({
  detail,
  locale,
  t,
}: {
  detail: ApprovalRequestDetail;
  locale: Locale;
  t: T;
}) {
  const { request, steps } = detail;

  return (
    <EvidenceInspector
      className="approval-evidence-inspector"
      count={steps.length}
      summary={t("approvalEvidence")}
    >
      <section aria-labelledby="approval-evidence-title" className="approval-evidence">
        <h3 id="approval-evidence-title">{t("approvalEvidence")}</h3>
        <p>{t("approvalEvidenceHint")}</p>
        <dl className="approval-fact-list approval-evidence-facts">
          <dt>{t("approvalRequest")}</dt>
          <dd><code>{request.approval_request_id}</code></dd>
          <dt>{t("approvalPolicyVersion")}</dt>
          <dd><code>{request.policy_version_id}</code></dd>
          <dt>{t("createdAt")}</dt>
          <dd>{formatDate(request.created_at, locale)}</dd>
          <dt>{t("expiresAt")}</dt>
          <dd>{formatDate(request.expires_at, locale)}</dd>
          {request.completed_at ? <>
            <dt>{t("approvalCompletedAt")}</dt>
            <dd>{formatDate(request.completed_at, locale)}</dd>
          </> : null}
          {request.cancelled_at ? <>
            <dt>{t("approvalCancelledAt")}</dt>
            <dd>{formatDate(request.cancelled_at, locale)}</dd>
          </> : null}
        </dl>
        {steps.length ? (
          <ol aria-label={t("decisionHistory")} className="approval-evidence-steps">
            {steps.map((step) => (
              <li key={step.approval_step_id}>
                <StatusIndicator
                  label={t(step.decision)}
                  tone={step.decision === "approved" ? "positive" : "critical"}
                />
                <dl className="approval-step-facts">
                  <dt>{t("decisionReason")}</dt><dd>{step.reason}</dd>
                  <dt>{t("approverPrincipal")}</dt><dd><code>{step.approver_principal_id}</code></dd>
                  <dt>{t("approvalStep")}</dt><dd><code>{step.approval_step_id}</code></dd>
                  <dt>{t("decisionRecordedAt")}</dt><dd>{formatDate(step.decided_at, locale)}</dd>
                </dl>
              </li>
            ))}
          </ol>
        ) : null}
      </section>
    </EvidenceInspector>
  );
}

interface ApprovalReconciliationState {
  status: ApprovalStatus;
  approvalCount: number;
  /** A successful local write keeps its evidence visible if a list lags. */
  locallyCommitted: boolean;
  /** One operator decision must never reappear in the actionable inbox. */
  actionableConsumed: boolean;
  conflict?: {
    previousStatus: ApprovalStatus;
    previousApprovalCount: number;
    observedStatus: ApprovalStatus;
    observedApprovalCount: number;
  };
}

function terminalApprovalStatus(status: ApprovalStatus): boolean {
  return status !== "pending";
}

/**
 * Approval state is monotonic: counts only grow and a terminal state never
 * returns to pending (or changes to a different terminal outcome).
 */
function advanceApprovalState(
  current: ApprovalReconciliationState | undefined,
  observed: Pick<ApprovalReconciliationState, "status" | "approvalCount">
    & Partial<Pick<ApprovalReconciliationState, "locallyCommitted" | "actionableConsumed">>,
): ApprovalReconciliationState {
  if (current?.conflict) return current;
  if (current) {
    const temporalApprovalPair = (
      current.status === "approved" && observed.status === "expired"
    ) || (
      current.status === "expired" && observed.status === "approved"
    );
    const contradictoryTerminal = terminalApprovalStatus(current.status)
      && terminalApprovalStatus(observed.status)
      && current.status !== observed.status
      && !temporalApprovalPair;
    const impossibleCountAdvance = terminalApprovalStatus(current.status)
      && observed.approvalCount > current.approvalCount;
    const terminalCountRegression = current.status === "pending"
      && terminalApprovalStatus(observed.status)
      && observed.approvalCount < current.approvalCount;
    if (contradictoryTerminal || impossibleCountAdvance || terminalCountRegression) {
      return {
        ...current,
        conflict: {
          previousStatus: current.status,
          previousApprovalCount: current.approvalCount,
          observedStatus: observed.status,
          observedApprovalCount: observed.approvalCount,
        },
      };
    }
  }
  let status = observed.status;
  if (current && current.status !== "pending") {
    if (current.status === "approved" && observed.status === "expired") {
      status = "expired";
    } else {
      status = current.status;
    }
  }
  return {
    status,
    approvalCount: Math.max(current?.approvalCount ?? 0, observed.approvalCount),
    locallyCommitted: Boolean(
      current?.locallyCommitted || observed.locallyCommitted,
    ),
    actionableConsumed: Boolean(
      current?.actionableConsumed || observed.actionableConsumed,
    ),
  };
}

export default function ApprovalView({
  client,
  locale,
  t,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: T;
}) {
  const [view, setView] = useState<ApprovalViewKind>("actionable");
  const [status, setStatus] = useState<ApprovalStatus | "all">("all");
  const [cursors, setCursors] = useState<Array<string | undefined>>([undefined]);
  const [page, setPage] = useState<Page<ApprovalRequest>>();
  const [selectedId, setSelectedId] = useState<string>();
  const [detail, setDetail] = useState<ApprovalRequestDetail>();
  const [decisionReason, setDecisionReason] = useState("");
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [acting, setActing] = useState(false);
  const [listError, setListError] = useState<unknown>();
  const [detailError, setDetailError] = useState<unknown>();
  const [actionError, setActionError] = useState<unknown>();
  const [createOpen, setCreateOpen] = useState(false);
  const [createDraft, setCreateDraft] = useState<ApprovalCreateDraft>(
    emptyApprovalCreateDraft,
  );
  const [createWindow, setCreateWindow] = useState(expirationBounds);
  const [creating, setCreating] = useState(false);
  const [createValidationError, setCreateValidationError] = useState<string>();
  const [createError, setCreateError] = useState<unknown>();
  const [uncertainCreate, setUncertainCreate] = useState<ApprovalRequestCreateInput>();
  const [uncertainDecision, setUncertainDecision] = useState<ApprovalDecisionCommand>();
  const [uncertainCancellation, setUncertainCancellation] = useState<
    ApprovalCancellationCommand
  >();
  const [reconcilingUnknownWrite, setReconcilingUnknownWrite] = useState(false);
  const [reconciliationEvidence, setReconciliationEvidence] = useState<
    ApprovalReconciliationEvidence
  >();
  const [reconciliationError, setReconciliationError] = useState<unknown>();
  const [createdRequest, setCreatedRequest] = useState<ApprovalRequestCreated>();
  const [pendingCancellation, setPendingCancellation] = useState<ApprovalRequest>();
  const [locallyDecidedIds, setLocallyDecidedIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [focusDetailToken, setFocusDetailToken] = useState(0);
  const [listReloadToken, setListReloadToken] = useState(0);
  const listRequestSequence = useRef(0);
  const detailRequestSequence = useRef(0);
  const actionRequestSequence = useRef(0);
  const createRequestSequence = useRef(0);
  const selectedIdRef = useRef<string | undefined>(undefined);
  const createdRequestToSelectRef = useRef<
    CreatedRequestSelectionIntent | undefined
  >(undefined);
  const createTriggerRef = useRef<HTMLButtonElement>(null);
  const createFirstFieldRef = useRef<HTMLInputElement>(null);
  const createdRequestNoticeRef = useRef<HTMLDivElement>(null);
  const returnCreateFocusRef = useRef(false);
  const cancellationInFlightRef = useRef(false);
  const createInFlightRef = useRef(false);
  const decisionInFlightRef = useRef(false);
  const interactionLockedRef = useRef(false);
  const reconciliationStatesRef = useRef(new Map<
    string,
    ApprovalReconciliationState
  >());
  const latestLoadRef = useRef<(preservePage?: boolean) => Promise<void>>(
    async () => undefined,
  );
  const approvalDetailTitleRef = useRef<HTMLHeadingElement>(null);
  const focusedDetailTokenRef = useRef(0);

  const currentCursor = cursors.at(-1);
  const uncertainAction = uncertainDecision ?? uncertainCancellation;
  const reconciliationConflict = Array.from(reconciliationStatesRef.current.entries())
    .find(([, state]) => state.conflict !== undefined);
  const interactionLocked = Boolean(
    uncertainCreate || uncertainAction || reconciliationConflict,
  );
  const recordApprovalState = useCallback((
    requestId: string,
    observed: Pick<ApprovalReconciliationState, "status" | "approvalCount">
      & Partial<Pick<ApprovalReconciliationState, "locallyCommitted" | "actionableConsumed">>,
  ): ApprovalReconciliationState => {
    const next = advanceApprovalState(
      reconciliationStatesRef.current.get(requestId),
      observed,
    );
    if (next.conflict) interactionLockedRef.current = true;
    reconciliationStatesRef.current.set(requestId, next);
    return next;
  }, []);
  const reconcileApprovalRequest = useCallback((
    request: ApprovalRequest,
  ): ApprovalRequest => {
    const state = recordApprovalState(request.approval_request_id, {
      status: request.status,
      approvalCount: request.approval_count,
    });
    return {
      ...request,
      status: state.status,
      approval_count: state.approvalCount,
    };
  }, [recordApprovalState]);
  const reconcileApprovalDetail = useCallback((
    value: ApprovalRequestDetail,
  ): ApprovalRequestDetail => ({
    ...value,
    request: reconcileApprovalRequest(value.request),
  }), [reconcileApprovalRequest]);
  const reconcilePageItems = useCallback((
    items: ApprovalRequest[],
  ): ApprovalRequest[] => items
    .map(reconcileApprovalRequest)
    .filter((item) => {
      if (view === "actionable") {
        const state = reconciliationStatesRef.current.get(
          item.approval_request_id,
        );
        return item.status === "pending" && !state?.actionableConsumed;
      }
      return status === "all" || item.status === status;
    }), [reconcileApprovalRequest, status, view]);
  const clearSelection = useCallback(() => {
    detailRequestSequence.current += 1;
    actionRequestSequence.current += 1;
    selectedIdRef.current = undefined;
    setSelectedId(undefined);
    setDetail(undefined);
    setDetailError(undefined);
    setActionError(undefined);
    setDetailLoading(false);
    setActing(false);
    setDecisionReason("");
    setPendingCancellation(undefined);
  }, []);
  const selectRequest = useCallback(async (requestId: string) => {
    if (interactionLockedRef.current) return;
    const sequence = detailRequestSequence.current + 1;
    detailRequestSequence.current = sequence;
    actionRequestSequence.current += 1;
    selectedIdRef.current = requestId;
    setSelectedId(requestId);
    setDetail(undefined);
    setDecisionReason("");
    setDetailError(undefined);
    setActionError(undefined);
    setDetailLoading(true);
    setActing(false);
    try {
      const result = reconcileApprovalDetail(
        await client.getApprovalRequest(requestId),
      );
      if (
        sequence === detailRequestSequence.current
        && selectedIdRef.current === requestId
      ) {
        setDetail(result);
        setPage((current) => current ? {
          ...current,
          items: reconcilePageItems(current.items),
        } : current);
      }
    } catch (cause) {
      if (
        sequence === detailRequestSequence.current
        && selectedIdRef.current === requestId
      ) setDetailError(cause);
    } finally {
      if (
        sequence === detailRequestSequence.current
        && selectedIdRef.current === requestId
      ) setDetailLoading(false);
    }
  }, [client, reconcileApprovalDetail, reconcilePageItems]);
  const load = useCallback(async (preservePage = true) => {
    const sequence = listRequestSequence.current + 1;
    listRequestSequence.current = sequence;
    if (!preservePage) {
      setPage(undefined);
      clearSelection();
    }
    setLoading(true);
    setListError(undefined);
    try {
      const result = await client.listApprovalRequests({
        view,
        status: view === "requested" && status !== "all" ? status : undefined,
        cursor: currentCursor,
        limit: 50,
      });
      if (sequence !== listRequestSequence.current) return;
      const reconciledItems = reconcilePageItems(result.items);
      const reconciledResult = { ...result, items: reconciledItems };
      setPage(reconciledResult);
      setDetail((current) => current
        ? reconcileApprovalDetail(current)
        : current);
      const createdSelection = createdRequestToSelectRef.current;
      if (
        createdSelection
        && view === createdSelection.view
        && status === createdSelection.status
        && currentCursor === createdSelection.cursor
      ) {
        createdRequestToSelectRef.current = undefined;
        void selectRequest(createdSelection.requestId);
        return;
      }
      const currentSelection = selectedIdRef.current;
      if (
        currentSelection
        && !reconciledItems.some((item) => item.approval_request_id === currentSelection)
        && !reconciliationStatesRef.current.get(currentSelection)?.locallyCommitted
      ) {
        clearSelection();
      }
    } catch (cause) {
      if (sequence === listRequestSequence.current) setListError(cause);
    } finally {
      if (sequence === listRequestSequence.current) setLoading(false);
    }
  }, [
    clearSelection,
    client,
    currentCursor,
    reconcileApprovalDetail,
    reconcilePageItems,
    selectRequest,
    status,
    view,
  ]);
  latestLoadRef.current = load;

  useEffect(() => {
    void load(false);
    return () => {
      listRequestSequence.current += 1;
    };
  }, [listReloadToken, load]);

  useEffect(() => {
    if (createOpen) {
      createFirstFieldRef.current?.focus();
    } else if (returnCreateFocusRef.current) {
      returnCreateFocusRef.current = false;
      createTriggerRef.current?.focus();
    }
  }, [createOpen]);

  useEffect(() => {
    if (createdRequest) createdRequestNoticeRef.current?.focus();
  }, [createdRequest]);

  useLayoutEffect(() => {
    if (
      !pendingCancellation
      && focusDetailToken !== focusedDetailTokenRef.current
    ) {
      const detailTitle = approvalDetailTitleRef.current;
      if (detailTitle) {
        detailTitle.focus();
        focusedDetailTokenRef.current = focusDetailToken;
      }
    }
  }, [focusDetailToken, pendingCancellation]);

  useEffect(() => () => {
    createRequestSequence.current += 1;
    detailRequestSequence.current += 1;
    actionRequestSequence.current += 1;
  }, []);

  function openCreateForm() {
    if (interactionLockedRef.current) return;
    setCreateWindow(expirationBounds());
    setCreateValidationError(undefined);
    setCreateError(undefined);
    setCreateOpen(true);
  }

  function closeCreateForm() {
    if (creating || interactionLockedRef.current) return;
    returnCreateFocusRef.current = true;
    setCreateOpen(false);
    setCreateDraft(emptyApprovalCreateDraft());
    setCreateValidationError(undefined);
    setCreateError(undefined);
  }

  function updateCreateDraft(patch: Partial<ApprovalCreateDraft>) {
    if (interactionLockedRef.current) return;
    setCreateDraft((value) => ({ ...value, ...patch }));
    setCreateValidationError(undefined);
    setCreateError(undefined);
  }

  async function executeCreateApprovalRequest(
    command: ApprovalRequestCreateInput,
  ) {
    if (createInFlightRef.current) return;
    createInFlightRef.current = true;
    interactionLockedRef.current = true;
    const sequence = createRequestSequence.current + 1;
    createRequestSequence.current = sequence;
    setCreating(true);
    setCreateValidationError(undefined);
    setCreateError(undefined);
    try {
      const result = await client.createApprovalRequest(command);
      if (sequence !== createRequestSequence.current) return;

      interactionLockedRef.current = false;
      setUncertainCreate(undefined);
      createdRequestToSelectRef.current = {
        requestId: result.approval_request_id,
        view: "requested",
        status: "all",
        cursor: undefined,
      };
      setCreatedRequest(result);
      setCreateOpen(false);
      setCreateDraft(emptyApprovalCreateDraft());
      listRequestSequence.current += 1;
      setView("requested");
      setStatus("all");
      setCursors([undefined]);
      setPage(undefined);
      clearSelection();
      setListReloadToken((value) => value + 1);
    } catch (cause) {
      if (sequence === createRequestSequence.current) {
        setCreateError(cause);
        if (isUnknownWriteFailure(cause)) {
          setUncertainCreate(command);
          setReconciliationEvidence(undefined);
          setReconciliationError(undefined);
        } else {
          interactionLockedRef.current = false;
        }
      }
    } finally {
      createInFlightRef.current = false;
      if (sequence === createRequestSequence.current) setCreating(false);
    }
  }

  async function submitApprovalRequest() {
    if (creating || interactionLockedRef.current) return;
    const now = Date.now();
    const validationError = validateApprovalCreateDraft(createDraft, now, t);
    if (validationError) {
      setCreateValidationError(validationError);
      setCreateError(undefined);
      return;
    }
    if (!createDraft.action) return;

    await executeCreateApprovalRequest({
      policy_key: createDraft.policyKey,
      action: createDraft.action,
      resource_kind: createDraft.resourceKind,
      resource_id: createDraft.resourceId,
      reason: createDraft.reason,
      ticket_reference: createDraft.ticketReference || null,
      expires_at: new Date(createDraft.expiresAt).toISOString(),
    });
  }

  async function executeDecision(
    command: ApprovalDecisionCommand,
  ) {
    if (decisionInFlightRef.current) return;
    decisionInFlightRef.current = true;
    interactionLockedRef.current = true;
    const { requestId, decision, reason } = command;
    const sequence = actionRequestSequence.current + 1;
    actionRequestSequence.current = sequence;
    setActing(true);
    setActionError(undefined);
    try {
      const committed = await client.decideApprovalRequest(
        requestId,
        decision,
        reason,
        {
          approval_count: command.expectedApprovalCount,
          rejection_count: command.expectedRejectionCount,
          required_approvals: command.requiredApprovals,
        },
      );
      const committedState = recordApprovalState(requestId, {
        status: committed.status,
        approvalCount: committed.approval_count,
        locallyCommitted: true,
        actionableConsumed: true,
      });
      if (
        sequence !== actionRequestSequence.current
        || selectedIdRef.current !== requestId
      ) return;
      if (!committedState.conflict) interactionLockedRef.current = false;
      setUncertainDecision(undefined);
      setLocallyDecidedIds((current) => {
        const next = new Set(current);
        next.add(requestId);
        return next;
      });
      setPage((current) => current ? {
        ...current,
        items: reconcilePageItems(current.items),
      } : current);
      setDetail((current) => current
        && current.request.approval_request_id === requestId
        ? {
          ...current,
          request: {
            ...current.request,
            status: committedState.status,
            approval_count: committedState.approvalCount,
          },
        }
        : current);
      setDecisionReason("");
      setFocusDetailToken((value) => value + 1);
      try {
        const refreshed = reconcileApprovalDetail(
          await client.getApprovalRequest(requestId),
        );
        if (
          sequence === actionRequestSequence.current
          && selectedIdRef.current === requestId
        ) {
          setDetail(refreshed);
          setPage((current) => current ? {
            ...current,
            items: reconcilePageItems(current.items),
          } : current);
        }
      } catch (cause) {
        if (
          sequence === actionRequestSequence.current
          && selectedIdRef.current === requestId
        ) setActionError(cause);
      }
      if (
        sequence === actionRequestSequence.current
        && selectedIdRef.current === requestId
      ) await latestLoadRef.current();
    } catch (cause) {
      if (
        sequence === actionRequestSequence.current
        && selectedIdRef.current === requestId
      ) {
        setActionError(cause);
        if (isUnknownWriteFailure(cause)) {
          setUncertainDecision(command);
          setReconciliationEvidence(undefined);
          setReconciliationError(undefined);
        } else {
          interactionLockedRef.current = false;
        }
      }
    } finally {
      decisionInFlightRef.current = false;
      if (sequence === actionRequestSequence.current) setActing(false);
    }
  }

  async function decide(decision: "approved" | "rejected") {
    if (!detail || !decisionReason.trim() || interactionLockedRef.current) return;
    await executeDecision({
      requestId: detail.request.approval_request_id,
      decision,
      reason: decisionReason.trim(),
      expectedApprovalCount: detail.request.approval_count,
      expectedRejectionCount: detail.request.rejection_count,
      requiredApprovals: detail.request.required_approvals,
    });
  }

  async function executeCancellation(
    command: ApprovalCancellationCommand,
  ) {
    if (cancellationInFlightRef.current) return;
    const { requestId } = command;
    if (selectedIdRef.current !== requestId) {
      setPendingCancellation(undefined);
      return;
    }
    cancellationInFlightRef.current = true;
    interactionLockedRef.current = true;
    const sequence = actionRequestSequence.current + 1;
    actionRequestSequence.current = sequence;
    setActing(true);
    setActionError(undefined);
    try {
      const committed = await client.cancelApprovalRequest(requestId);
      const committedState = recordApprovalState(requestId, {
        status: committed.status,
        approvalCount: committed.approval_count,
        locallyCommitted: true,
      });
      if (
        sequence !== actionRequestSequence.current
        || selectedIdRef.current !== requestId
      ) return;
      if (!committedState.conflict) interactionLockedRef.current = false;
      setUncertainCancellation(undefined);
      setDetail((current) => current
        && current.request.approval_request_id === requestId
        ? {
          ...current,
          request: {
            ...current.request,
            status: committedState.status,
            approval_count: committedState.approvalCount,
          },
        }
        : current);
      setPage((current) => current ? {
        ...current,
        items: reconcilePageItems(current.items),
      } : current);
      setFocusDetailToken((value) => value + 1);
      try {
        const refreshed = reconcileApprovalDetail(
          await client.getApprovalRequest(requestId),
        );
        if (
          sequence === actionRequestSequence.current
          && selectedIdRef.current === requestId
        ) {
          setDetail(refreshed);
          setPage((current) => current ? {
            ...current,
            items: reconcilePageItems(current.items),
          } : current);
        }
      } catch (cause) {
        if (
          sequence === actionRequestSequence.current
          && selectedIdRef.current === requestId
        ) setActionError(cause);
      }
      if (
        sequence === actionRequestSequence.current
        && selectedIdRef.current === requestId
      ) await latestLoadRef.current();
    } catch (cause) {
      if (
        sequence === actionRequestSequence.current
        && selectedIdRef.current === requestId
      ) {
        setActionError(cause);
        if (isUnknownWriteFailure(cause)) {
          setUncertainCancellation(command);
          setReconciliationEvidence(undefined);
          setReconciliationError(undefined);
        } else {
          interactionLockedRef.current = false;
        }
      }
    } finally {
      cancellationInFlightRef.current = false;
      if (sequence === actionRequestSequence.current) {
        setActing(false);
        setPendingCancellation(undefined);
      }
    }
  }

  async function cancelRequest() {
    const request = pendingCancellation;
    if (!request || interactionLockedRef.current) return;
    await executeCancellation({ requestId: request.approval_request_id });
  }

  async function reconcileUnknownWrite() {
    if (
      reconcilingUnknownWrite
      || (!uncertainDecision && !uncertainCancellation)
    ) return;
    setReconcilingUnknownWrite(true);
    setReconciliationError(undefined);
    try {
      const command = uncertainDecision ?? uncertainCancellation;
      if (!command) return;
      const latest = reconcileApprovalDetail(
        await client.getApprovalRequest(command.requestId),
      );
      if (selectedIdRef.current === command.requestId) setDetail(latest);
      setPage((current) => current ? {
        ...current,
        items: reconcilePageItems(current.items),
      } : current);
      setReconciliationEvidence({
        kind: uncertainDecision ? "decision" : "cancellation",
        requestId: command.requestId,
        status: latest.request.status,
        approvalCount: latest.request.approval_count,
      });
    } catch (cause) {
      setReconciliationError(cause);
    } finally {
      setReconcilingUnknownWrite(false);
    }
  }

  function changeView(next: ApprovalViewKind) {
    if (interactionLockedRef.current) return;
    createdRequestToSelectRef.current = undefined;
    listRequestSequence.current += 1;
    setView(next);
    setStatus("all");
    setCursors([undefined]);
    setPage(undefined);
    clearSelection();
  }

  function changeStatus(next: ApprovalStatus | "all") {
    if (interactionLockedRef.current) return;
    createdRequestToSelectRef.current = undefined;
    listRequestSequence.current += 1;
    setStatus(next);
    setCursors([undefined]);
    setPage(undefined);
    clearSelection();
  }

  function showPreviousPage() {
    if (interactionLockedRef.current) return;
    createdRequestToSelectRef.current = undefined;
    listRequestSequence.current += 1;
    setPage(undefined);
    clearSelection();
    setCursors((value) => value.slice(0, -1));
  }

  function showNextPage() {
    if (interactionLockedRef.current || !page?.next_cursor) return;
    createdRequestToSelectRef.current = undefined;
    listRequestSequence.current += 1;
    setPage(undefined);
    clearSelection();
    setCursors((value) => [...value, page.next_cursor!]);
  }

  const selectedRequest = detail?.request;
  const approvalContextFacts = [
    {
      id: "work-view",
      label: t("approvalView"),
      value: view === "actionable" ? t("actionableApprovals") : t("myRequests"),
    },
    {
      id: "status-filter",
      label: t("status"),
      value: view === "actionable" ? t("pending") : t(status),
    },
    ...(selectedRequest ? [
      {
        id: "resource",
        label: t("resource"),
        value: (
          <span className="approval-context-resource">
            <span>{selectedRequest.resource_kind}</span>
            <code>{selectedRequest.resource_id}</code>
          </span>
        ),
      },
      {
        id: "requested-by",
        label: t("requestedBy"),
        value: <code>{selectedRequest.requested_by}</code>,
      },
    ] : []),
  ];

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1>{t("approvalTitle")}</h1>
          <p>{t("approvalCenterHint")}</p>
        </div>
        <div className="approval-heading-actions">
          <Button
            disabled={creating || acting || interactionLocked}
            variant="secondary"
            onClick={() => {
              if (!interactionLockedRef.current) void load();
            }}
          >
            {t("refresh")}
          </Button>
        </div>
      </div>

      <ContextBar
        actions={(
          <button
            aria-controls="approval-create-panel"
            aria-expanded={createOpen}
            className="button button--primary"
            disabled={createOpen || creating || acting || interactionLocked}
            onClick={openCreateForm}
            ref={createTriggerRef}
            type="button"
          >
            {t("newApprovalRequest")}
          </button>
        )}
        className="approval-context"
        facts={approvalContextFacts}
        status={selectedRequest ? (
          <StatusIndicator
            label={t(selectedRequest.status)}
            tone={statusTone(selectedRequest.status)}
          />
        ) : undefined}
        title={t("approvalWorkspaceContext")}
      />

      {createdRequest ? (
        <div
          aria-label={t("approvalRequestCreated")}
          className="notice approval-create-success"
          ref={createdRequestNoticeRef}
          role="status"
          tabIndex={-1}
        >
          <div>
            <strong>{t("approvalRequestCreated")}</strong>
            <p>{t("approvalRequestCreatedHint")}</p>
            <code>{createdRequest.approval_request_id}</code>
            <Badge tone={statusTone(createdRequest.status)}>
              {t(createdRequest.status)}
            </Badge>
          </div>
          <Button
            onClick={() => setCreatedRequest(undefined)}
            type="button"
            variant="quiet"
          >
            {t("dismissCreatedApproval")}
          </Button>
        </div>
      ) : null}

      {createOpen ? (
        <Panel
          className="approval-create-panel"
          id="approval-create-panel"
          title={t("createApprovalRequest")}
        >
          <p className="approval-create-intro">{t("createApprovalRequestHint")}</p>
          <form
            className="approval-create-form"
            onKeyDown={(event) => {
              if (event.key === "Escape" && !creating && !interactionLocked) {
                event.preventDefault();
                closeCreateForm();
              }
            }}
            onSubmit={(event) => {
              event.preventDefault();
              void submitApprovalRequest();
            }}
          >
            {createValidationError ? (
              <div className="notice notice--error approval-create-form__wide" role="alert">
                <p>{createValidationError}</p>
              </div>
            ) : null}
            {createError ? (
              <div className="approval-create-form__wide">
                <AdminErrorNotice error={createError} t={t} />
              </div>
            ) : null}
            {uncertainCreate ? (
              <UnknownWriteLock
                aria-label={t("approvalWriteOutcomeUnknown")}
                className="approval-create-form__wide"
                description={t("approvalCreateReconciliationUnresolved")}
                evidence={(
                  <>
                  <p><span>{t("action")}:</span> <code>{uncertainCreate.action}</code></p>
                  <p><span>{t("resource")}:</span> <code>{uncertainCreate.resource_id}</code></p>
                  </>
                )}
                title={t("approvalWriteOutcomeUnknown")}
              />
            ) : null}
            <Field label={t("policyKey")} hint={t("policyKeyServerHint")}>
              <Input
                autoComplete="off"
                disabled={creating || interactionLocked}
                maxLength={100}
                onChange={(event) => updateCreateDraft({
                  policyKey: event.target.value,
                })}
                pattern="[a-z0-9]+([.][a-z0-9]+)*"
                ref={createFirstFieldRef}
                required
                value={createDraft.policyKey}
              />
            </Field>
            <Field label={t("approvalAction")}>
              <Select
                disabled={creating || interactionLocked}
                onChange={(event) => updateCreateDraft({
                  action: event.target.value as ApprovalAction | "",
                })}
                required
                value={createDraft.action}
              >
                <option disabled value="">{t("chooseApprovalAction")}</option>
                {APPROVAL_ACTIONS.map((action) => (
                  <option key={action} value={action}>{action}</option>
                ))}
              </Select>
            </Field>
            <Field
              label={t("resourceKind")}
              hint={t("approvalResourceKindHint")}
            >
              <Input
                disabled={creating || interactionLocked}
                maxLength={100}
                onChange={(event) => updateCreateDraft({
                  resourceKind: event.target.value,
                })}
                required
                value={createDraft.resourceKind}
              />
            </Field>
            <Field
              label={t("resourceId")}
              hint={t("approvalResourceIdHint")}
            >
              <Input
                disabled={creating || interactionLocked}
                maxLength={500}
                onChange={(event) => updateCreateDraft({
                  resourceId: event.target.value,
                })}
                required
                value={createDraft.resourceId}
              />
            </Field>
            <div className="approval-create-form__wide">
              <Field label={t("reason")} hint={t("approvalReasonHint")}>
                <Textarea
                  className="control approval-create-reason"
                  disabled={creating || interactionLocked}
                  maxLength={1000}
                  onChange={(event) => updateCreateDraft({
                    reason: event.target.value,
                  })}
                  required
                  value={createDraft.reason}
                />
              </Field>
            </div>
            <Field label={t("ticketReferenceOptional")}>
              <Input
                disabled={creating || interactionLocked}
                maxLength={500}
                onChange={(event) => updateCreateDraft({
                  ticketReference: event.target.value,
                })}
                value={createDraft.ticketReference}
              />
            </Field>
            <Field label={t("expiresAt")} hint={t("approvalExpirationHint")}>
              <Input
                disabled={creating || interactionLocked}
                max={createWindow.max}
                min={createWindow.min}
                onChange={(event) => updateCreateDraft({
                  expiresAt: event.target.value,
                })}
                required
                type="datetime-local"
                value={createDraft.expiresAt}
              />
            </Field>
            <div className="approval-create-form__actions approval-create-form__wide">
              <Button
                disabled={creating || interactionLocked}
                onClick={closeCreateForm}
                type="button"
                variant="secondary"
              >
                {t("cancel")}
              </Button>
              <Button disabled={creating || interactionLocked} type="submit">
                {creating ? t("loading") : t("submitApprovalRequest")}
              </Button>
            </div>
          </form>
        </Panel>
      ) : null}

      <div className="toolbar">
        <Field label={t("approvalView")}>
          <Select
            disabled={interactionLocked}
            value={view}
            onChange={(event) => changeView(event.target.value as ApprovalViewKind)}
          >
            <option value="actionable">{t("actionableApprovals")}</option>
            <option value="requested">{t("myRequests")}</option>
          </Select>
        </Field>
        {view === "requested" ? (
          <Field label={t("status")}>
            <Select
              disabled={interactionLocked}
              value={status}
              onChange={(event) => changeStatus(
                event.target.value as ApprovalStatus | "all",
              )}
            >
              {(["all", "pending", "approved", "rejected", "expired", "cancelled"] as const)
                .map((item) => <option value={item} key={item}>{t(item)}</option>)}
            </Select>
          </Field>
        ) : null}
      </div>

      {reconciliationConflict ? (
        <div className="notice notice--error" role="alert">
          <div>
            <strong>{t("approvalStateConflict")}</strong>
            <p><code>{reconciliationConflict[0]}</code></p>
            <p>
              {t(reconciliationConflict[1].conflict!.previousStatus)}
              {" "}
              {reconciliationConflict[1].conflict!.previousApprovalCount}
              {" → "}
              {t(reconciliationConflict[1].conflict!.observedStatus)}
              {" "}
              {reconciliationConflict[1].conflict!.observedApprovalCount}
            </p>
          </div>
        </div>
      ) : null}
      {uncertainAction ? (
        <UnknownWriteLock
          aria-label={t("approvalWriteOutcomeUnknown")}
          description={t("approvalWriteOutcomeUnknown")}
          evidence={(
            <>
            <p><span>{t("approvalRequest")}:</span> <code>{uncertainAction.requestId}</code></p>
            {uncertainDecision ? (
              <>
                <p><span>{t("action")}:</span> <code>{uncertainDecision.decision}</code></p>
                <p><span>{t("reason")}:</span> {uncertainDecision.reason}</p>
              </>
            ) : null}
            </>
          )}
          reconciliationAction={(
            <Button
              disabled={reconcilingUnknownWrite}
              onClick={() => void reconcileUnknownWrite()}
              type="button"
              variant="secondary"
            >
              {reconcilingUnknownWrite ? t("loading") : t("reconcileApprovalState")}
            </Button>
          )}
          title={t("approvalWriteOutcomeUnknown")}
        />
      ) : null}
      {reconciliationError ? (
        <AdminErrorNotice error={reconciliationError} t={t} />
      ) : null}
      {reconciliationEvidence ? (
        <div className="notice" role="status">
          <div>
            <strong>{t("approvalReconciliationReadOnly")}</strong>
            <p>
              <code>{reconciliationEvidence.requestId}</code>
              {" · "}
              {t(reconciliationEvidence.status)}
              {" · "}
              {reconciliationEvidence.approvalCount}
            </p>
          </div>
        </div>
      ) : null}

      {listError ? (
        <AdminErrorNotice error={listError} retry={() => void load()} t={t} />
      ) : null}
      {loading ? <Spinner label={t("loading")} /> : null}
      {page ? (
        <AdminListDetail
          backLabel={t("backToApprovals")}
          className="approval-layout"
          detailBody={!selectedId ? null : detailLoading ? (
            <Spinner label={t("loading")} />
          ) : detailError ? (
            <AdminErrorNotice
              error={detailError}
              retry={() => void selectRequest(selectedId)}
              t={t}
            />
          ) : detail ? (
            <article className="approval-decision-workspace">
              {actionError ? <AdminErrorNotice error={actionError} t={t} /> : null}
              <ApprovalImpactSummary request={detail.request} t={t} />
              <section
                aria-labelledby="approval-decision-boundary"
                className="approval-decision-boundary"
              >
                <div className="approval-section-heading">
                  <div>
                    <h3 id="approval-decision-boundary">{t("approvalDecisionBoundary")}</h3>
                    <p>{view === "actionable"
                      ? t("actionableApprovalsHint")
                      : t("myRequestsHint")}
                    </p>
                  </div>
                </div>
                <dl className="approval-fact-list approval-boundary-facts">
                  <dt>{t("resource")}</dt>
                  <dd className="approval-resource-value">
                    <span>{detail.request.resource_kind}</span>
                    <code>{detail.request.resource_id}</code>
                  </dd>
                  <dt>{t("requestedBy")}</dt>
                  <dd><code>{detail.request.requested_by}</code></dd>
                  <dt>{t("ticket")}</dt>
                  <dd>{detail.request.ticket_reference ?? "—"}</dd>
                  <dt>{t("expiresAt")}</dt>
                  <dd>{formatDate(detail.request.expires_at, locale)}</dd>
                </dl>
                {detail.request.status === "pending"
                  && view === "actionable"
                  && locallyDecidedIds.has(detail.request.approval_request_id) ? (
                    <div className="notice" role="status">
                      <div>
                        <strong>{t("approvalDecisionRecorded")}</strong>
                        <p>{t("approvalDecisionRecordedHint")}</p>
                      </div>
                    </div>
                  ) : null}
                {detail.request.status === "pending"
                  && view === "actionable"
                  && !locallyDecidedIds.has(detail.request.approval_request_id) ? (
                    <div className="detail-actions approval-decision">
                      <h4>{t("approvalRecordDecision")}</h4>
                      <Field label={t("decisionReason")}>
                        <Textarea
                          disabled={acting || interactionLocked}
                          value={decisionReason}
                          maxLength={1000}
                          onChange={(event) => setDecisionReason(event.target.value)}
                        />
                      </Field>
                      <div>
                        <Button disabled={acting || interactionLocked || !decisionReason.trim()} onClick={() => void decide("approved")}>{t("approve")}</Button>
                        <Button variant="danger" disabled={acting || interactionLocked || !decisionReason.trim()} onClick={() => void decide("rejected")}>{t("reject")}</Button>
                      </div>
                    </div>
                  ) : null}
                {detail.request.status === "pending" && view === "requested" ? (
                  <div className="detail-actions approval-cancellation-action">
                    <h4>{t("approvalRequestAction")}</h4>
                    <Button
                      variant="danger"
                      disabled={acting || interactionLocked}
                      onClick={() => {
                        if (!interactionLockedRef.current) setPendingCancellation(detail.request);
                      }}
                    >
                      {t("cancelApproval")}
                    </Button>
                  </div>
                ) : null}
                {detail.request.status !== "pending" ? (
                  <p className="approval-no-action">{t("approvalNoAction")}</p>
                ) : null}
              </section>
              <ApprovalEvidence detail={detail} locale={locale} t={t} />
            </article>
          ) : <Spinner label={t("loading")} />}
          detailClassName="approval-detail"
          detailEmpty={<EmptyState title={t("details")} description={t("selectRow")} />}
          detailHeader={<p className="eyebrow">{t("approvalRequest")}</p>}
          detailTitle={selectedId ? shortId(selectedId) : t("details")}
          detailTitleRef={approvalDetailTitleRef}
          detailTrailing={detail ? (
            <Badge tone={statusTone(detail.request.status)}>{t(detail.request.status)}</Badge>
          ) : null}
          listLabel={t("approvalTitle")}
          navigationLocked={acting || interactionLocked}
          renderList={(registerTrigger) => (
            <>
              {!page.items.length ? (
                <EmptyState title={t("noApprovals")} description={t("noApprovalsHint")} />
              ) : (
                <div className="table-wrap">
                  <table>
                    <thead><tr><th>{t("action")}</th><th>{t("resourceType")}</th><th>{t("progress")}</th><th>{t("expiresAt")}</th><th>{t("status")}</th></tr></thead>
                    <tbody>
                      {page.items.map((item) => (
                        <tr
                          key={item.approval_request_id}
                          className={selectedId === item.approval_request_id ? "is-selected" : ""}
                          onClick={() => void selectRequest(item.approval_request_id)}
                        >
                          <td>
                            <button
                              ref={registerTrigger(item.approval_request_id)}
                              aria-current={selectedId === item.approval_request_id ? "true" : undefined}
                              className="row-button"
                              disabled={interactionLocked}
                              onClick={(event) => {
                                event.stopPropagation();
                                void selectRequest(item.approval_request_id);
                              }}
                            >
                              {item.action}
                            </button>
                            <span className="table-subline mono">{shortId(item.approval_request_id)}</span>
                          </td>
                          <td>{item.resource_kind}<span className="table-subline mono">{shortId(item.resource_id)}</span></td>
                          <td>{item.approval_count}/{item.required_approvals}</td>
                          <td>{formatDate(item.expires_at, locale)}</td>
                          <td><Badge tone={statusTone(item.status)}>{t(item.status)}</Badge></td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
              {(cursors.length > 1 || page.next_cursor) ? (
                <nav className="pagination" aria-label={t("pagination")}>
                  <Button disabled={interactionLocked || cursors.length === 1} onClick={showPreviousPage} variant="secondary">{t("previous")}</Button>
                  <Button disabled={interactionLocked || !page.next_cursor} onClick={showNextPage} variant="secondary">{t("next")}</Button>
                </nav>
              ) : null}
            </>
          )}
          selectionKey={selectedId ?? null}
          onBack={() => {
            if (!interactionLockedRef.current) clearSelection();
          }}
        />
      ) : null}
      {pendingCancellation ? (
        <ConfirmDialog
          busy={acting}
          cancelLabel={t("cancel")}
          closeLabel={t("closeDialog")}
          confirmLabel={t("cancelApproval")}
          description={t("confirmCancelApproval")}
          title={t("cancelApproval")}
          onCancel={() => {
            if (!acting) setPendingCancellation(undefined);
          }}
          onConfirm={() => void cancelRequest()}
        >
          <p>
            <strong>{pendingCancellation.action}</strong>
            {" · "}
            <code>{shortId(pendingCancellation.approval_request_id)}</code>
          </p>
        </ConfirmDialog>
      ) : null}
    </>
  );
}
