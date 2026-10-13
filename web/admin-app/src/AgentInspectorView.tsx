import {
  type BadgeTone,
  Button,
  ContextBar,
  Field,
  IdentityBadge,
  Input,
  Panel,
  Spinner,
  StatusIndicator,
} from "@copilotmail/design-system";
import { useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { Fact } from "./AdminListDetail";
import { ApiError, type ControlPlaneClient } from "./api";
import { formatEvidenceDate as formatDate, isCanonicalUuid } from "./format";
import { adminErrorPresentation } from "./AdminErrorNotice";
import AgentMailSubmissionEvidenceView from "./AgentMailSubmissionEvidenceView";
import type { translator } from "./i18n";
import type {
  AgentEnvironment,
  AgentInspector,
  AgentInspectorMailAdmissionState,
  AgentInspectorReviewState,
  AgentInspectorTokenAccessState,
  AgentRiskLevel,
  Locale,
  PrincipalStatus,
} from "./types";

type Translate = ReturnType<typeof translator>;
type InspectorFailure = "invalid" | "forbidden" | "not-found" | "unavailable" | "unknown";
type Completion = "loaded" | "refreshed";

interface InspectorError {
  kind: InspectorFailure;
  requestId?: string;
  retryable: boolean;
}



function optionalDate(
  value: string | null,
  emptyKey:
    | "agentInspectorNoCredentialExpiry"
    | "agentInspectorNoAuthentication"
    | "agentInspectorNoProcessingActivity",
  locale: Locale,
  t: Translate,
): string {
  return value ? formatDate(value, locale) : t(emptyKey);
}

function principalStatusLabel(status: PrincipalStatus, t: Translate): string {
  switch (status) {
    case "draft": return t("agentInspectorDraft");
    case "active": return t("agentInspectorActive");
    case "suspended": return t("agentInspectorSuspended");
    case "retired": return t("agentInspectorRetired");
  }
}

function principalStatusTone(status: PrincipalStatus): BadgeTone {
  switch (status) {
    case "active": return "positive";
    case "suspended": return "warning";
    case "draft":
    case "retired": return "neutral";
  }
}

function environmentLabel(value: AgentEnvironment, t: Translate): string {
  switch (value) {
    case "development": return t("agentInspectorDevelopment");
    case "testing": return t("agentInspectorTesting");
    case "production": return t("agentInspectorProduction");
  }
}

function riskLabel(value: AgentRiskLevel, t: Translate): string {
  switch (value) {
    case "low": return t("agentInspectorLow");
    case "medium": return t("agentInspectorMedium");
    case "high": return t("agentInspectorHigh");
  }
}

function riskTone(value: AgentRiskLevel): BadgeTone {
  switch (value) {
    case "low": return "positive";
    case "medium": return "warning";
    case "high": return "critical";
  }
}

function reviewLabel(value: AgentInspectorReviewState, t: Translate): string {
  return value === "due" ? t("agentInspectorDue") : t("agentInspectorScheduled");
}

function reviewTone(value: AgentInspectorReviewState): BadgeTone {
  return value === "due" ? "critical" : "neutral";
}

function tokenAccessLabel(value: AgentInspectorTokenAccessState, t: Translate): string {
  switch (value) {
    case "identity_inactive": return t("agentInspectorIdentityInactive");
    case "no_active_credential": return t("agentInspectorNoActiveCredential");
    case "no_effective_scope": return t("agentInspectorNoEffectiveScope");
    case "effective": return t("agentInspectorEffective");
  }
}

function tokenAccessTone(value: AgentInspectorTokenAccessState): BadgeTone {
  switch (value) {
    case "effective": return "positive";
    case "identity_inactive": return "neutral";
    case "no_active_credential":
    case "no_effective_scope": return "warning";
  }
}

function mailAdmissionLabel(value: AgentInspectorMailAdmissionState, t: Translate): string {
  switch (value) {
    case "not_required": return t("agentInspectorNotRequired");
    case "blocked_no_active_policy": return t("agentInspectorBlockedNoActivePolicy");
    case "active": return t("agentInspectorMailActive");
  }
}

function mailAdmissionTone(value: AgentInspectorMailAdmissionState): BadgeTone {
  switch (value) {
    case "active": return "positive";
    case "blocked_no_active_policy": return "critical";
    case "not_required": return "neutral";
  }
}

function toInspectorError(error: unknown): InspectorError {
  if (!(error instanceof ApiError)) return { kind: "unknown", retryable: false };
  const presentation = adminErrorPresentation(error);
  if (error.status === 403) {
    return { kind: "forbidden", requestId: presentation.requestId, retryable: false };
  }
  if (error.status === 404) {
    return { kind: "not-found", requestId: presentation.requestId, retryable: false };
  }
  if (error.status === 503) {
    return {
      kind: "unavailable",
      requestId: presentation.requestId,
      retryable: presentation.retryable,
    };
  }
  return {
    kind: "unknown",
    requestId: presentation.requestId,
    retryable: presentation.retryable,
  };
}

function InspectorErrorNotice({
  error,
  retry,
  busy,
  snapshot,
  locale,
  t,
}: {
  error: InspectorError;
  retry: () => void;
  busy: boolean;
  snapshot?: AgentInspector;
  locale: Locale;
  t: Translate;
}) {
  if (error.kind === "invalid") return null;

  let title = t("agentInspectorRequestFailed");
  let hint = t("agentInspectorRequestFailedHint");
  if (error.kind === "forbidden") {
    title = t("agentInspectorAccessDenied");
    hint = t("agentInspectorAccessDeniedHint");
  } else if (error.kind === "not-found") {
    title = t("agentInspectorTargetUnavailable");
    hint = t("agentInspectorTargetUnavailableHint");
  } else if (error.kind === "unavailable") {
    title = t("agentInspectorTemporarilyUnavailable");
    hint = t("agentInspectorTemporarilyUnavailableHint");
  }

  return (
    <div className="notice notice--error agent-inspector-error" role="alert">
      <div>
        <strong>{title}</strong>
        <p>{hint}</p>
        {snapshot ? (
          <p className="agent-inspector-last-confirmed">
            {t("agentInspectorLastConfirmedSnapshot")}
            <time dateTime={snapshot.observed_at}>{formatDate(snapshot.observed_at, locale)}</time>
            {t("agentInspectorLastConfirmedSnapshotBoundary")}
          </p>
        ) : null}
        {error.requestId ? <small>{t("requestId")}: <code>{error.requestId}</code></small> : null}
      </div>
      {error.retryable ? (
        <Button type="button" variant="secondary" disabled={busy} onClick={retry}>{t("retry")}</Button>
      ) : null}
    </div>
  );
}


function Snapshot({
  snapshot,
  locale,
  busy,
  refresh,
  t,
}: {
  snapshot: AgentInspector;
  locale: Locale;
  busy: boolean;
  refresh: () => void;
  t: Translate;
}) {
  return (
    <>
      <ContextBar
        className="agent-inspector-context"
        title={snapshot.display_name}
        facts={[
          {
            id: "agent-id",
            label: t("agentInspectorAgent"),
            value: (
              <span className="agent-inspector-context__identity">
                <IdentityBadge kind="agent" label={t("identityAgent")} />
                <code>{snapshot.agent_id}</code>
              </span>
            ),
          },
          {
            id: "owner",
            label: t("agentInspectorOwner"),
            value: (
              <span className="agent-inspector-context__identity">
                <IdentityBadge kind="human" label={t("identityHuman")} />
                <strong>{snapshot.owner.display_name}</strong>
                <StatusIndicator
                  tone={principalStatusTone(snapshot.owner.principal_status)}
                  label={principalStatusLabel(snapshot.owner.principal_status, t)}
                />
                <code>{snapshot.owner.principal_id}</code>
              </span>
            ),
          },
          {
            id: "environment",
            label: t("agentInspectorEnvironment"),
            value: environmentLabel(snapshot.profile.environment, t),
          },
          {
            id: "observed-at",
            label: t("agentInspectorObservedAt"),
            value: <time dateTime={snapshot.observed_at}>{formatDate(snapshot.observed_at, locale)}</time>,
          },
        ]}
        status={(
          <StatusIndicator
            tone={principalStatusTone(snapshot.principal_status)}
            label={principalStatusLabel(snapshot.principal_status, t)}
          />
        )}
        actions={(
          <Button
            type="button"
            variant="secondary"
            className="agent-inspector-context__refresh"
            aria-disabled={busy || undefined}
            onClick={() => {
              if (!busy) refresh();
            }}
          >
            {t("refreshSnapshot")}
          </Button>
        )}
      />

      <p className="agent-inspector-boundary">{t("agentInspectorBoundary")}</p>

      <div className="agent-inspector-grid">
        <Panel className="agent-inspector-card" aria-labelledby="agent-inspector-profile-heading">
          <h2 id="agent-inspector-profile-heading">{t("agentInspectorProfile")}</h2>
          <dl>
            <Fact label={t("agentInspectorRiskLevel")}>
              <StatusIndicator tone={riskTone(snapshot.profile.risk_level)} label={riskLabel(snapshot.profile.risk_level, t)} />
            </Fact>
            <Fact label={t("purpose")}>
              <span>{snapshot.profile.purpose}</span>
            </Fact>
            <Fact label={t("agentInspectorReviewDue")}>
              <span>{formatDate(snapshot.profile.review_due_at, locale)}</span>
            </Fact>
            <Fact label={t("agentInspectorReviewState")}>
              <StatusIndicator tone={reviewTone(snapshot.profile.review_state)} label={reviewLabel(snapshot.profile.review_state, t)} />
            </Fact>
          </dl>
        </Panel>

        <Panel className="agent-inspector-card" aria-labelledby="agent-inspector-token-heading">
          <h2 id="agent-inspector-token-heading">{t("agentInspectorTokenAccess")}</h2>
          <dl>
            <Fact label={t("agentInspectorTokenState")}>
              <StatusIndicator tone={tokenAccessTone(snapshot.token_access.state)} label={tokenAccessLabel(snapshot.token_access.state, t)} />
            </Fact>
            <Fact label={t("agentInspectorEffectiveScopes")}>
              {snapshot.token_access.effective_scopes.length ? (
                <span className="agent-inspector-scopes">
                  {snapshot.token_access.effective_scopes.map((scope) => <code key={scope}>{scope}</code>)}
                </span>
              ) : <span>{t("agentInspectorNoEffectiveScopes")}</span>}
            </Fact>
            <Fact label={t("agentInspectorActiveCredentials")}>
              <span>{snapshot.token_access.active_credential_count}</span>
            </Fact>
            <Fact label={t("agentInspectorNextCredentialExpiry")}>
              <span>{optionalDate(snapshot.token_access.next_credential_expiry_at, "agentInspectorNoCredentialExpiry", locale, t)}</span>
            </Fact>
            <Fact label={t("agentInspectorLastAuthenticated")}>
              <span>{optionalDate(snapshot.token_access.last_authenticated_at, "agentInspectorNoAuthentication", locale, t)}</span>
            </Fact>
          </dl>
        </Panel>

        <Panel className="agent-inspector-card" aria-labelledby="agent-inspector-control-heading">
          <h2 id="agent-inspector-control-heading">{t("agentInspectorControlPlane")}</h2>
          <dl>
            <Fact label={t("agentInspectorMailAdmission")}>
              <StatusIndicator tone={mailAdmissionTone(snapshot.control_plane.mail_admission_state)} label={mailAdmissionLabel(snapshot.control_plane.mail_admission_state, t)} />
            </Fact>
            <Fact label={t("agentInspectorActiveSubscriptions")}>
              <span>{snapshot.control_plane.active_processing_subscription_count}</span>
            </Fact>
            <Fact label={t("agentInspectorPausedSubscriptions")}>
              <span>{snapshot.control_plane.paused_processing_subscription_count}</span>
            </Fact>
            <Fact label={t("agentInspectorRetiredSubscriptions")}>
              <span>{snapshot.control_plane.retired_processing_subscription_count}</span>
            </Fact>
            <Fact label={t("agentInspectorLastProcessingActivity")}>
              <span>{optionalDate(snapshot.control_plane.last_processing_activity_at, "agentInspectorNoProcessingActivity", locale, t)}</span>
            </Fact>
          </dl>
        </Panel>
      </div>
    </>
  );
}

export default function AgentInspectorView({
  client,
  locale,
  t,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  t: Translate;
}) {
  const [agentId, setAgentId] = useState("");
  const [snapshot, setSnapshot] = useState<AgentInspector>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<InspectorError>();
  const [completion, setCompletion] = useState<Completion>();
  const requestSequence = useRef(0);
  const validationMessageId = useId();

  async function load(
    id = agentId,
    { preserveSnapshot = false, completion: nextCompletion = "loaded" }: {
      preserveSnapshot?: boolean;
      completion?: Completion;
    } = {},
  ) {
    const normalizedId = id.trim();
    const sequence = requestSequence.current + 1;
    requestSequence.current = sequence;
    if (!preserveSnapshot) setSnapshot(undefined);
    setError(undefined);
    setCompletion(undefined);
    if (!isCanonicalUuid(normalizedId)) {
      setBusy(false);
      setError({ kind: "invalid", retryable: false });
      return;
    }

    setBusy(true);
    try {
      const result = await client.getAgentInspector(normalizedId);
      if (sequence !== requestSequence.current) return;
      setAgentId(result.agent_id);
      setSnapshot(result);
      setCompletion(nextCompletion);
    } catch (cause) {
      if (sequence !== requestSequence.current) return;
      setError(toInspectorError(cause));
    } finally {
      if (sequence === requestSequence.current) setBusy(false);
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void load();
  }

  function changeAgentId(value: string) {
    requestSequence.current += 1;
    setAgentId(value);
    setSnapshot(undefined);
    setError(undefined);
    setCompletion(undefined);
    setBusy(false);
  }

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1>{t("agentInspectorTitle")}</h1>
          <p>{t("agentInspectorHint")}</p>
        </div>
      </div>

      <Panel className="agent-inspector-lookup" aria-label={t("agentInspector")}>
        <form onSubmit={submit}>
          <div className="agent-inspector-id-field">
            <Field label={t("agentInspectorId")} hint={t("agentInspectorIdHint")}>
              <Input
                value={agentId}
                autoComplete="off"
                inputMode="text"
                maxLength={36}
                spellCheck={false}
                aria-invalid={error?.kind === "invalid" || undefined}
                aria-describedby={error?.kind === "invalid" ? validationMessageId : undefined}
                onChange={(event) => changeAgentId(event.target.value)}
                required
              />
            </Field>
            {error?.kind === "invalid" ? (
              <p id={validationMessageId} className="agent-inspector-validation" role="alert">
                {t("agentInspectorInvalidId")}
              </p>
            ) : null}
          </div>
          <Button type="submit" disabled={busy || !agentId.trim()}>{t("inspectAgent")}</Button>
        </form>
        {busy ? <Spinner label={t("loading")} /> : null}
      </Panel>

      {error ? <InspectorErrorNotice
        error={error}
        retry={() => {
          if (snapshot) {
            void load(snapshot.agent_id, { preserveSnapshot: true, completion: "refreshed" });
          } else {
            void load();
          }
        }}
        busy={busy}
        snapshot={snapshot}
        locale={locale}
        t={t}
      /> : null}
      <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {completion === "loaded"
          ? t("agentInspectorSnapshotLoaded")
          : completion === "refreshed"
            ? t("agentInspectorSnapshotRefreshed")
            : ""}
      </p>
      {snapshot ? (
        <>
          <Snapshot
            snapshot={snapshot}
            locale={locale}
            busy={busy}
            refresh={() => void load(snapshot.agent_id, { preserveSnapshot: true, completion: "refreshed" })}
            t={t}
          />
          <AgentMailSubmissionEvidenceView
            key={snapshot.agent_id}
            agentId={snapshot.agent_id}
            client={client}
            locale={locale}
            t={t}
          />
        </>
      ) : null}
    </>
  );
}
