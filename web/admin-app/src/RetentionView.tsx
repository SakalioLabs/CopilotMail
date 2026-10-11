import {
  Button,
  ConfirmDialog,
  ContextBar,
  Dialog,
  EvidenceInspector,
  Field,
  Input,
  Panel,
  Select,
  Spinner,
  StatusIndicator,
  Textarea,
  UnknownWriteLock,
} from "@copilotmail/design-system";
import { useRef, useState, type FormEvent } from "react";
import {
  AdminErrorNotice,
  AdminUiError,
  adminErrorPresentation,
} from "./AdminErrorNotice";
import { isUnknownWriteFailure, type ControlPlaneClient } from "./api";
import type { translator } from "./i18n";
import type {
  Locale,
  RetentionPolicy,
  RetentionResourceKind,
} from "./types";

type Translate = ReturnType<typeof translator>;

interface RetentionVersionDraft {
  minimumDays: string;
  archiveDays: string;
  deleteDays: string;
  rule: string;
}

type OptionalRetentionMode = "" | "not-configured" | "after-days";
type DisposalRetentionMode = "" | "indefinite" | "after-days";

interface RetentionCreateDraft {
  policyKey: string;
  displayName: string;
  resourceKind: RetentionResourceKind | "";
  minimumDays: string;
  archiveMode: OptionalRetentionMode;
  archiveDays: string;
  deleteMode: DisposalRetentionMode;
  deleteDays: string;
  rule: string;
}

interface RetentionCreateCommand {
  policy_key: string;
  display_name: string;
  resource_kind: RetentionResourceKind;
  minimum_retention_days: number;
  archive_after_days: number | null;
  delete_after_days: number | null;
  rule: Record<string, unknown>;
}

const emptyCreateDraft = (): RetentionCreateDraft => ({
  policyKey: "",
  displayName: "",
  resourceKind: "",
  minimumDays: "",
  archiveMode: "",
  archiveDays: "",
  deleteMode: "",
  deleteDays: "",
  rule: "",
});

type Operation = "load" | "create" | "append" | "activate";

interface ConfirmedRetentionWrite {
  policyId: string;
  kind: "append" | "activate";
  version: RetentionPolicy["latest_version"];
}

interface ActivationReview {
  policyId: string;
  policyName: string;
  resourceKind: RetentionResourceKind;
  activeVersion: RetentionPolicy["active_version"];
  latestVersion: RetentionPolicy["latest_version"];
  approvalRequestId: string;
}

type UnknownRetentionWrite =
  | {
    kind: "create";
    command: RetentionCreateCommand;
  }
  | {
    kind: "append";
    policyId: string;
    previousLatest: RetentionPolicy["latest_version"];
    command: Omit<RetentionCreateCommand, "policy_key" | "display_name" | "resource_kind">;
  }
  | {
    kind: "activate";
    policyId: string;
    version: RetentionPolicy["latest_version"];
    approvalRequestId: string;
  };

interface RetentionVersionHighWater {
  version: number;
  versionId: string;
}

interface RetentionPolicyHighWater {
  latest: RetentionVersionHighWater;
  active?: RetentionVersionHighWater;
  versionIds: ReadonlyMap<number, string>;
}

function versionHighWater(
  version: RetentionPolicy["latest_version"],
): RetentionVersionHighWater {
  return {
    version: version.version,
    versionId: version.retention_policy_version_id,
  };
}

function versionIsAtOrAbove(
  observed: RetentionVersionHighWater,
  boundary: RetentionVersionHighWater,
): boolean {
  return observed.version > boundary.version || (
    observed.version === boundary.version
    && observed.versionId === boundary.versionId
  );
}

function snapshotIsAtOrAbove(
  snapshot: RetentionPolicy,
  boundary: RetentionPolicyHighWater | undefined,
): boolean {
  const latest = versionHighWater(snapshot.latest_version);
  const active = snapshot.active_version
    ? versionHighWater(snapshot.active_version)
    : undefined;
  if (
    active
    && (
      active.version > latest.version
      || (
        active.version === latest.version
        && active.versionId !== latest.versionId
      )
    )
  ) return false;
  if (!boundary) return true;
  if (
    boundary.versionIds.get(latest.version) !== undefined
    && boundary.versionIds.get(latest.version) !== latest.versionId
  ) return false;
  if (
    active
    && boundary.versionIds.get(active.version) !== undefined
    && boundary.versionIds.get(active.version) !== active.versionId
  ) return false;
  if (!versionIsAtOrAbove(latest, boundary.latest)) return false;
  return !boundary.active || Boolean(
    active && versionIsAtOrAbove(active, boundary.active),
  );
}

function mergeHighWater(
  snapshot: RetentionPolicy,
  current: RetentionPolicyHighWater | undefined,
): RetentionPolicyHighWater {
  const latest = versionHighWater(snapshot.latest_version);
  const active = snapshot.active_version
    ? versionHighWater(snapshot.active_version)
    : undefined;
  const versionIds = new Map(current?.versionIds);
  versionIds.set(latest.version, latest.versionId);
  if (active) versionIds.set(active.version, active.versionId);
  return {
    latest: current && current.latest.version >= latest.version
      ? current.latest
      : latest,
    active: !active
      ? current?.active
      : current?.active && current.active.version >= active.version
        ? current.active
        : active,
    versionIds,
  };
}

function formatDate(value: string, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function versionDays(value: string, required: true): number;
function versionDays(value: string, required: false): number | null;
function versionDays(value: string, required: boolean): number | null {
  const normalized = value.trim();
  if (!normalized) {
    if (required) {
      throw new AdminUiError("retention_minimum_days_required");
    }
    return null;
  }
  const parsed = Number(normalized);
  if (!Number.isInteger(parsed) || parsed < 1 || parsed > 36500) {
    throw new AdminUiError("retention_days_out_of_range");
  }
  return parsed;
}

function parseRule(value: string): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value) as unknown;
  } catch {
    throw new AdminUiError("retention_rule_invalid_json");
  }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new AdminUiError("retention_rule_must_be_an_object");
  }
  return parsed as Record<string, unknown>;
}

function parseCreateRule(value: string): Record<string, unknown> {
  return value.trim() ? parseRule(value) : {};
}

function versionDraftFrom(
  version: RetentionPolicy["latest_version"],
): RetentionVersionDraft {
  return {
    minimumDays: String(version.minimum_retention_days),
    archiveDays: version.archive_after_days === null
      ? ""
      : String(version.archive_after_days),
    deleteDays: version.delete_after_days === null
      ? ""
      : String(version.delete_after_days),
    rule: JSON.stringify(version.rule, null, 2),
  };
}

function VersionCard({
  title,
  version,
  locale,
  t,
}: {
  title: string;
  version: RetentionPolicy["latest_version"] | null;
  locale: Locale;
  t: Translate;
}) {
  return (
    <div className="retention-version">
      <strong>{title}</strong>
      {version ? (
        <>
          <p>v{version.version} · {version.minimum_retention_days} {t("days")}</p>
          <small>
            {t("archiveAfter")}: {version.archive_after_days ?? t("notConfigured")} ·{" "}
            {t("deleteAfter")}: {version.delete_after_days ?? t("indefinite")}
          </small>
          <small>{formatDate(version.created_at, locale)}</small>
          <code>{version.retention_policy_version_id}</code>
        </>
      ) : <p>{t("noActiveVersion")}</p>}
    </div>
  );
}

function VersionEvidence({
  label,
  version,
  locale,
  t,
}: {
  label: string;
  version: RetentionPolicy["latest_version"] | null;
  locale: Locale;
  t: Translate;
}) {
  if (!version) return null;
  return (
    <section className="retention-evidence-version" aria-label={label}>
      <h3>{label}</h3>
      <dl className="retention-evidence-facts">
        <dt>{t("retentionVersionId")}</dt>
        <dd><code>{version.retention_policy_version_id}</code></dd>
        <dt>{t("retentionVersionNumber")}</dt>
        <dd>v{version.version}</dd>
        <dt>{t("createdBy")}</dt>
        <dd><code>{version.created_by}</code></dd>
        <dt>{t("createdAt")}</dt>
        <dd>{formatDate(version.created_at, locale)}</dd>
        <dt>{t("scopeRule")}</dt>
        <dd><code>{JSON.stringify(version.rule)}</code></dd>
      </dl>
    </section>
  );
}

function RetentionEvidence({
  policy,
  locale,
  t,
}: {
  policy: RetentionPolicy;
  locale: Locale;
  t: Translate;
}) {
  return (
    <EvidenceInspector className="retention-evidence" summary={t("retentionEvidence")}>
      <dl className="retention-evidence-facts">
        <dt>{t("retentionPolicyId")}</dt>
        <dd><code>{policy.retention_policy_id}</code></dd>
        <dt>{t("organizationId")}</dt>
        <dd><code>{policy.organization_id}</code></dd>
        <dt>{t("createdBy")}</dt>
        <dd><code>{policy.created_by}</code></dd>
        <dt>{t("createdAt")}</dt>
        <dd>{formatDate(policy.created_at, locale)}</dd>
      </dl>
      <VersionEvidence
        label={t("activeVersion")}
        locale={locale}
        t={t}
        version={policy.active_version}
      />
      <VersionEvidence
        label={t("latestVersion")}
        locale={locale}
        t={t}
        version={policy.latest_version}
      />
    </EvidenceInspector>
  );
}

function RetentionCreateSummary({
  command,
  t,
}: {
  command: RetentionCreateCommand;
  t: Translate;
}) {
  return (
    <dl className="retention-create-summary">
      <dt>{t("policyKey")}</dt>
      <dd><code>{command.policy_key}</code></dd>
      <dt>{t("displayName")}</dt>
      <dd>{command.display_name}</dd>
      <dt>{t("resourceKind")}</dt>
      <dd>{t(command.resource_kind)}</dd>
      <dt>{t("minimumRetention")}</dt>
      <dd>{command.minimum_retention_days} {t("days")}</dd>
      <dt>{t("archiveAfter")}</dt>
      <dd>{command.archive_after_days === null
        ? t("notConfigured")
        : `${command.archive_after_days} ${t("days")}`}</dd>
      <dt>{t("deleteAfter")}</dt>
      <dd>{command.delete_after_days === null
        ? t("indefinite")
        : `${command.delete_after_days} ${t("days")}`}</dd>
      <dt>{t("scopeRule")}</dt>
      <dd><code>{JSON.stringify(command.rule)}</code></dd>
    </dl>
  );
}

function RetentionVersionCommandSummary({
  command,
  t,
}: {
  command: Omit<
    RetentionCreateCommand,
    "policy_key" | "display_name" | "resource_kind"
  >;
  t: Translate;
}) {
  return (
    <dl className="retention-create-summary">
      <dt>{t("minimumRetention")}</dt>
      <dd>{command.minimum_retention_days} {t("days")}</dd>
      <dt>{t("archiveAfter")}</dt>
      <dd>{command.archive_after_days === null
        ? t("notConfigured")
        : `${command.archive_after_days} ${t("days")}`}</dd>
      <dt>{t("deleteAfter")}</dt>
      <dd>{command.delete_after_days === null
        ? t("indefinite")
        : `${command.delete_after_days} ${t("days")}`}</dd>
      <dt>{t("scopeRule")}</dt>
      <dd><code>{JSON.stringify(command.rule)}</code></dd>
    </dl>
  );
}

function CreatePolicyErrorNotice({
  error,
  t,
}: {
  error: unknown;
  t: Translate;
}) {
  const presentation = adminErrorPresentation(error);
  return (
    <div className="notice notice--error" role="alert">
      <div>
        <strong>{t("retentionCreateNotSubmitted")}</strong>
        <p><code>{presentation.code}</code></p>
        {presentation.requestId ? (
          <small>{t("requestId")}: {presentation.requestId}</small>
        ) : null}
      </div>
    </div>
  );
}

export default function RetentionView({
  client,
  locale,
  t,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: Translate;
}) {
  const [policyId, setPolicyId] = useState("");
  const [policy, setPolicy] = useState<RetentionPolicy>();
  const [createDialog, setCreateDialog] = useState<"edit" | "confirm">();
  const [createDraft, setCreateDraft] =
    useState<RetentionCreateDraft>(emptyCreateDraft);
  const [createCommand, setCreateCommand] =
    useState<RetentionCreateCommand>();
  const [createError, setCreateError] = useState<unknown>();
  const [createdPolicyEvidence, setCreatedPolicyEvidence] =
    useState<RetentionPolicy>();
  const [versionDraft, setVersionDraft] =
    useState<RetentionVersionDraft>();
  const [approvalId, setApprovalId] = useState("");
  const [pendingActivation, setPendingActivation] = useState<ActivationReview>();
  const [snapshotFresh, setSnapshotFresh] = useState(false);
  const [confirmedWrite, setConfirmedWrite] =
    useState<ConfirmedRetentionWrite>();
  const [unknownWrite, setUnknownWrite] = useState<UnknownRetentionWrite>();
  const [operation, setOperation] = useState<Operation>();
  const operationRef = useRef<Operation | undefined>(undefined);
  const createPolicyButtonRef = useRef<HTMLButtonElement>(null);
  const createPolicyFirstFieldRef = useRef<HTMLInputElement>(null);
  const policyHighWaterRef = useRef(new Map<string, RetentionPolicyHighWater>());
  const [error, setError] = useState<unknown>();
  const busy = operation !== undefined;
  const writeFrozen = unknownWrite !== undefined;

  function beginOperation(next: Operation): boolean {
    if (operationRef.current !== undefined) return false;
    operationRef.current = next;
    setOperation(next);
    setError(undefined);
    return true;
  }

  function finishOperation(current: Operation) {
    if (operationRef.current !== current) return;
    operationRef.current = undefined;
    setOperation(undefined);
  }

  function bindAcceptedPolicy(result: RetentionPolicy) {
    const current = policyHighWaterRef.current.get(result.retention_policy_id);
    policyHighWaterRef.current.set(
      result.retention_policy_id,
      mergeHighWater(result, current),
    );
    if (
      policy?.retention_policy_id === result.retention_policy_id
      && policy.latest_version.retention_policy_version_id
        !== result.latest_version.retention_policy_version_id
    ) setApprovalId("");
    setPolicy(result);
    setPolicyId(result.retention_policy_id);
    setVersionDraft(versionDraftFrom(result.latest_version));
    setSnapshotFresh(true);
    setConfirmedWrite(undefined);
  }

  function bindMonotonicPolicy(
    result: RetentionPolicy,
    confirmed: ConfirmedRetentionWrite | undefined,
    expectedPolicyId = result.retention_policy_id,
  ) {
    const boundary = policyHighWaterRef.current.get(expectedPolicyId);
    if (
      result.retention_policy_id !== expectedPolicyId
      || !snapshotIsAtOrAbove(result, boundary)
    ) {
      setSnapshotFresh(false);
      if (confirmed) setConfirmedWrite(confirmed);
      throw new AdminUiError("retention_snapshot_stale");
    }
    bindAcceptedPolicy(result);
  }

  function advanceHighWaterFromWrite(confirmed: ConfirmedRetentionWrite) {
    const observed = versionHighWater(confirmed.version);
    const current = policyHighWaterRef.current.get(confirmed.policyId);
    const knownVersionId = current?.versionIds.get(observed.version);
    if (knownVersionId !== undefined && knownVersionId !== observed.versionId) {
      throw new AdminUiError("retention_snapshot_stale");
    }
    if (current && !versionIsAtOrAbove(observed, current.latest)) {
      throw new AdminUiError("retention_snapshot_stale");
    }
    if (
      confirmed.kind === "activate"
      && current?.active
      && !versionIsAtOrAbove(observed, current.active)
    ) {
      throw new AdminUiError("retention_snapshot_stale");
    }
    const versionIds = new Map(current?.versionIds);
    versionIds.set(observed.version, observed.versionId);
    policyHighWaterRef.current.set(confirmed.policyId, {
      latest: !current || observed.version > current.latest.version
        ? observed
        : current.latest,
      active: confirmed.kind === "activate"
        ? (!current?.active || observed.version > current.active.version
            ? observed
            : current.active)
        : current?.active,
      versionIds,
    });
  }

  async function load(
    id = policyId,
    reconciliation?: Exclude<UnknownRetentionWrite, { kind: "create" }>,
  ) {
    const requestedId = id.trim();
    if (!requestedId || !beginOperation("load")) return;
    const confirmedBoundary = confirmedWrite?.policyId === requestedId
      ? confirmedWrite
      : undefined;
    if (policy?.retention_policy_id !== requestedId) {
      setPolicy(undefined);
      setVersionDraft(undefined);
      setApprovalId("");
      setSnapshotFresh(false);
      setConfirmedWrite(undefined);
    }
    try {
      const result = await client.getRetentionPolicy(requestedId);
      bindMonotonicPolicy(result, confirmedBoundary, requestedId);
      if (
        reconciliation?.kind === "activate"
        && result.active_version?.retention_policy_version_id
          === reconciliation.version.retention_policy_version_id
      ) {
        setUnknownWrite(undefined);
      }
    } catch (cause) {
      setError(cause);
    } finally {
      finishOperation("load");
    }
  }

  function openCreatePolicy() {
    if (operationRef.current !== undefined || writeFrozen) return;
    setCreateDraft(emptyCreateDraft());
    setCreateCommand(undefined);
    setCreateError(undefined);
    setCreateDialog("edit");
  }

  function closeCreatePolicy() {
    if (operationRef.current === "create") return;
    setCreateDialog(undefined);
    setCreateCommand(undefined);
    setCreateError(undefined);
  }

  function updateCreateDraft(
    field: keyof RetentionCreateDraft,
    value: string,
  ) {
    setCreateDraft((current) => ({ ...current, [field]: value }));
    setCreateError(undefined);
  }

  function reviewCreatePolicy(event: FormEvent) {
    event.preventDefault();
    if (operationRef.current !== undefined || writeFrozen) return;
    try {
      if (
        !createDraft.policyKey.trim()
        || !createDraft.displayName.trim()
        || !createDraft.resourceKind
        || !createDraft.archiveMode
        || !createDraft.deleteMode
      ) throw new AdminUiError("retention_create_required");
      if (
        createDraft.archiveMode === "after-days"
        && !createDraft.archiveDays.trim()
      ) throw new AdminUiError("retention_archive_days_required");
      if (
        createDraft.deleteMode === "after-days"
        && !createDraft.deleteDays.trim()
      ) throw new AdminUiError("retention_delete_days_required");

      const command = {
        policy_key: createDraft.policyKey.trim(),
        display_name: createDraft.displayName.trim(),
        resource_kind: createDraft.resourceKind,
        minimum_retention_days: versionDays(createDraft.minimumDays, true),
        archive_after_days: createDraft.archiveMode === "not-configured"
          ? null
          : versionDays(createDraft.archiveDays, true),
        delete_after_days: createDraft.deleteMode === "indefinite"
          ? null
          : versionDays(createDraft.deleteDays, true),
        rule: parseCreateRule(createDraft.rule),
      } satisfies RetentionCreateCommand;
      setCreateCommand(command);
      setCreateError(undefined);
      setCreateDialog("confirm");
    } catch (cause) {
      setCreateError(cause);
    }
  }

  function returnToCreateEditor() {
    if (operationRef.current === "create") return;
    setCreateCommand(undefined);
    setCreateError(undefined);
    setCreateDialog("edit");
  }

  async function createPolicy() {
    const command = createCommand;
    if (!command || writeFrozen || !beginOperation("create")) return;
    try {
      let result: RetentionPolicy;
      try {
        result = await client.createRetentionPolicy(command);
      } catch (cause) {
        if (isUnknownWriteFailure(cause)) {
          setUnknownWrite({ kind: "create", command });
          setCreateDialog(undefined);
          setCreateCommand(undefined);
          setCreateError(undefined);
        } else {
          setCreateError(cause);
        }
        return;
      }

      // The POST response is the authoritative creation fact. Preserve it
      // independently before any reconciliation logic that could reject a
      // conflicting snapshot, so a successful write can never be retried as
      // though it failed.
      setCreatedPolicyEvidence(result);
      setCreateDialog(undefined);
      setCreateCommand(undefined);
      setCreateDraft(emptyCreateDraft());
      setCreateError(undefined);
      setApprovalId("");
      try {
        bindMonotonicPolicy(result, undefined);
      } catch (cause) {
        setError(cause);
      }
    } finally {
      finishOperation("create");
    }
  }

  async function appendVersion(event: FormEvent) {
    event.preventDefault();
    if (
      !policy
      || !versionDraft
      || writeFrozen
      || operationRef.current !== undefined
    ) return;
    let parsedRule: Record<string, unknown>;
    let minimumRetentionDays: number;
    let archiveAfterDays: number | null;
    let deleteAfterDays: number | null;
    try {
      parsedRule = parseRule(versionDraft.rule);
      minimumRetentionDays = versionDays(versionDraft.minimumDays, true);
      archiveAfterDays = versionDays(versionDraft.archiveDays, false);
      deleteAfterDays = versionDays(versionDraft.deleteDays, false);
    } catch (cause) {
      setError(cause);
      return;
    }
    if (!beginOperation("append")) return;
    const currentPolicy = policy;
    const command: Omit<
      RetentionCreateCommand,
      "policy_key" | "display_name" | "resource_kind"
    > = {
      minimum_retention_days: minimumRetentionDays,
      archive_after_days: archiveAfterDays,
      delete_after_days: deleteAfterDays,
      rule: parsedRule,
    };
    try {
      let appended: RetentionPolicy["latest_version"];
      try {
        appended = await client.appendRetentionPolicyVersion(
          currentPolicy.retention_policy_id,
          command,
        );
      } catch (cause) {
        if (isUnknownWriteFailure(cause)) {
          setUnknownWrite({
            kind: "append",
            policyId: currentPolicy.retention_policy_id,
            previousLatest: currentPolicy.latest_version,
            command,
          });
        } else {
          setError(cause);
        }
        return;
      }
      // The write response proves that this exact version was created, but it
      // cannot prove it is still the latest after a concurrent append. Keep it
      // as separate evidence and lock further mutations until a full snapshot
      // is read successfully.
      const confirmed = {
        policyId: currentPolicy.retention_policy_id,
        kind: "append",
        version: appended,
      } satisfies ConfirmedRetentionWrite;
      setConfirmedWrite(confirmed);
      setSnapshotFresh(false);
      advanceHighWaterFromWrite(confirmed);
      const refreshed = await client.getRetentionPolicy(
        currentPolicy.retention_policy_id,
      );
      bindMonotonicPolicy(refreshed, confirmed);
    } catch (cause) {
      // At this point a success response already committed the exact new
      // version. A failed reconciliation read makes the snapshot stale; it
      // must not be treated as an unknown POST or invite another append.
      setError(cause);
    } finally {
      finishOperation("append");
    }
  }

  function reviewActivation(event: FormEvent) {
    event.preventDefault();
    if (
      !policy
      || writeFrozen
      || !snapshotFresh
      || !approvalId.trim()
      || operationRef.current !== undefined
    ) return;
    setError(undefined);
    setPendingActivation({
      policyId: policy.retention_policy_id,
      policyName: policy.display_name,
      resourceKind: policy.resource_kind,
      activeVersion: policy.active_version,
      latestVersion: policy.latest_version,
      approvalRequestId: approvalId.trim(),
    });
  }

  async function activateLatest() {
    const review = pendingActivation;
    if (!review || writeFrozen || !beginOperation("activate")) return;
    const currentPolicy = policy;
    if (
      !currentPolicy
      || currentPolicy.retention_policy_id !== review.policyId
      || currentPolicy.latest_version.retention_policy_version_id
        !== review.latestVersion.retention_policy_version_id
    ) {
      setPendingActivation(undefined);
      setSnapshotFresh(false);
      setError(new AdminUiError("retention_snapshot_stale"));
      finishOperation("activate");
      return;
    }
    try {
      let activated: RetentionPolicy["latest_version"];
      try {
        activated = await client.activateRetentionPolicyVersion(
          currentPolicy.retention_policy_id,
          review.latestVersion.retention_policy_version_id,
          review.approvalRequestId,
        );
      } catch (cause) {
        if (isUnknownWriteFailure(cause)) {
          setUnknownWrite({
            kind: "activate",
            policyId: review.policyId,
            version: review.latestVersion,
            approvalRequestId: review.approvalRequestId,
          });
        } else {
          setError(cause);
        }
        return;
      }
      // The approval has been consumed by a successful privileged write. Clear
      // it before any fallible reconciliation read so it can never be retried.
      setApprovalId("");
      if (
        activated.version !== currentPolicy.latest_version.version
        || activated.retention_policy_version_id
          !== currentPolicy.latest_version.retention_policy_version_id
      ) {
        setSnapshotFresh(false);
        throw new AdminUiError("retention_snapshot_stale");
      }
      setPolicy((current) => current
        && current.retention_policy_id === currentPolicy.retention_policy_id
        ? { ...current, active_version: activated }
        : current);
      const confirmed = {
        policyId: currentPolicy.retention_policy_id,
        kind: "activate",
        version: activated,
      } satisfies ConfirmedRetentionWrite;
      setConfirmedWrite(confirmed);
      setSnapshotFresh(false);
      advanceHighWaterFromWrite(confirmed);
      const refreshed = await client.getRetentionPolicy(
        currentPolicy.retention_policy_id,
      );
      bindMonotonicPolicy(refreshed, confirmed);
    } catch (cause) {
      // A valid activation response is already a committed fact. Later read
      // failures leave the snapshot stale rather than reopening activation.
      setError(cause);
    } finally {
      finishOperation("activate");
      setPendingActivation(undefined);
    }
  }

  async function reconcileUnknownWrite() {
    const current = unknownWrite;
    if (!current || current.kind === "create") return;
    // Reconciliation is deliberately bound to the persisted policy ID from
    // the uncertain command. It never reads the editable lookup input.
    await load(current.policyId, current);
  }

  function updateVersionDraft(
    field: keyof RetentionVersionDraft,
    value: string,
  ) {
    setVersionDraft((current) => current
      ? { ...current, [field]: value }
      : current);
  }

  const createdPolicyCommand = createdPolicyEvidence ? {
    policy_key: createdPolicyEvidence.policy_key,
    display_name: createdPolicyEvidence.display_name,
    resource_kind: createdPolicyEvidence.resource_kind,
    minimum_retention_days:
      createdPolicyEvidence.latest_version.minimum_retention_days,
    archive_after_days: createdPolicyEvidence.latest_version.archive_after_days,
    delete_after_days: createdPolicyEvidence.latest_version.delete_after_days,
    rule: createdPolicyEvidence.latest_version.rule,
  } satisfies RetentionCreateCommand : undefined;
  const latestMatchesActive = Boolean(
    policy?.active_version
      && policy.active_version.retention_policy_version_id
        === policy.latest_version.retention_policy_version_id,
  );
  const activationRequiresDecision = Boolean(policy && !latestMatchesActive);
  const unknownPolicyId = unknownWrite?.kind === "create"
    ? undefined
    : unknownWrite?.policyId;

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1>{t("retentionTitle")}</h1>
          <p>{t("retentionHint")}</p>
        </div>
        <Button
          ref={createPolicyButtonRef}
          type="button"
          disabled={writeFrozen || (busy && operation !== "create")}
          onClick={openCreatePolicy}
        >
          {t("createDraftPolicy")}
        </Button>
      </div>

      {error ? <AdminErrorNotice error={error} t={t} /> : null}
      {unknownWrite ? (
        <UnknownWriteLock
          aria-label={t("retentionWriteOutcomeUnknown")}
          className="retention-unknown-write"
          description={unknownWrite.kind === "create"
            ? t("retentionCreateOutcomeUnknown")
            : unknownWrite.kind === "append"
              ? t("retentionAppendOutcomeUnknown")
              : t("retentionActivationOutcomeUnknown")}
          evidence={unknownWrite.kind === "create" ? (
            <RetentionCreateSummary command={unknownWrite.command} t={t} />
          ) : unknownWrite.kind === "append" ? (
            <div className="retention-write-evidence">
              <p><span>{t("retentionPolicyId")}:</span> <code>{unknownWrite.policyId}</code></p>
              <p>
                <span>{t("retentionPreviousLatestVersion")}:</span>{" "}
                v{unknownWrite.previousLatest.version}{" "}
                <code>{unknownWrite.previousLatest.retention_policy_version_id}</code>
              </p>
              <RetentionVersionCommandSummary command={unknownWrite.command} t={t} />
            </div>
          ) : (
            <dl className="retention-write-facts">
              <dt>{t("retentionPolicyId")}</dt>
              <dd><code>{unknownWrite.policyId}</code></dd>
              <dt>{t("retentionVersionId")}</dt>
              <dd><code>{unknownWrite.version.retention_policy_version_id}</code></dd>
              <dt>{t("retentionVersionNumber")}</dt>
              <dd>v{unknownWrite.version.version}</dd>
              <dt>{t("approvalId")}</dt>
              <dd><code>{unknownWrite.approvalRequestId}</code></dd>
            </dl>
          )}
          reconciliationAction={unknownPolicyId ? (
            <Button
              disabled={busy}
              type="button"
              variant="secondary"
              onClick={() => void reconcileUnknownWrite()}
            >
              {t("retentionReconcileExactPolicy")}
            </Button>
          ) : undefined}
          title={t("retentionWriteOutcomeUnknown")}
        />
      ) : null}
      {busy ? <Spinner label={t("loading")} /> : null}
      {createdPolicyEvidence && createdPolicyCommand ? (
        <div
          className="notice retention-create-evidence"
          role="status"
          aria-label={t("retentionPolicyCreated")}
        >
          <div>
            <strong>{t("retentionPolicyCreated")}</strong>
            <p>{createdPolicyEvidence.display_name}</p>
            <code>{createdPolicyEvidence.retention_policy_id}</code>
            <RetentionCreateSummary command={createdPolicyCommand} t={t} />
          </div>
        </div>
      ) : null}

      <Panel>
        <div className="retention-lookup">
          <Field label={t("retentionPolicyId")} hint={t("retentionPolicyIdHint")}>
            <Input
              disabled={busy || writeFrozen}
              value={policyId}
              onChange={(event) => setPolicyId(event.target.value)}
            />
          </Field>
          {unknownPolicyId ? (
            <Button
              type="button"
              variant="secondary"
              disabled={busy}
              onClick={() => void reconcileUnknownWrite()}
            >
              {t("retentionReconcileExactPolicy")}
            </Button>
          ) : (
            <Button
              type="button"
              variant="secondary"
              disabled={busy || writeFrozen || !policyId.trim()}
              onClick={() => void load()}
            >
              {t("loadPolicy")}
            </Button>
          )}
        </div>
      </Panel>

      {policy ? (
        <>
          <ContextBar
            className="retention-context"
            facts={[
              {
                id: "policy-key",
                label: t("policyKey"),
                value: <code>{policy.policy_key}</code>,
              },
              {
                id: "resource-kind",
                label: t("resourceKind"),
                value: t(policy.resource_kind),
              },
              {
                id: "policy-id",
                label: t("retentionPolicyId"),
                value: <code>{policy.retention_policy_id}</code>,
              },
              {
                id: "organization-id",
                label: t("organizationId"),
                value: <code>{policy.organization_id}</code>,
              },
              {
                id: "created-at",
                label: t("createdAt"),
                value: formatDate(policy.created_at, locale),
              },
            ]}
            status={(
              <StatusIndicator
                tone={activationRequiresDecision ? "warning" : "positive"}
                label={activationRequiresDecision
                  ? t("retentionStatusActivationRequired")
                  : t("retentionStatusCurrentVersionActive")}
              />
            )}
            title={policy.display_name}
          />

          <Panel title={t("retentionVersionFacts")}>
            <div className="retention-versions">
              <VersionCard
                title={t("activeVersion")}
                version={policy.active_version}
                locale={locale}
                t={t}
              />
              <VersionCard
                title={snapshotFresh ? t("latestVersion") : t("lastConfirmedLatestVersion")}
                version={policy.latest_version}
                locale={locale}
                t={t}
              />
            </div>
            {confirmedWrite ? (
              <div className="notice retention-refresh-required" role="status">
                <div>
                  <strong>{t("retentionWriteConfirmed")}</strong>
                  <p>
                    {confirmedWrite.kind === "append"
                      ? t("retentionCreatedVersion")
                      : t("retentionActivatedVersion")} v{confirmedWrite.version.version}
                  </p>
                  <small>{t("retentionRefreshRequired")}</small>
                </div>
              </div>
            ) : null}
          </Panel>

          <Panel title={t("retentionCurrentDecision")}>
            <div className="retention-decision-layout">
              {versionDraft ? (
                <section
                  className="retention-decision-section"
                  data-primary={!activationRequiresDecision || undefined}
                  aria-labelledby="retention-version-editor-title"
                >
                  <div className="retention-decision-heading">
                    <h3 id="retention-version-editor-title">{t("appendDraft")}</h3>
                    <p>{t("retentionAppendDecisionHint")}</p>
                  </div>
                  <form
                    className="retention-version-editor"
                    aria-label={t("appendDraft")}
                    onSubmit={(event) => void appendVersion(event)}
                  >
                    <div className="retention-form">
                      <Field label={t("minimumRetention")}>
                        <Input
                          type="number"
                          min="1"
                          max="36500"
                          disabled={busy || writeFrozen || !snapshotFresh}
                          value={versionDraft.minimumDays}
                          onChange={(event) => updateVersionDraft("minimumDays", event.target.value)}
                          required
                        />
                      </Field>
                      <Field label={t("archiveAfter")} hint={t("retentionArchiveBlankHint")}>
                        <Input
                          type="number"
                          min="1"
                          max="36500"
                          disabled={busy || writeFrozen || !snapshotFresh}
                          value={versionDraft.archiveDays}
                          onChange={(event) => updateVersionDraft("archiveDays", event.target.value)}
                        />
                      </Field>
                      <Field label={t("deleteAfter")} hint={t("retentionDeleteBlankHint")}>
                        <Input
                          type="number"
                          min="1"
                          max="36500"
                          disabled={busy || writeFrozen || !snapshotFresh}
                          value={versionDraft.deleteDays}
                          onChange={(event) => updateVersionDraft("deleteDays", event.target.value)}
                        />
                      </Field>
                      <Field label={t("scopeRule")} hint={t("retentionScopeRuleEditHint")}>
                        <Textarea
                          rows={3}
                          disabled={busy || writeFrozen || !snapshotFresh}
                          value={versionDraft.rule}
                          onChange={(event) => updateVersionDraft("rule", event.target.value)}
                        />
                      </Field>
                    </div>
                    <div className="retention-submit">
                      <Button
                        type="submit"
                        variant={activationRequiresDecision ? "secondary" : "primary"}
                        disabled={busy || writeFrozen || !snapshotFresh}
                      >
                        {t("appendDraft")}
                      </Button>
                    </div>
                  </form>
                </section>
              ) : null}

              <section
                className="retention-decision-section"
                data-primary={activationRequiresDecision || undefined}
                aria-labelledby="retention-activation-title"
              >
                <div className="retention-decision-heading">
                  <h3 id="retention-activation-title">{t("activateLatest")}</h3>
                  <p>{t("retentionActivationDecisionHint")}</p>
                </div>
                <form className="retention-activation-form" onSubmit={reviewActivation}>
                  <Field label={t("approvalId")} hint={t("retentionApprovalIdHint")}>
                    <Input
                      disabled={busy || writeFrozen || !snapshotFresh}
                      value={approvalId}
                      onChange={(event) => setApprovalId(event.target.value)}
                      required
                    />
                  </Field>
                  <Button
                    type="submit"
                    variant={activationRequiresDecision ? "primary" : "secondary"}
                    disabled={busy || writeFrozen || !snapshotFresh || !approvalId.trim()}
                  >
                    {t("reviewRetentionActivation")}
                  </Button>
                </form>
              </section>
            </div>
          </Panel>

          <RetentionEvidence locale={locale} policy={policy} t={t} />
        </>
      ) : null}

      {pendingActivation ? (
        <ConfirmDialog
          busy={operation === "activate"}
          cancelLabel={t("cancel")}
          closeLabel={t("closeDialog")}
          confirmLabel={t("activateLatest")}
          confirmVariant="primary"
          description={t("confirmRetentionActivation")}
          title={t("reviewRetentionActivation")}
          onCancel={() => {
            if (operation !== "activate") setPendingActivation(undefined);
          }}
          onConfirm={() => void activateLatest()}
        >
          <dl className="retention-confirmation-facts">
            <dt>{t("retentionPolicyId")}</dt>
            <dd><code>{pendingActivation.policyId}</code></dd>
            <dt>{t("displayName")}</dt>
            <dd>{pendingActivation.policyName}</dd>
            <dt>{t("resourceKind")}</dt>
            <dd>{t(pendingActivation.resourceKind)}</dd>
            <dt>{t("activeVersion")}</dt>
            <dd>{pendingActivation.activeVersion
              ? `v${pendingActivation.activeVersion.version}`
              : t("noActiveVersion")}</dd>
            <dt>{t("latestVersion")}</dt>
            <dd>
              v{pendingActivation.latestVersion.version}{" "}
              <code>{pendingActivation.latestVersion.retention_policy_version_id}</code>
            </dd>
            <dt>{t("approvalId")}</dt>
            <dd><code>{pendingActivation.approvalRequestId}</code></dd>
          </dl>
        </ConfirmDialog>
      ) : null}

      {createDialog ? (
        <Dialog
          busy={operation === "create"}
          closeLabel={t("closeDialog")}
          description={createDialog === "edit"
            ? t("retentionCreateHint")
            : t("retentionCreateReviewHint")}
          dismissDisabled={operation === "create"}
          initialFocusRef={createPolicyFirstFieldRef}
          returnFocusRef={createPolicyButtonRef}
          title={createDialog === "edit"
            ? t("createDraftPolicy")
            : t("retentionCreateReviewTitle")}
          onClose={closeCreatePolicy}
        >
          {createDialog === "edit" ? (
            <form noValidate onSubmit={reviewCreatePolicy}>
              <div className="dialog__body retention-create-form">
                {createError ? <CreatePolicyErrorNotice error={createError} t={t} /> : null}
                <Field label={t("policyKey")}>
                  <Input
                    ref={createPolicyFirstFieldRef}
                    value={createDraft.policyKey}
                    maxLength={100}
                    required
                    onChange={(event) => updateCreateDraft("policyKey", event.target.value)}
                  />
                </Field>
                <Field label={t("displayName")}>
                  <Input
                    value={createDraft.displayName}
                    maxLength={200}
                    required
                    onChange={(event) => updateCreateDraft("displayName", event.target.value)}
                  />
                </Field>
                <Field label={t("resourceKind")}>
                  <Select
                    value={createDraft.resourceKind}
                    required
                    onChange={(event) => updateCreateDraft("resourceKind", event.target.value)}
                  >
                    <option value="" disabled>{t("chooseResourceKind")}</option>
                    <option value="audit_evidence">{t("audit_evidence")}</option>
                    <option value="mail_content">{t("mail_content")}</option>
                  </Select>
                </Field>
                <Field label={t("minimumRetention")}>
                  <Input
                    type="number"
                    min="1"
                    max="36500"
                    value={createDraft.minimumDays}
                    required
                    onChange={(event) => updateCreateDraft("minimumDays", event.target.value)}
                  />
                </Field>
                <Field label={t("archiveConfiguration")}>
                  <Select
                    value={createDraft.archiveMode}
                    required
                    onChange={(event) => updateCreateDraft("archiveMode", event.target.value)}
                  >
                    <option value="" disabled>{t("chooseRetentionConfiguration")}</option>
                    <option value="not-configured">{t("notConfigured")}</option>
                    <option value="after-days">{t("afterSpecifiedDays")}</option>
                  </Select>
                </Field>
                {createDraft.archiveMode === "after-days" ? (
                  <Field label={t("archiveAfter")}>
                    <Input
                      type="number"
                      min="1"
                      max="36500"
                      value={createDraft.archiveDays}
                      required
                      onChange={(event) => updateCreateDraft("archiveDays", event.target.value)}
                    />
                  </Field>
                ) : null}
                <Field label={t("deleteConfiguration")}>
                  <Select
                    value={createDraft.deleteMode}
                    required
                    onChange={(event) => updateCreateDraft("deleteMode", event.target.value)}
                  >
                    <option value="" disabled>{t("chooseRetentionConfiguration")}</option>
                    <option value="indefinite">{t("indefinite")}</option>
                    <option value="after-days">{t("afterSpecifiedDays")}</option>
                  </Select>
                </Field>
                {createDraft.deleteMode === "after-days" ? (
                  <Field label={t("deleteAfter")}>
                    <Input
                      type="number"
                      min="1"
                      max="36500"
                      value={createDraft.deleteDays}
                      required
                      onChange={(event) => updateCreateDraft("deleteDays", event.target.value)}
                    />
                  </Field>
                ) : null}
                <Field label={t("scopeRule")} hint={t("retentionScopeRuleHint")}>
                  <Textarea
                    rows={4}
                    value={createDraft.rule}
                    onChange={(event) => updateCreateDraft("rule", event.target.value)}
                  />
                </Field>
              </div>
              <div className="dialog__actions">
                <Button type="button" variant="secondary" onClick={closeCreatePolicy}>
                  {t("cancel")}
                </Button>
                <Button type="submit">{t("reviewPolicyDraft")}</Button>
              </div>
            </form>
          ) : createCommand ? (
            <form onSubmit={(event) => {
              event.preventDefault();
              void createPolicy();
            }}>
              <div className="dialog__body retention-create-review">
                {createError ? <CreatePolicyErrorNotice error={createError} t={t} /> : null}
                <RetentionCreateSummary command={createCommand} t={t} />
              </div>
              <div className="dialog__actions">
                <Button
                  type="button"
                  variant="secondary"
                  disabled={operation === "create"}
                  onClick={returnToCreateEditor}
                >
                  {t("backToEdit")}
                </Button>
                <Button type="submit" disabled={operation === "create"}>
                  {t("confirmCreatePolicy")}
                </Button>
              </div>
            </form>
          ) : null}
        </Dialog>
      ) : null}
    </>
  );
}
