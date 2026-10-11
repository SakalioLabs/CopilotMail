import {
  type BadgeTone,
  Button,
  ContextBar,
  EvidenceInspector,
  Field,
  Input,
  Panel,
  Spinner,
  StatusIndicator,
} from "@copilotmail/design-system";
import { useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { ApiError, type ControlPlaneClient } from "./api";
import { adminErrorPresentation } from "./AdminErrorNotice";
import type { translator } from "./i18n";
import type { AgentMailSubmissionEvidence, AgentMailSubmissionStatus, Locale } from "./types";

type Translate = ReturnType<typeof translator>;
type EvidenceFailure = "invalid" | "forbidden" | "not-found" | "unavailable" | "unknown";

interface EvidenceError {
  kind: EvidenceFailure;
  requestId?: string;
  retryable: boolean;
}

function isCanonicalUuid(value: string): boolean {
  return value !== "00000000-0000-0000-0000-000000000000"
    && /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/.test(value);
}

function formatDate(value: string, locale: Locale): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function statusLabel(status: AgentMailSubmissionStatus, t: Translate): string {
  switch (status) {
    case "pending": return t("agentMailSubmissionStatusPending");
    case "reconciling": return t("agentMailSubmissionStatusReconciling");
    case "accepted": return t("agentMailSubmissionStatusAccepted");
    case "rejected": return t("agentMailSubmissionStatusRejected");
    case "manual_review": return t("agentMailSubmissionStatusManualReview");
  }
}

function statusTone(status: AgentMailSubmissionStatus): BadgeTone {
  switch (status) {
    case "accepted": return "positive";
    case "rejected": return "critical";
    case "pending":
    case "manual_review": return "warning";
    case "reconciling": return "info";
  }
}

function toEvidenceError(error: unknown): EvidenceError {
  if (!(error instanceof ApiError)) return { kind: "unknown", retryable: false };
  const presentation = adminErrorPresentation(error);
  if (error.status === 400) return { kind: "invalid", retryable: false };
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

function EvidenceErrorNotice({
  error,
  busy,
  retry,
  t,
}: {
  error: EvidenceError;
  busy: boolean;
  retry: () => void;
  t: Translate;
}) {
  if (error.kind === "invalid") return null;

  let title = t("agentMailSubmissionEvidenceRequestFailed");
  let hint = t("agentMailSubmissionEvidenceRequestFailedHint");
  if (error.kind === "forbidden") {
    title = t("agentMailSubmissionEvidenceAccessDenied");
    hint = t("agentMailSubmissionEvidenceAccessDeniedHint");
  } else if (error.kind === "not-found") {
    title = t("agentMailSubmissionEvidenceTargetUnavailable");
    hint = t("agentMailSubmissionEvidenceTargetUnavailableHint");
  } else if (error.kind === "unavailable") {
    title = t("agentMailSubmissionEvidenceUnavailable");
    hint = t("agentMailSubmissionEvidenceUnavailableHint");
  }

  return (
    <div className="notice notice--error agent-inspector-error" role="alert">
      <div>
        <strong>{title}</strong>
        <p>{hint}</p>
        {error.requestId ? <small>{t("requestId")}: <code>{error.requestId}</code></small> : null}
      </div>
      {error.retryable ? (
        <Button type="button" variant="secondary" disabled={busy} onClick={retry}>{t("retry")}</Button>
      ) : null}
    </div>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="agent-inspector-fact">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function EvidenceResult({
  evidence,
  locale,
  t,
}: {
  evidence: AgentMailSubmissionEvidence;
  locale: Locale;
  t: Translate;
}) {
  const { admission, delivery, submission } = evidence;

  return (
    <>
      <ContextBar
        className="agent-inspector-context"
        title={t("agentMailSubmissionEvidenceTitle")}
        facts={[
          {
            id: "submission-id",
            label: t("agentMailSubmissionEvidenceId"),
            value: <code>{submission.submission_id}</code>,
          },
          {
            id: "agent-id",
            label: t("agentInspectorAgent"),
            value: <code>{evidence.agent_id}</code>,
          },
          {
            id: "admission-id",
            label: t("agentMailSubmissionEvidenceAdmissionId"),
            value: <code>{admission.admission_id}</code>,
          },
          {
            id: "observed-at",
            label: t("agentMailSubmissionEvidenceObservedAt"),
            value: <time dateTime={evidence.observed_at}>{formatDate(evidence.observed_at, locale)}</time>,
          },
        ]}
        status={<StatusIndicator tone={statusTone(submission.status)} label={statusLabel(submission.status, t)} />}
      />

      <div className="agent-inspector-grid">
        <Panel className="agent-inspector-card" aria-labelledby="agent-mail-submission-heading">
          <h2 id="agent-mail-submission-heading">{t("agentMailSubmissionEvidenceSubmission")}</h2>
          <dl>
            <Fact label={t("agentMailSubmissionEvidenceStatus")}>
              <StatusIndicator tone={statusTone(submission.status)} label={statusLabel(submission.status, t)} />
            </Fact>
            <Fact label={t("agentMailSubmissionEvidenceCreatedAt")}>
              <time dateTime={submission.created_at}>{formatDate(submission.created_at, locale)}</time>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidenceUpdatedAt")}>
              <time dateTime={submission.updated_at}>{formatDate(submission.updated_at, locale)}</time>
            </Fact>
          </dl>
        </Panel>

        <Panel className="agent-inspector-card" aria-labelledby="agent-mail-admission-heading">
          <h2 id="agent-mail-admission-heading">{t("agentMailSubmissionEvidenceAdmission")}</h2>
          <dl>
            <Fact label={t("agentMailSubmissionEvidenceAdmissionId")}>
              <code>{admission.admission_id}</code>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidenceAdmittedAt")}>
              <time dateTime={admission.admitted_at}>{formatDate(admission.admitted_at, locale)}</time>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidencePolicyActivation")}>
              <code>{admission.policy.activation_id}</code>
              <time dateTime={admission.policy.activated_at}>{formatDate(admission.policy.activated_at, locale)}</time>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidencePolicyVersion")}>
              <code>{admission.policy.version_id}</code>
              <span>{admission.policy.version}</span>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidenceApprovalRequest")}>
              <code>{admission.policy.approval_request_id}</code>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidenceRecipientCount")}>
              <span>{admission.recipient_count}</span>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidenceFirstContactCount")}>
              <span>{admission.first_contact_count}</span>
            </Fact>
            <Fact label={t("agentMailSubmissionEvidenceFirstContactApproval")}>
              {admission.first_contact_approval_request_id ? (
                <code>{admission.first_contact_approval_request_id}</code>
              ) : <span>{t("agentMailSubmissionEvidenceNoFirstContactApproval")}</span>}
            </Fact>
          </dl>
        </Panel>

        <Panel className="agent-inspector-card" aria-labelledby="agent-mail-delivery-heading">
          <h2 id="agent-mail-delivery-heading">{t("agentMailSubmissionEvidenceDelivery")}</h2>
          {delivery ? (
            <dl>
              <Fact label={t("agentMailSubmissionEvidenceDeliveryObservedAt")}>
                <time dateTime={delivery.observed_at}>{formatDate(delivery.observed_at, locale)}</time>
              </Fact>
              <Fact label={t("agentMailSubmissionEvidenceTerminal")}>
                <StatusIndicator
                  tone={delivery.terminal ? "neutral" : "info"}
                  label={delivery.terminal
                    ? t("agentMailSubmissionEvidenceTerminalTrue")
                    : t("agentMailSubmissionEvidenceTerminalFalse")}
                />
              </Fact>
              <Fact label={t("agentMailSubmissionEvidenceRecipientCount")}>
                <span>{delivery.recipient_count}</span>
              </Fact>
              <Fact label={t("agentMailSubmissionEvidenceDeliveredCount")}>
                <span>{delivery.delivered_count}</span>
              </Fact>
              <Fact label={t("agentMailSubmissionEvidenceFailedCount")}>
                <span>{delivery.failed_count}</span>
              </Fact>
              <Fact label={t("agentMailSubmissionEvidenceQueuedCount")}>
                <span>{delivery.queued_count}</span>
              </Fact>
              <Fact label={t("agentMailSubmissionEvidenceUnknownCount")}>
                <span>{delivery.unknown_count}</span>
              </Fact>
            </dl>
          ) : <p>{t("agentMailSubmissionEvidenceNoDelivery")}</p>}
        </Panel>
      </div>

      <EvidenceInspector
        className="evidence-section"
        count={evidence.audit_references.length}
        summary={t("agentMailSubmissionEvidenceAuditReferences")}
      >
        <p>{t("agentMailSubmissionEvidenceAuditReferenceHint")}</p>
        <dl>
          {evidence.audit_references.map((reference) => (
            <div key={reference.event_id}>
              <dt><code>{reference.action}</code></dt>
              <dd>
                <code>{reference.event_id}</code>
                <time dateTime={reference.occurred_at}>{formatDate(reference.occurred_at, locale)}</time>
              </dd>
            </div>
          ))}
        </dl>
      </EvidenceInspector>
    </>
  );
}

/**
 * An explicit, one-pair evidence read. It deliberately owns no URL, storage,
 * polling, or Agent directory state; a new Agent key starts a fresh request
 * boundary in the parent Inspector.
 */
export default function AgentMailSubmissionEvidenceView({
  agentId,
  client,
  locale,
  t,
}: {
  agentId: string;
  client: ControlPlaneClient;
  locale: Locale;
  t: Translate;
}) {
  const [submissionId, setSubmissionId] = useState("");
  const [evidence, setEvidence] = useState<AgentMailSubmissionEvidence>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<EvidenceError>();
  const requestSequence = useRef(0);
  const validationMessageId = useId();

  async function load() {
    const requestedSubmissionId = submissionId;
    const sequence = requestSequence.current + 1;
    requestSequence.current = sequence;
    setEvidence(undefined);
    setError(undefined);

    if (!isCanonicalUuid(requestedSubmissionId)) {
      setBusy(false);
      setError({ kind: "invalid", retryable: false });
      return;
    }

    setBusy(true);
    try {
      const result = await client.getAgentMailSubmissionEvidence(agentId, requestedSubmissionId);
      if (sequence !== requestSequence.current) return;
      if (
        result.agent_id !== agentId
        || result.submission.submission_id !== requestedSubmissionId
      ) {
        setError({ kind: "unknown", retryable: false });
        return;
      }
      setEvidence(result);
    } catch (cause) {
      if (sequence !== requestSequence.current) return;
      setError(toEvidenceError(cause));
    } finally {
      if (sequence === requestSequence.current) setBusy(false);
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void load();
  }

  function changeSubmissionId(value: string) {
    requestSequence.current += 1;
    setSubmissionId(value);
    setEvidence(undefined);
    setError(undefined);
    setBusy(false);
  }

  return (
    <>
      <Panel className="agent-inspector-lookup" aria-label={t("agentMailSubmissionEvidenceTitle")}>
        <form onSubmit={submit}>
          <div className="agent-inspector-id-field">
            <Field
              label={t("agentMailSubmissionEvidenceId")}
              hint={t("agentMailSubmissionEvidenceIdHint")}
            >
              <Input
                value={submissionId}
                autoComplete="off"
                inputMode="text"
                maxLength={36}
                spellCheck={false}
                aria-invalid={error?.kind === "invalid" || undefined}
                aria-describedby={error?.kind === "invalid" ? validationMessageId : undefined}
                onChange={(event) => changeSubmissionId(event.target.value)}
                required
              />
            </Field>
            {error?.kind === "invalid" ? (
              <p id={validationMessageId} className="agent-inspector-validation" role="alert">
                {t("agentMailSubmissionEvidenceInvalidId")}
              </p>
            ) : null}
          </div>
          <Button type="submit" disabled={busy || !submissionId}>{t("inspectAgentMailSubmission")}</Button>
        </form>
        {busy ? <Spinner label={t("agentMailSubmissionEvidenceLoading")} /> : null}
      </Panel>

      <p className="agent-inspector-boundary">{t("agentMailSubmissionEvidenceHint")}</p>

      {error ? <EvidenceErrorNotice error={error} busy={busy} retry={() => void load()} t={t} /> : null}
      {evidence ? <EvidenceResult evidence={evidence} locale={locale} t={t} /> : null}
    </>
  );
}
