import {
  type BadgeTone,
  AddIcon,
  Button,
  ConfirmDialog,
  ContextBar,
  Dialog,
  EmptyState,
  EvidenceInspector,
  Field,
  Input,
  Panel,
  Select,
  Spinner,
  StatusIndicator,
  UnknownWriteLock,
} from "@copilotmail/design-system";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { AdminErrorNotice, AdminUiError } from "./AdminErrorNotice";
import { AdminListDetail } from "./AdminListDetail";
import { ControlPlaneClient, isUnknownWriteFailure } from "./api";
import type { translator } from "./i18n";
import type { Locale, MailDomain } from "./types";

type Translate = ReturnType<typeof translator>;
type RouteMode = MailDomain["route_mode"];
type DialogKind = "create" | "token" | "activate" | "suspend" | "rotate";
type WriteOperation = "create" | "rotate" | "verify" | "activate" | "suspend" | "route";
type DnsValidation = "required" | "invalid-selector";
type RouteValidation = "activation" | "smart-host" | "dkim";
type SuspendValidation = "required" | "invalid";
type LifecycleDecision = "verify" | "activate" | "route" | "none";
type OneTimeToken = Awaited<ReturnType<ControlPlaneClient["createMailDomain"]>>;
type ChallengeRotation = Awaited<
  ReturnType<ControlPlaneClient["rotateMailDomainOwnershipChallenge"]>
>;
type DnsVerification = Awaited<
  ReturnType<ControlPlaneClient["verifyMailDomainDns"]>
>;
type RouteUpdate = Awaited<ReturnType<ControlPlaneClient["updateMailDomainRoute"]>>;

interface CreateDraft {
  name: string;
  routeMode: RouteMode | "";
}

interface RouteDraft {
  inboundEnabled: boolean;
  outboundEnabled: boolean;
  smartHostRef: string;
  dkimKeyRef: string;
}

interface WriteEvidence {
  kind: "created" | "challenge-rotated" | "dns-verified" | "activated" | "suspended" | "route-updated";
  domainId: string;
  version?: number;
  validUntil?: string;
}

interface UnknownWriteContext {
  /** The exact mutation whose server-side outcome remains unresolved. */
  operation: WriteOperation;
  /**
   * Only existing-domain operations have a trustworthy read target. A create
   * response that never arrived must not be guessed from a broad list query.
   */
  targetDomainId?: string;
}

const emptyCreateDraft = (): CreateDraft => ({ name: "", routeMode: "" });

function routeDraftFor(domain: MailDomain): RouteDraft {
  return {
    inboundEnabled: domain.inbound_enabled,
    outboundEnabled: domain.outbound_enabled,
    smartHostRef: domain.smart_host_ref ?? "",
    dkimKeyRef: domain.dkim_key_ref ?? "",
  };
}

function statusLabel(status: MailDomain["status"], t: Translate): string {
  switch (status) {
    case "draft": return t("domainStatusDraft");
    case "pending_dns": return t("domainStatusPendingDns");
    case "verified": return t("domainStatusVerified");
    case "active": return t("domainStatusActive");
    case "suspended": return t("domainStatusSuspended");
    case "retired": return t("domainStatusRetired");
  }
}

function statusTone(status: MailDomain["status"]): BadgeTone {
  switch (status) {
    case "draft": return "neutral";
    case "pending_dns": return "warning";
    case "verified": return "info";
    case "active": return "positive";
    case "suspended": return "critical";
    case "retired": return "neutral";
  }
}

function routeModeLabel(routeMode: RouteMode, t: Translate): string {
  return routeMode === "direct_mx" ? t("domainDirectMx") : t("domainSmartHost");
}

function projectionLabel(
  status: MailDomain["mail_engine_projection_status"],
  t: Translate,
): string {
  switch (status) {
    case "not_projected": return t("domainProjectionNotProjected");
    case "pending": return t("domainProjectionPending");
    case "ready": return t("domainProjectionReady");
    case "failed": return t("domainProjectionFailed");
  }
}

function projectionTone(
  status: MailDomain["mail_engine_projection_status"],
): BadgeTone {
  switch (status) {
    case "not_projected": return "neutral";
    case "pending": return "warning";
    case "ready": return "positive";
    case "failed": return "critical";
  }
}

function dnsEvidenceLabel(domain: MailDomain, t: Translate): string {
  return domain.latest_dns_activation_ready
    ? t("domainDnsVerified")
    : t("domainDnsNotActivationReady");
}

function dnsEvidenceTone(domain: MailDomain): BadgeTone {
  return domain.latest_dns_activation_ready ? "positive" : "warning";
}

/**
 * This intentionally derives the one prominent decision from the persisted
 * control-plane lifecycle. DNS evidence and engine projection are separate
 * observations, never substitute states for activation.
 */
function lifecycleDecision(domain: MailDomain): LifecycleDecision {
  if (
    domain.status === "draft"
    || domain.status === "suspended"
    || domain.status === "retired"
  ) return "none";
  if (domain.status === "verified" && domain.latest_dns_activation_ready) {
    return "activate";
  }
  if (domain.status === "active" && domain.latest_dns_activation_ready) {
    return "route";
  }
  return "verify";
}

function formatDate(value: string | null, locale: Locale, empty: string): string {
  if (!value) return empty;
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return empty;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(parsed);
}

function isCandidateDomain(value: string): boolean {
  return Boolean(value)
    && value === value.trim()
    && value.length <= 253
    && !/[\s\p{C}]/u.test(value);
}

function isDkimSelector(value: string): boolean {
  return /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(value);
}

function isReasonCode(value: string): boolean {
  return /^[a-z0-9_.-]{1,100}$/.test(value);
}

function optionalReference(value: string): string | null {
  const normalized = value.trim();
  return normalized || null;
}

function ownershipRecordName(domain: MailDomain): string {
  return `_copilotmail-verification.${domain.name}`;
}

function evidenceLabel(evidence: WriteEvidence, t: Translate): string {
  switch (evidence.kind) {
    case "created": return t("domainCreateCommitted");
    case "challenge-rotated": return t("domainChallengeRotated");
    case "dns-verified": return t("domainDnsVerified");
    case "activated": return t("domainActivated");
    case "suspended": return t("domainSuspended");
    case "route-updated": return t("domainRouteCommitted");
  }
}

function OneTimeTokenDialog({
  token,
  t,
  onClose,
}: {
  token: OneTimeToken | ChallengeRotation;
  t: Translate;
  onClose: () => void;
}) {
  return (
    <Dialog
      closeLabel={t("closeDialog")}
      description={t("domainTokenWarning")}
      title={t("domainOneTimeToken")}
      onClose={onClose}
    >
      <div className="dialog__body form-grid">
        <Field label={t("domainOwnershipRecord")}>
          <Input
            autoComplete="off"
            readOnly
            spellCheck={false}
            value={token.ownership_record_name}
          />
        </Field>
        <Field label={t("domainOwnershipToken")}>
          <Input
            autoComplete="off"
            readOnly
            spellCheck={false}
            value={token.ownership_token}
          />
        </Field>
        <dl>
          <dt>{t("domainChallengeVersion")}</dt>
          <dd>{token.ownership_challenge_version}</dd>
        </dl>
      </div>
      <div className="dialog__actions">
        <Button type="button" onClick={onClose}>{t("closeDialog")}</Button>
      </div>
    </Dialog>
  );
}

export default function MailDomainsView({
  client,
  locale,
  t,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: Translate;
}) {
  const [domains, setDomains] = useState<MailDomain[]>([]);
  const [nextAfter, setNextAfter] = useState<string | null>(null);
  const [afterHistory, setAfterHistory] = useState<Array<string | undefined>>([
    undefined,
  ]);
  const [listLoading, setListLoading] = useState(true);
  const [listError, setListError] = useState<unknown>();
  const [selectedId, setSelectedId] = useState<string>();
  const [selected, setSelected] = useState<MailDomain>();
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailFresh, setDetailFresh] = useState(false);
  const [detailError, setDetailError] = useState<unknown>();
  const [dialog, setDialog] = useState<DialogKind>();
  const [createDraft, setCreateDraft] = useState<CreateDraft>(emptyCreateDraft);
  const [createValidation, setCreateValidation] = useState<"required" | "invalid">();
  const [oneTimeToken, setOneTimeToken] = useState<OneTimeToken | ChallengeRotation>();
  const [dnsToken, setDnsToken] = useState("");
  const [dkimSelector, setDkimSelector] = useState("");
  const [dnsValidation, setDnsValidation] = useState<DnsValidation>();
  const [dnsEvidence, setDnsEvidence] = useState<DnsVerification>();
  const [routeDraft, setRouteDraft] = useState<RouteDraft>();
  const [routeValidation, setRouteValidation] = useState<RouteValidation>();
  const [suspendReason, setSuspendReason] = useState("");
  const [suspendValidation, setSuspendValidation] = useState<SuspendValidation>();
  const [operation, setOperation] = useState<WriteOperation>();
  const [unknownWriteContext, setUnknownWriteContext] = useState<UnknownWriteContext>();
  const [writeError, setWriteError] = useState<unknown>();
  const [evidence, setEvidence] = useState<WriteEvidence>();
  const listSequence = useRef(0);
  const detailSequence = useRef(0);
  const operationRef = useRef<WriteOperation | undefined>(undefined);
  const draftIdentity = useRef("");
  const registerButtonRef = useRef<HTMLButtonElement>(null);
  const createNameRef = useRef<HTMLInputElement>(null);

  const after = afterHistory.at(-1);

  const loadList = useCallback(async (requestedAfter = after) => {
    const sequence = listSequence.current + 1;
    listSequence.current = sequence;
    setListLoading(true);
    setListError(undefined);
    try {
      const response = await client.listMailDomains({ after: requestedAfter, limit: 50 });
      if (sequence !== listSequence.current) return;
      setDomains(response.items);
      setNextAfter(response.next_after);
    } catch (cause) {
      if (sequence !== listSequence.current) return;
      setListError(cause);
      setNextAfter(null);
    } finally {
      if (sequence === listSequence.current) setListLoading(false);
    }
  }, [after, client]);

  const readDomain = useCallback(async (
    domainId: string,
    { clearCurrent }: { clearCurrent: boolean },
  ) => {
    const sequence = detailSequence.current + 1;
    detailSequence.current = sequence;
    if (clearCurrent) setSelected(undefined);
    setSelectedId(domainId);
    setDetailFresh(false);
    setDetailLoading(true);
    setDetailError(undefined);
    try {
      const response = await client.getMailDomain(domainId);
      if (sequence !== detailSequence.current) return;
      if (response.mail_domain_id !== domainId) {
        throw new AdminUiError("mail_domain_detail_mismatch");
      }
      // Populate the CAS form in the same committed read render. Waiting for
      // a post-paint effect leaves an avoidable blank operational section and
      // invites a click against a stale detail.
      draftIdentity.current = `${response.mail_domain_id}:${response.route_version}`;
      setRouteDraft(routeDraftFor(response));
      setSelected(response);
      setDetailFresh(true);
    } catch (cause) {
      if (sequence !== detailSequence.current) return;
      setDetailFresh(false);
      setDetailError(cause);
    } finally {
      if (sequence === detailSequence.current) setDetailLoading(false);
    }
  }, [client]);

  useEffect(() => {
    void loadList();
    return () => {
      listSequence.current += 1;
    };
  }, [loadList]);

  useEffect(() => () => {
    detailSequence.current += 1;
    operationRef.current = undefined;
  }, []);

  useEffect(() => {
    if (!selected) {
      draftIdentity.current = "";
      setRouteDraft(undefined);
      return;
    }
    const identity = `${selected.mail_domain_id}:${selected.route_version}`;
    if (draftIdentity.current === identity) return;
    draftIdentity.current = identity;
    setRouteDraft(routeDraftFor(selected));
  }, [selected]);

  useEffect(() => {
    setDnsToken("");
    setDkimSelector("");
    setDnsValidation(undefined);
    setDnsEvidence(undefined);
    setRouteValidation(undefined);
    setSuspendReason("");
    setSuspendValidation(undefined);
  }, [selectedId]);

  function beginWrite(next: WriteOperation): boolean {
    if (operationRef.current !== undefined || writeFrozen) return false;
    operationRef.current = next;
    setOperation(next);
    setWriteError(undefined);
    return true;
  }

  function finishWrite(current: WriteOperation) {
    if (operationRef.current !== current) return;
    operationRef.current = undefined;
    setOperation(undefined);
  }

  function clearSelection() {
    detailSequence.current += 1;
    setSelectedId(undefined);
    setSelected(undefined);
    setDetailFresh(false);
    setDetailError(undefined);
    setDetailLoading(false);
  }

  function selectDomain(domainId: string) {
    if (operationRef.current !== undefined || writeFrozen) return;
    void readDomain(domainId, { clearCurrent: true });
  }

  async function reconcileUnknownWriteReadOnly() {
    const domainId = unknownWriteContext?.targetDomainId;
    if (!domainId) return;
    // Do not use the mutable current selection or refresh the list here. The
    // uncertain write is tied to this exact resource, and reconciliation must
    // remain a single read-only GET for that resource.
    await readDomain(domainId, { clearCurrent: false });
  }

  async function refreshAfterCommitted(domainId: string) {
    await Promise.all([
      loadList(),
      readDomain(domainId, { clearCurrent: false }),
    ]);
  }

  function unknownWrite(cause: unknown, context: UnknownWriteContext) {
    if (!isUnknownWriteFailure(cause)) {
      setWriteError(cause);
      return;
    }
    // The request might already have committed. No mutation is retried from
    // this browser session; only explicit read operations remain available.
    // Keep the intent after executeWrite's finally block clears the active
    // operation ref. It defines the one safe follow-up read, if there is one.
    setUnknownWriteContext(context);
    setDetailFresh(false);
    setDialog(undefined);
    setOneTimeToken(undefined);
    setDnsToken("");
    setWriteError(cause);
  }

  async function executeWrite<T>(
    kind: WriteOperation,
    targetDomainId: string | undefined,
    request: () => Promise<T>,
    onCommitted: (response: T) => Promise<void> | void,
  ) {
    if (!beginWrite(kind)) return;
    try {
      const response = await request();
      await onCommitted(response);
    } catch (cause) {
      unknownWrite(cause, { operation: kind, targetDomainId });
    } finally {
      finishWrite(kind);
    }
  }

  function openCreate() {
    if (operationRef.current !== undefined || writeFrozen) return;
    setCreateDraft(emptyCreateDraft());
    setCreateValidation(undefined);
    setDialog("create");
  }

  function submitCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const draft = createDraft;
    const name = draft.name.trim();
    const routeMode = draft.routeMode;
    if (!name || !routeMode) {
      setCreateValidation("required");
      return;
    }
    if (!isCandidateDomain(draft.name)) {
      setCreateValidation("invalid");
      return;
    }
    setCreateValidation(undefined);
    void executeWrite(
      "create",
      undefined,
      () => client.createMailDomain({ name, route_mode: routeMode }),
      async (response) => {
        setOneTimeToken(response);
        setEvidence({ kind: "created", domainId: response.mail_domain_id });
        setCreateDraft(emptyCreateDraft());
        // A newly registered domain must be visible even when the operator
        // created it from a later page. Read the first page explicitly rather
        // than waiting for React state to publish the new cursor.
        setAfterHistory([undefined]);
        setDialog("token");
        await Promise.all([
          loadList(undefined),
          readDomain(response.mail_domain_id, { clearCurrent: true }),
        ]);
      },
    );
  }

  function rotateChallenge() {
    const domain = selected;
    if (!domain) return;
    void executeWrite(
      "rotate",
      domain.mail_domain_id,
      () => client.rotateMailDomainOwnershipChallenge(
        domain.mail_domain_id,
        domain.ownership_challenge_version,
      ),
      async (response) => {
        if (response.mail_domain_id !== domain.mail_domain_id) {
          throw new AdminUiError("mail_domain_challenge_mismatch");
        }
        setOneTimeToken(response);
        setEvidence({
          kind: "challenge-rotated",
          domainId: response.mail_domain_id,
          version: response.ownership_challenge_version,
        });
        setDialog("token");
        await refreshAfterCommitted(response.mail_domain_id);
      },
    );
  }

  function verifyDns(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const domain = selected;
    const ownershipToken = dnsToken.trim();
    const selector = dkimSelector.trim();
    if (!domain || !ownershipToken || !selector) {
      setDnsValidation("required");
      return;
    }
    if (!isDkimSelector(selector)) {
      setDnsValidation("invalid-selector");
      return;
    }
    setDnsValidation(undefined);
    void executeWrite(
      "verify",
      domain.mail_domain_id,
      () => client.verifyMailDomainDns(domain.mail_domain_id, {
        domain: domain.name,
        ownership_token: ownershipToken,
        dkim_selector: selector,
      }),
      async (response) => {
        if (response.mail_domain_id !== domain.mail_domain_id) {
          throw new AdminUiError("mail_domain_dns_mismatch");
        }
        setDnsToken("");
        setDnsEvidence(response);
        setEvidence({
          kind: "dns-verified",
          domainId: response.mail_domain_id,
          validUntil: response.valid_until,
        });
        await refreshAfterCommitted(response.mail_domain_id);
      },
    );
  }

  function activateDomain() {
    const domain = selected;
    if (!domain) return;
    void executeWrite(
      "activate",
      domain.mail_domain_id,
      () => client.activateMailDomain(domain.mail_domain_id),
      async (response) => {
        if (
          response.mail_domain_id !== domain.mail_domain_id
          || response.status !== "active"
        ) throw new AdminUiError("mail_domain_activation_mismatch");
        setEvidence({ kind: "activated", domainId: response.mail_domain_id });
        setDialog(undefined);
        await refreshAfterCommitted(response.mail_domain_id);
      },
    );
  }

  function suspendDomain() {
    const domain = selected;
    const reasonCode = suspendReason.trim();
    if (!domain || !reasonCode) {
      setSuspendValidation("required");
      return;
    }
    if (!isReasonCode(reasonCode)) {
      setSuspendValidation("invalid");
      return;
    }
    setSuspendValidation(undefined);
    void executeWrite(
      "suspend",
      domain.mail_domain_id,
      () => client.suspendMailDomain(domain.mail_domain_id, { reason_code: reasonCode }),
      async (response) => {
        if (
          response.mail_domain_id !== domain.mail_domain_id
          || response.status !== "suspended"
        ) throw new AdminUiError("mail_domain_suspension_mismatch");
        setEvidence({ kind: "suspended", domainId: response.mail_domain_id });
        setSuspendReason("");
        setDialog(undefined);
        await refreshAfterCommitted(response.mail_domain_id);
      },
    );
  }

  function saveRoute(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const domain = selected;
    const draft = routeDraft;
    if (!domain || !draft) return;
    const smartHostRef = optionalReference(draft.smartHostRef);
    const dkimKeyRef = optionalReference(draft.dkimKeyRef);
    if ((draft.inboundEnabled || draft.outboundEnabled) && domain.status !== "active") {
      setRouteValidation("activation");
      return;
    }
    if (draft.outboundEnabled && domain.route_mode === "smart_host" && !smartHostRef) {
      setRouteValidation("smart-host");
      return;
    }
    if (draft.outboundEnabled && !dkimKeyRef) {
      setRouteValidation("dkim");
      return;
    }
    setRouteValidation(undefined);
    void executeWrite(
      "route",
      domain.mail_domain_id,
      () => client.updateMailDomainRoute(domain.mail_domain_id, {
        expected_version: domain.route_version,
        inbound_enabled: draft.inboundEnabled,
        outbound_enabled: draft.outboundEnabled,
        smart_host_ref: domain.route_mode === "smart_host" ? smartHostRef : null,
        dkim_key_ref: dkimKeyRef,
      }),
      async (response: RouteUpdate) => {
        if (response.mail_domain_id !== domain.mail_domain_id) {
          throw new AdminUiError("mail_domain_route_mismatch");
        }
        setEvidence({
          kind: "route-updated",
          domainId: response.mail_domain_id,
          version: response.version,
        });
        await refreshAfterCommitted(response.mail_domain_id);
      },
    );
  }

  const writeFrozen = unknownWriteContext !== undefined;
  const writing = operation !== undefined;
  const canWriteDetail = Boolean(selected && detailFresh && !writing && !writeFrozen);
  const tokenDialog = dialog === "token" && oneTimeToken;
  const detailTitle = selected?.name ?? t("domainDetail");
  const selectedIsRetired = selected?.status === "retired";
  const canVerifyDns = Boolean(
    selected && (
      selected.status === "pending_dns"
      || selected.status === "verified"
      || selected.status === "active"
    ),
  );
  const currentLifecycleDecision = selected ? lifecycleDecision(selected) : "none";
  const suspendDomainAction = selected?.status === "active" ? (
    <Button
      disabled={!canWriteDetail}
      type="button"
      variant="secondary"
      onClick={() => setDialog("suspend")}
    >
      {t("domainSuspend")}
    </Button>
  ) : null;

  const dnsVerificationForm = selected && canVerifyDns ? (
    <form className="form-grid" onSubmit={verifyDns}>
      {dnsValidation ? (
        <p role="alert">{dnsValidation === "required"
          ? t("domainRequired")
          : t("domainInvalidDkimSelector")}</p>
      ) : null}
      <Field label={t("domainOwnershipToken")} hint={t("domainTokenWarning")}>
        <Input
          autoComplete="off"
          disabled={!canWriteDetail}
          spellCheck={false}
          type="password"
          value={dnsToken}
          onChange={(event) => {
            setDnsToken(event.target.value);
            setDnsValidation(undefined);
          }}
        />
      </Field>
      <Field label={t("domainDkimSelector")}>
        <Input
          autoComplete="off"
          disabled={!canWriteDetail}
          maxLength={63}
          spellCheck={false}
          value={dkimSelector}
          onChange={(event) => {
            setDkimSelector(event.target.value);
            setDnsValidation(undefined);
          }}
        />
      </Field>
      <div className="detail-actions">
        <Button disabled={!canWriteDetail} type="submit">
          {t("domainVerifyDns")}
        </Button>
      </div>
    </form>
  ) : null;

  const routeConfigurationForm = selected && routeDraft ? (
    <form className="form-grid" onSubmit={saveRoute}>
      {routeValidation ? <p role="alert">{
        routeValidation === "activation"
          ? t("domainRouteActivationRequired")
          : routeValidation === "smart-host"
            ? t("domainRouteSmartHostRequired")
            : t("domainRouteDkimRequired")
      }</p> : null}
      <label className="checkbox-field">
        <input
          checked={routeDraft.inboundEnabled}
          disabled={!canWriteDetail || selectedIsRetired}
          type="checkbox"
          onChange={(event) => setRouteDraft((current) => current
            ? { ...current, inboundEnabled: event.target.checked }
            : current)}
        />
        {t("domainRouteInbound")}
      </label>
      <label className="checkbox-field">
        <input
          checked={routeDraft.outboundEnabled}
          disabled={!canWriteDetail || selectedIsRetired}
          type="checkbox"
          onChange={(event) => setRouteDraft((current) => current
            ? { ...current, outboundEnabled: event.target.checked }
            : current)}
        />
        {t("domainRouteOutbound")}
      </label>
      {selected.route_mode === "smart_host" ? (
        <Field label={t("domainSmartHostRef")}>
          <Input
            disabled={!canWriteDetail || selectedIsRetired}
            value={routeDraft.smartHostRef}
            onChange={(event) => {
              setRouteDraft((current) => current
                ? { ...current, smartHostRef: event.target.value }
                : current);
              setRouteValidation(undefined);
            }}
          />
        </Field>
      ) : null}
      <Field label={t("domainDkimKeyRef")}>
        <Input
          disabled={!canWriteDetail || selectedIsRetired}
          value={routeDraft.dkimKeyRef}
          onChange={(event) => {
            setRouteDraft((current) => current
              ? { ...current, dkimKeyRef: event.target.value }
              : current);
            setRouteValidation(undefined);
          }}
        />
      </Field>
      <div className="detail-actions">
        <Button disabled={!canWriteDetail || selectedIsRetired} type="submit">
          {t("domainSaveRoute")}
        </Button>
        {suspendDomainAction}
      </div>
    </form>
  ) : null;

  const detailBody = selected ? (
    <>
      <ContextBar
        facts={[
          { id: "domain-name", label: t("domainName"), value: <code>{selected.name}</code> },
          { id: "route-mode", label: t("domainRouteMode"), value: routeModeLabel(selected.route_mode, t) },
          { id: "route-version", label: t("domainRouteVersion"), value: selected.route_version },
          {
            id: "dns-observed-at",
            label: t("domainDnsObservedAt"),
            value: formatDate(selected.latest_dns_observed_at, locale, t("domainProjectionUnknown")),
          },
        ]}
        headingLevel={3}
        status={<StatusIndicator label={statusLabel(selected.status, t)} tone={statusTone(selected.status)} />}
        title={t("domainLifecycle")}
      />

      <ol aria-label={t("domainLifecycle")} className="mail-domain-lifecycle">
        <li className="mail-domain-lifecycle__stage">
          <span className="mail-domain-lifecycle__label">{t("domainLifecycleRegistered")}</span>
          <StatusIndicator label={t("domainCreateCommitted")} tone="neutral" />
          <small>{t("domainChallengeVersion")}: {selected.ownership_challenge_version}</small>
        </li>
        <li className="mail-domain-lifecycle__stage">
          <span className="mail-domain-lifecycle__label">{t("domainDnsEvidence")}</span>
          <StatusIndicator
            label={dnsEvidenceLabel(selected, t)}
            tone={dnsEvidenceTone(selected)}
          />
          <small>{t("domainDnsValidUntil")}: {formatDate(
            selected.latest_dns_valid_until,
            locale,
            t("domainProjectionUnknown"),
          )}</small>
        </li>
        <li className="mail-domain-lifecycle__stage">
          <span className="mail-domain-lifecycle__label">{t("domainLifecycleActivation")}</span>
          <StatusIndicator label={statusLabel(selected.status, t)} tone={statusTone(selected.status)} />
          <small>{t("domainRouteVersion")}: {selected.route_version}</small>
        </li>
        <li className="mail-domain-lifecycle__stage">
          <span className="mail-domain-lifecycle__label">{t("domainProjection")}</span>
          <StatusIndicator
            label={projectionLabel(selected.mail_engine_projection_status, t)}
            tone={projectionTone(selected.mail_engine_projection_status)}
          />
          <small>{selected.mail_engine_projected_enabled === true
            ? t("domainProjectionEnabled")
            : selected.mail_engine_projected_enabled === false
              ? t("domainProjectionDisabled")
              : t("domainProjectionUnknown")}</small>
        </li>
      </ol>

      <section className="mail-domain-decision" aria-labelledby="mail-domain-decision-heading">
        <h3 id="mail-domain-decision-heading">{t("domainCurrentDecision")}</h3>
        {currentLifecycleDecision === "verify" ? (
          <>
            <p>{t("domainDecisionDnsHint")}</p>
            {dnsVerificationForm}
            {suspendDomainAction ? (
              <div className="detail-actions">{suspendDomainAction}</div>
            ) : null}
          </>
        ) : currentLifecycleDecision === "activate" ? (
          <>
            <p>{t("domainDecisionActivationHint")}</p>
            <div className="detail-actions">
              <Button
                disabled={!canWriteDetail}
                type="button"
                onClick={() => setDialog("activate")}
              >
                {t("domainActivate")}
              </Button>
            </div>
          </>
        ) : currentLifecycleDecision === "route" ? (
          <>
            <p>{t("domainDecisionRouteHint")}</p>
            {routeConfigurationForm}
          </>
        ) : <p>{t("domainDecisionUnavailable")}</p>}
        {dnsEvidence ? (
          <div aria-live="polite" className="notice" role="status">
            <strong>{t("domainDnsVerified")}</strong>
            <span>{formatDate(dnsEvidence.valid_until, locale, t("domainProjectionUnknown"))}</span>
          </div>
        ) : null}
      </section>

      <EvidenceInspector
        className="mail-domain-evidence"
        defaultOpen={false}
        summary={t("domainEvidenceInspector")}
      >
        <section className="mail-domain-detail__section" aria-labelledby="mail-domain-ownership-heading">
          <h3 id="mail-domain-ownership-heading">{t("domainOwnership")}</h3>
          <dl>
            <dt>{t("domainOwnershipRecord")}</dt>
            <dd><code>{ownershipRecordName(selected)}</code></dd>
            <dt>{t("domainChallengeVersion")}</dt>
            <dd>{selected.ownership_challenge_version}</dd>
          </dl>
          {!selectedIsRetired ? (
            <div className="detail-actions">
              <Button
                disabled={!canWriteDetail || selected.status === "active"}
                type="button"
                variant="secondary"
                onClick={() => setDialog("rotate")}
              >
                {t("domainRotateChallenge")}
              </Button>
            </div>
          ) : null}
        </section>

        <section className="mail-domain-detail__section" aria-labelledby="mail-domain-dns-heading">
          <h3 id="mail-domain-dns-heading">{t("domainDnsEvidence")}</h3>
          <dl>
            <dt>{t("domainActivationReady")}</dt>
            <dd>
              <StatusIndicator
                label={dnsEvidenceLabel(selected, t)}
                tone={dnsEvidenceTone(selected)}
              />
            </dd>
            <dt>{t("domainDnsValidUntil")}</dt>
            <dd>{formatDate(
              selected.latest_dns_valid_until,
              locale,
              t("domainProjectionUnknown"),
            )}</dd>
            <dt>{t("domainDnsObservedAt")}</dt>
            <dd>{formatDate(
              selected.latest_dns_observed_at,
              locale,
              t("domainProjectionUnknown"),
            )}</dd>
            {selected.latest_dns_failure_codes.length ? (
              <>
                <dt>{t("domainDnsFailures")}</dt>
                <dd>
                  <ul>
                    {selected.latest_dns_failure_codes.map((code) => (
                      <li key={code}><code>{code}</code></li>
                    ))}
                  </ul>
                </dd>
              </>
            ) : null}
          </dl>
        </section>

        <section className="mail-domain-detail__section" aria-labelledby="mail-domain-route-heading">
          <h3 id="mail-domain-route-heading">{t("domainRouteEvidence")}</h3>
          <dl>
            <dt>{t("domainRouteMode")}</dt>
            <dd>{routeModeLabel(selected.route_mode, t)}</dd>
            <dt>{t("domainRouteVersion")}</dt>
            <dd>{selected.route_version}</dd>
            <dt>{t("domainRouteInbound")}</dt>
            <dd>{selected.inbound_enabled ? t("domainRouteEnabled") : t("domainRouteDisabled")}</dd>
            <dt>{t("domainRouteOutbound")}</dt>
            <dd>{selected.outbound_enabled ? t("domainRouteEnabled") : t("domainRouteDisabled")}</dd>
            {selected.smart_host_ref ? (
              <>
                <dt>{t("domainSmartHostRef")}</dt>
                <dd><code>{selected.smart_host_ref}</code></dd>
              </>
            ) : null}
            {selected.dkim_key_ref ? (
              <>
                <dt>{t("domainDkimKeyRef")}</dt>
                <dd><code>{selected.dkim_key_ref}</code></dd>
              </>
            ) : null}
          </dl>
        </section>

        <section className="mail-domain-detail__section" aria-labelledby="mail-domain-projection-heading">
          <h3 id="mail-domain-projection-heading">{t("domainProjection")}</h3>
          <dl>
            <dt>{t("domainProjection")}</dt>
            <dd>
              <StatusIndicator
                label={projectionLabel(selected.mail_engine_projection_status, t)}
                tone={projectionTone(selected.mail_engine_projection_status)}
              />
            </dd>
            <dt>{t("domainProjectionEnabled")}</dt>
            <dd>{selected.mail_engine_projected_enabled === true
              ? t("domainProjectionEnabled")
              : selected.mail_engine_projected_enabled === false
                ? t("domainProjectionDisabled")
                : t("domainProjectionUnknown")}</dd>
            {selected.mail_engine_domain_ref ? (
              <>
                <dt>{t("domainEngineReference")}</dt>
                <dd><code>{selected.mail_engine_domain_ref}</code></dd>
              </>
            ) : null}
            <dt>{t("domainProjectionLastAttempt")}</dt>
            <dd>{formatDate(
              selected.mail_engine_projection_last_attempt_at,
              locale,
              t("domainProjectionUnknown"),
            )}</dd>
            <dt>{t("domainProjectionSucceeded")}</dt>
            <dd>{formatDate(
              selected.mail_engine_projection_succeeded_at,
              locale,
              t("domainProjectionUnknown"),
            )}</dd>
            {selected.mail_engine_projection_error_code ? (
              <>
                <dt>{t("domainProjectionError")}</dt>
                <dd><code>{selected.mail_engine_projection_error_code}</code></dd>
              </>
            ) : null}
          </dl>
        </section>
      </EvidenceInspector>
    </>
  ) : detailLoading ? <Spinner label={t("domainDetailLoading")} /> : detailError ? (
    <AdminErrorNotice
      error={detailError}
      retry={selectedId ? () => void readDomain(selectedId, { clearCurrent: false }) : undefined}
      t={t}
    />
  ) : <EmptyState title={t("domainDetail")} description={t("domainSelectionEmpty")} />;

  return (
    <section className="view-stack" aria-labelledby="mail-domains-title">
      <header className="view-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1 id="mail-domains-title">{t("domainTitle")}</h1>
          <p>{t("domainHint")}</p>
        </div>
        <Button
          ref={registerButtonRef}
          disabled={writing || writeFrozen}
          type="button"
          onClick={openCreate}
        >
          <AddIcon aria-hidden size={17} />
          {t("domainRegister")}
        </Button>
      </header>

      {writeFrozen ? (
        <UnknownWriteLock
          className="mail-domain-unknown-write"
          description={unknownWriteContext.targetDomainId
            ? t("domainWriteOutcomeUnknownHint")
            : t("writeOutcomeUnknown")}
          evidence={unknownWriteContext.targetDomainId ? (
            <code>{unknownWriteContext.targetDomainId}</code>
          ) : undefined}
          reconciliationAction={unknownWriteContext.targetDomainId ? (
            <Button
              disabled={detailLoading}
              type="button"
              variant="secondary"
              onClick={() => void reconcileUnknownWriteReadOnly()}
            >
              {t("domainReadOnlyRefresh")}
            </Button>
          ) : undefined}
          title={t("domainWriteOutcomeUnknown")}
        />
      ) : null}
      {writeError ? <AdminErrorNotice error={writeError} t={t} /> : null}
      {evidence ? (
        <div aria-live="polite" className="notice" role="status">
          <strong>{evidenceLabel(evidence, t)}</strong>
          <code>{evidence.domainId}</code>
          {evidence.version !== undefined ? <code>{evidence.version}</code> : null}
          {evidence.validUntil ? (
            <span>{formatDate(evidence.validUntil, locale, t("domainProjectionUnknown"))}</span>
          ) : null}
        </div>
      ) : null}
      {listError ? (
        <AdminErrorNotice
          error={listError}
          retry={writeFrozen ? undefined : () => void loadList()}
          t={t}
        />
      ) : null}
      {listLoading && !domains.length ? <Spinner label={t("loading")} /> : null}

      {domains.length ? (
        <AdminListDetail
          className="mail-domain-workbench"
          backLabel={t("domainList")}
          detailBody={detailBody}
          detailEmpty={<EmptyState title={t("domainDetail")} description={t("domainSelectionEmpty")} />}
          detailHeader={<p className="eyebrow">{t("domainDetail")}</p>}
          detailClassName="mail-domain-detail"
          detailTitle={detailTitle}
          detailTrailing={selected ? (
            <StatusIndicator
              label={statusLabel(selected.status, t)}
              tone={statusTone(selected.status)}
            />
          ) : undefined}
          listLabel={t("domainList")}
          navigationLocked={writing || writeFrozen}
          selectionKey={selectedId ?? null}
          onBack={clearSelection}
          renderList={(registerTrigger) => (
            <>
              <div className="table-wrap">
                <table className="mail-domain-table">
                  <caption className="sr-only">{t("domainList")}</caption>
                  <thead>
                    <tr>
                      <th>{t("domainName")}</th>
                      <th>{t("domainRouteMode")}</th>
                      <th>{t("status")}</th>
                      <th>{t("domainProjection")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {domains.map((domain) => (
                      <tr
                        key={domain.mail_domain_id}
                        className={selectedId === domain.mail_domain_id ? "is-selected" : ""}
                      >
                        <td>
                          <button
                            ref={registerTrigger(domain.mail_domain_id)}
                            aria-current={selectedId === domain.mail_domain_id ? "true" : undefined}
                            className="row-button"
                            disabled={writing || writeFrozen}
                            type="button"
                            onClick={() => selectDomain(domain.mail_domain_id)}
                          >
                            {domain.name}
                          </button>
                        </td>
                        <td>{routeModeLabel(domain.route_mode, t)}</td>
                        <td>
                          <StatusIndicator
                            label={statusLabel(domain.status, t)}
                            tone={statusTone(domain.status)}
                          />
                        </td>
                        <td>
                          <StatusIndicator
                            label={projectionLabel(domain.mail_engine_projection_status, t)}
                            tone={projectionTone(domain.mail_engine_projection_status)}
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {afterHistory.length > 1 || nextAfter ? (
                <nav aria-label={t("domainListPagination")} className="pagination">
                  <Button
                    disabled={afterHistory.length === 1 || listLoading || writing || writeFrozen}
                    type="button"
                    variant="secondary"
                    onClick={() => {
                      clearSelection();
                      setAfterHistory((history) => history.slice(0, -1));
                    }}
                  >
                    {t("previous")}
                  </Button>
                  <Button
                    disabled={!nextAfter || listLoading || writing || writeFrozen}
                    type="button"
                    variant="secondary"
                    onClick={() => {
                      if (!nextAfter) return;
                      clearSelection();
                      setAfterHistory((history) => [...history, nextAfter]);
                    }}
                  >
                    {t("next")}
                  </Button>
                </nav>
              ) : null}
            </>
          )}
        />
      ) : !listLoading && !listError ? (
        <EmptyState
          action={(
            <Button disabled={writeFrozen} type="button" onClick={openCreate}>
              {t("domainRegister")}
            </Button>
          )}
          description={t("domainNoDomainsHint")}
          title={t("domainNoDomains")}
        />
      ) : null}

      {dialog === "create" ? (
        <Dialog
          busy={operation === "create"}
          closeLabel={t("closeDialog")}
          description={t("domainRegisterHint")}
          dismissDisabled={operation === "create"}
          initialFocusRef={createNameRef}
          returnFocusRef={registerButtonRef}
          title={t("domainRegisterTitle")}
          onClose={() => {
            if (operation !== "create") setDialog(undefined);
          }}
        >
          <form onSubmit={submitCreate}>
            <div className="dialog__body form-grid">
              {createValidation ? (
                <p role="alert">{createValidation === "required"
                  ? t("domainRequired")
                  : t("domainInvalidName")}</p>
              ) : null}
              <Field label={t("domainName")}>
                <Input
                  ref={createNameRef}
                  autoComplete="off"
                  disabled={operation === "create"}
                  spellCheck={false}
                  value={createDraft.name}
                  onChange={(event) => {
                    setCreateDraft((current) => ({ ...current, name: event.target.value }));
                    setCreateValidation(undefined);
                  }}
                />
              </Field>
              <Field label={t("domainRouteMode")}>
                <Select
                  disabled={operation === "create"}
                  value={createDraft.routeMode}
                  onChange={(event) => {
                    setCreateDraft((current) => ({
                      ...current,
                      routeMode: event.target.value as RouteMode | "",
                    }));
                    setCreateValidation(undefined);
                  }}
                >
                  <option value="">{t("domainRouteMode")}</option>
                  <option value="direct_mx">{t("domainDirectMx")}</option>
                  <option value="smart_host">{t("domainSmartHost")}</option>
                </Select>
              </Field>
            </div>
            <div className="dialog__actions">
              <Button
                disabled={operation === "create"}
                type="button"
                variant="secondary"
                onClick={() => setDialog(undefined)}
              >
                {t("cancel")}
              </Button>
              <Button disabled={operation === "create"} type="submit">
                {t("domainRegisterSubmit")}
              </Button>
            </div>
          </form>
        </Dialog>
      ) : null}

      {tokenDialog ? (
        <OneTimeTokenDialog
          t={t}
          token={tokenDialog}
          onClose={() => {
            setOneTimeToken(undefined);
            setDialog(undefined);
          }}
        />
      ) : null}

      {dialog === "rotate" && selected ? (
        <ConfirmDialog
          busy={operation === "rotate"}
          cancelLabel={t("cancel")}
          closeLabel={t("closeDialog")}
          confirmLabel={t("domainRotateChallenge")}
          description={t("domainTokenWarning")}
          title={t("domainRotateChallenge")}
          onCancel={() => {
            if (operation !== "rotate") setDialog(undefined);
          }}
          onConfirm={rotateChallenge}
        >
          <code>{selected.name}</code>
        </ConfirmDialog>
      ) : null}

      {dialog === "activate" && selected ? (
        <ConfirmDialog
          busy={operation === "activate"}
          cancelLabel={t("cancel")}
          closeLabel={t("closeDialog")}
          confirmLabel={t("domainActivate")}
          description={t("domainConfirmActivate")}
          title={t("domainActivate")}
          onCancel={() => {
            if (operation !== "activate") setDialog(undefined);
          }}
          onConfirm={activateDomain}
        >
          <code>{selected.name}</code>
        </ConfirmDialog>
      ) : null}

      {dialog === "suspend" && selected ? (
        <ConfirmDialog
          busy={operation === "suspend"}
          cancelLabel={t("cancel")}
          closeLabel={t("closeDialog")}
          confirmLabel={t("domainSuspend")}
          description={t("domainConfirmSuspend")}
          title={t("domainSuspend")}
          onCancel={() => {
            if (operation !== "suspend") setDialog(undefined);
          }}
          onConfirm={suspendDomain}
        >
          <div className="form-grid">
            {suspendValidation ? <p role="alert">{suspendValidation === "required"
              ? t("domainRequired")
              : t("domainInvalidSuspendReason")}</p> : null}
            <Field label={t("domainSuspendReason")}>
              <Input
                disabled={operation === "suspend"}
                autoComplete="off"
                maxLength={100}
                value={suspendReason}
                onChange={(event) => {
                  setSuspendReason(event.target.value);
                  setSuspendValidation(undefined);
                }}
              />
            </Field>
          </div>
        </ConfirmDialog>
      ) : null}
    </section>
  );
}
