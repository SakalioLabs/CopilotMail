import {
  Badge,
  Button,
  Field,
  Input,
  Panel,
  Select,
  Spinner,
} from "@copilotmail/design-system";
import { useState, type FormEvent } from "react";
import type { ControlPlaneClient } from "./api";
import type { translator } from "./i18n";
import type {
  Locale,
  RetentionPolicy,
  RetentionResourceKind,
} from "./types";

type Translate = ReturnType<typeof translator>;

function formatDate(value: string, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : "unexpected_error";
}

function optionalDays(value: string): number | null {
  return value.trim() ? Number(value) : null;
}

function parseRule(value: string): Record<string, unknown> {
  const parsed = JSON.parse(value) as unknown;
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error("rule_must_be_an_object");
  }
  return parsed as Record<string, unknown>;
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
            {t("archiveAfter")}: {version.archive_after_days ?? "—"} ·{" "}
            {t("deleteAfter")}: {version.delete_after_days ?? t("indefinite")}
          </small>
          <small>{formatDate(version.created_at, locale)}</small>
          <code>{version.retention_policy_version_id}</code>
        </>
      ) : <p>{t("noActiveVersion")}</p>}
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
  const [policyKey, setPolicyKey] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [resourceKind, setResourceKind] =
    useState<RetentionResourceKind>("audit_evidence");
  const [minimumDays, setMinimumDays] = useState("2555");
  const [archiveDays, setArchiveDays] = useState("30");
  const [deleteDays, setDeleteDays] = useState("");
  const [rule, setRule] = useState("{}");
  const [approvalId, setApprovalId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  async function load(id = policyId) {
    if (!id.trim()) return;
    setBusy(true);
    setError(undefined);
    try {
      const result = await client.getRetentionPolicy(id.trim());
      setPolicy(result);
      setPolicyId(result.retention_policy_id);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  }

  async function create(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      const result = await client.createRetentionPolicy({
        policy_key: policyKey.trim(),
        display_name: displayName.trim(),
        resource_kind: resourceKind,
        minimum_retention_days: Number(minimumDays),
        archive_after_days: optionalDays(archiveDays),
        delete_after_days: optionalDays(deleteDays),
        rule: parseRule(rule),
      });
      setPolicy(result);
      setPolicyId(result.retention_policy_id);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  }

  async function appendVersion() {
    if (!policy) return;
    setBusy(true);
    setError(undefined);
    try {
      await client.appendRetentionPolicyVersion(policy.retention_policy_id, {
        minimum_retention_days: Number(minimumDays),
        archive_after_days: optionalDays(archiveDays),
        delete_after_days: optionalDays(deleteDays),
        rule: parseRule(rule),
      });
      await load(policy.retention_policy_id);
    } catch (cause) {
      setError(errorText(cause));
      setBusy(false);
    }
  }

  async function activateLatest() {
    if (!policy || !approvalId.trim()) return;
    setBusy(true);
    setError(undefined);
    try {
      await client.activateRetentionPolicyVersion(
        policy.retention_policy_id,
        policy.latest_version.retention_policy_version_id,
        approvalId.trim(),
      );
      setApprovalId("");
      await load(policy.retention_policy_id);
    } catch (cause) {
      setError(errorText(cause));
      setBusy(false);
    }
  }

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1>{t("retentionTitle")}</h1>
          <p>{t("retentionHint")}</p>
        </div>
      </div>

      {error ? <div className="notice notice--error" role="alert">{error}</div> : null}

      <Panel>
        <div className="retention-lookup">
          <Field label={t("retentionPolicyId")}>
            <Input value={policyId} onChange={(event) => setPolicyId(event.target.value)} placeholder="UUID" />
          </Field>
          <Button type="button" variant="secondary" disabled={busy || !policyId.trim()} onClick={() => void load()}>
            {t("loadPolicy")}
          </Button>
        </div>
      </Panel>

      {policy ? (
        <Panel>
          <div className="retention-heading">
            <div>
              <Badge tone={policy.active_version ? "positive" : "warning"}>
                {policy.active_version ? t("active") : t("draft")}
              </Badge>
              <h2>{policy.display_name}</h2>
              <p><code>{policy.policy_key}</code> · {t(policy.resource_kind)}</p>
            </div>
          </div>
          <div className="retention-versions">
            <VersionCard title={t("activeVersion")} version={policy.active_version} locale={locale} t={t} />
            <VersionCard title={t("latestVersion")} version={policy.latest_version} locale={locale} t={t} />
          </div>
          <div className="retention-actions">
            <Button type="button" variant="secondary" disabled={busy} onClick={() => void appendVersion()}>
              {t("appendDraft")}
            </Button>
            <Field label={t("approvalId")}>
              <Input value={approvalId} onChange={(event) => setApprovalId(event.target.value)} placeholder="UUID" />
            </Field>
            <Button type="button" disabled={busy || !approvalId.trim()} onClick={() => void activateLatest()}>
              {t("activateLatest")}
            </Button>
          </div>
        </Panel>
      ) : null}

      <Panel>
        <form onSubmit={(event) => void create(event)}>
          <div className="retention-form">
            <Field label={t("policyKey")}>
              <Input value={policyKey} onChange={(event) => setPolicyKey(event.target.value)} placeholder="audit.standard" required />
            </Field>
            <Field label={t("displayName")}>
              <Input value={displayName} onChange={(event) => setDisplayName(event.target.value)} required />
            </Field>
            <Field label={t("resourceKind")}>
              <Select value={resourceKind} onChange={(event) => setResourceKind(event.target.value as RetentionResourceKind)}>
                <option value="audit_evidence">{t("audit_evidence")}</option>
                <option value="mail_content">{t("mail_content")}</option>
              </Select>
            </Field>
            <Field label={t("minimumRetention")}>
              <Input type="number" min="1" max="36500" value={minimumDays} onChange={(event) => setMinimumDays(event.target.value)} required />
            </Field>
            <Field label={t("archiveAfter")}>
              <Input type="number" min="1" max="36500" value={archiveDays} onChange={(event) => setArchiveDays(event.target.value)} />
            </Field>
            <Field label={t("deleteAfter")}>
              <Input type="number" min="1" max="36500" value={deleteDays} onChange={(event) => setDeleteDays(event.target.value)} placeholder={t("indefinite")} />
            </Field>
            <Field label={t("scopeRule")}>
              <textarea className="control" rows={3} value={rule} onChange={(event) => setRule(event.target.value)} />
            </Field>
          </div>
          <div className="retention-submit">
            {busy ? <Spinner label={t("loading")} /> : null}
            <Button type="submit" disabled={busy}>{t("createDraftPolicy")}</Button>
          </div>
        </form>
      </Panel>
    </>
  );
}
