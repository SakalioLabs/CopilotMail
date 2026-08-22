import {
  Badge,
  Button,
  EmptyState,
  Field,
  Panel,
  Select,
  Spinner,
} from "@copilotmail/design-system";
import { useCallback, useEffect, useState } from "react";
import { ApiError, ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type {
  ApprovalRequest,
  ApprovalRequestDetail,
  ApprovalStatus,
  ApprovalView as ApprovalViewKind,
  Locale,
  Page,
} from "./types";

type T = ReturnType<typeof translator>;

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

function errorText(error: unknown): string {
  if (error instanceof ApiError) {
    return `${error.body.code}: ${error.body.message} · ${error.body.request_id}`;
  }
  return error instanceof Error ? error.message : "unexpected_error";
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
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<unknown>();

  const currentCursor = cursors.at(-1);
  const load = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      const result = await client.listApprovalRequests({
        view,
        status: view === "requested" && status !== "all" ? status : undefined,
        cursor: currentCursor,
        limit: 50,
      });
      setPage(result);
      if (selectedId && !result.items.some((item) => item.approval_request_id === selectedId)) {
        setSelectedId(undefined);
        setDetail(undefined);
      }
    } catch (cause) {
      setError(cause);
    } finally {
      setLoading(false);
    }
  }, [client, currentCursor, selectedId, status, view]);

  useEffect(() => {
    void load();
  }, [load]);

  async function select(item: ApprovalRequest) {
    setSelectedId(item.approval_request_id);
    setDetail(undefined);
    setDecisionReason("");
    setError(undefined);
    try {
      setDetail(await client.getApprovalRequest(item.approval_request_id));
    } catch (cause) {
      setError(cause);
    }
  }

  async function decide(decision: "approved" | "rejected") {
    if (!detail || !decisionReason.trim()) return;
    setActing(true);
    setError(undefined);
    try {
      await client.decideApprovalRequest(
        detail.request.approval_request_id,
        decision,
        decisionReason.trim(),
      );
      setDetail(await client.getApprovalRequest(detail.request.approval_request_id));
      setDecisionReason("");
      await load();
    } catch (cause) {
      setError(cause);
    } finally {
      setActing(false);
    }
  }

  async function cancelRequest() {
    if (!detail || !window.confirm(t("confirmCancelApproval"))) return;
    setActing(true);
    setError(undefined);
    try {
      await client.cancelApprovalRequest(detail.request.approval_request_id);
      setDetail(await client.getApprovalRequest(detail.request.approval_request_id));
      await load();
    } catch (cause) {
      setError(cause);
    } finally {
      setActing(false);
    }
  }

  function changeView(next: ApprovalViewKind) {
    setView(next);
    setStatus("all");
    setCursors([undefined]);
    setSelectedId(undefined);
    setDetail(undefined);
  }

  return (
    <>
      <div className="page-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1>{t("approvalTitle")}</h1>
          <p>{t("approvalCenterHint")}</p>
        </div>
        <Button variant="secondary" onClick={() => void load()}>{t("refresh")}</Button>
      </div>

      <div className="toolbar">
        <Field label={t("approvalView")}>
          <Select value={view} onChange={(event) => changeView(event.target.value as ApprovalViewKind)}>
            <option value="actionable">{t("actionableApprovals")}</option>
            <option value="requested">{t("myRequests")}</option>
          </Select>
        </Field>
        {view === "requested" ? (
          <Field label={t("status")}>
            <Select
              value={status}
              onChange={(event) => {
                setStatus(event.target.value as ApprovalStatus | "all");
                setCursors([undefined]);
              }}
            >
              {(["all", "pending", "approved", "rejected", "expired", "cancelled"] as const)
                .map((item) => <option value={item} key={item}>{t(item)}</option>)}
            </Select>
          </Field>
        ) : null}
      </div>

      {error ? <div className="notice notice--error" role="alert"><p>{errorText(error)}</p></div> : null}
      {loading ? <Spinner label={t("loading")} /> : (
        <div className="master-detail approval-layout">
          <Panel className="list-panel">
            {!page?.items.length ? (
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
                        onClick={() => void select(item)}
                      >
                        <td><button className="row-button">{item.action}</button><span className="table-subline mono">{shortId(item.approval_request_id)}</span></td>
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
            {(cursors.length > 1 || page?.next_cursor) ? (
              <nav className="pagination" aria-label="Pagination">
                <Button variant="secondary" disabled={cursors.length === 1} onClick={() => setCursors((value) => value.slice(0, -1))}>{t("previous")}</Button>
                <Button variant="secondary" disabled={!page?.next_cursor} onClick={() => page?.next_cursor && setCursors((value) => [...value, page.next_cursor!])}>{t("next")}</Button>
              </nav>
            ) : null}
          </Panel>

          <Panel className="detail-panel approval-detail">
            {!selectedId ? <EmptyState title={t("details")} description={t("selectRow")} /> : !detail ? (
              <Spinner label={t("loading")} />
            ) : (
              <>
                <div className="detail-panel__heading">
                  <div><p className="eyebrow">{t("approvalRequest")}</p><h2 className="mono">{shortId(detail.request.approval_request_id)}</h2></div>
                  <Badge tone={statusTone(detail.request.status)}>{t(detail.request.status)}</Badge>
                </div>
                <dl>
                  <dt>{t("action")}</dt><dd>{detail.request.action}</dd>
                  <dt>{t("resource")}</dt><dd>{detail.request.resource_kind}<br /><code>{detail.request.resource_id}</code></dd>
                  <dt>{t("reason")}</dt><dd>{detail.request.reason}</dd>
                  <dt>{t("ticket")}</dt><dd>{detail.request.ticket_reference ?? "—"}</dd>
                  <dt>{t("requestedBy")}</dt><dd className="mono">{detail.request.requested_by}</dd>
                  <dt>{t("progress")}</dt><dd>{detail.request.approval_count}/{detail.request.required_approvals}</dd>
                  <dt>{t("createdAt")}</dt><dd>{formatDate(detail.request.created_at, locale)}</dd>
                  <dt>{t("expiresAt")}</dt><dd>{formatDate(detail.request.expires_at, locale)}</dd>
                </dl>
                {detail.steps.length ? (
                  <div className="approval-steps">
                    <h3>{t("decisionHistory")}</h3>
                    {detail.steps.map((step) => (
                      <div className="approval-step" key={step.approval_step_id}>
                        <Badge tone={step.decision === "approved" ? "positive" : "critical"}>{t(step.decision)}</Badge>
                        <span>{step.reason}</span>
                        <small>{formatDate(step.decided_at, locale)} · {shortId(step.approver_principal_id)}</small>
                      </div>
                    ))}
                  </div>
                ) : null}
                {detail.request.status === "pending" && view === "actionable" ? (
                  <div className="detail-actions approval-decision">
                    <Field label={t("decisionReason")}>
                      <textarea value={decisionReason} maxLength={1000} onChange={(event) => setDecisionReason(event.target.value)} />
                    </Field>
                    <div>
                      <Button disabled={acting || !decisionReason.trim()} onClick={() => void decide("approved")}>{t("approve")}</Button>
                      <Button variant="danger" disabled={acting || !decisionReason.trim()} onClick={() => void decide("rejected")}>{t("reject")}</Button>
                    </div>
                  </div>
                ) : null}
                {detail.request.status === "pending" && view === "requested" ? (
                  <div className="detail-actions">
                    <Button variant="danger" disabled={acting} onClick={() => void cancelRequest()}>{t("cancelApproval")}</Button>
                  </div>
                ) : null}
              </>
            )}
          </Panel>
        </div>
      )}
    </>
  );
}
