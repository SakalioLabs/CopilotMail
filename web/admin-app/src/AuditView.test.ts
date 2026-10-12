// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AuditView, { auditFiltersFromDraft, type AuditDraftFilters } from "./AuditView";
import { ApiError, type ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type { AuditEvent } from "./types";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const EXPORT_ARCHIVE_ID = "019b0000-0000-7000-8000-000000000701";
const EXPORT_APPROVAL_ID = "019b0000-0000-7000-8000-000000000702";
const EXPORT_GRANT_ID = "019b0000-0000-7000-8000-000000000703";
const EXPORT_AUTHORIZATION_OUTCOME_UNKNOWN = "The server may already have created this export authorization. Grant creation is frozen; do not submit it again. Read only the exact grant for this archive and approval relationship.";
const EXPORT_DOWNLOAD_OUTCOME_UNKNOWN = "The server may already have recorded this download before bytes reached the browser. Downloads are frozen; do not retry. Read only this exact grant state.";

function exportGrant(expiresAt = "2099-08-24T08:15:00Z") {
  return {
    audit_export_grant_id: EXPORT_GRANT_ID,
    organization_id: "019b0000-0000-7000-8000-000000000700",
    archive_id: EXPORT_ARCHIVE_ID,
    approval_request_id: EXPORT_APPROVAL_ID,
    requested_by: "019b0000-0000-7000-8000-000000000704",
    created_at: "2026-08-24T08:00:00Z",
    expires_at: expiresAt,
  };
}

function emptyAuditPage() {
  return {
    items: [],
    has_more: false,
    next_cursor: null,
    query_id: "019b0000-0000-7000-8000-000000000705",
    occurred_from: "2026-08-23T08:00:00Z",
    occurred_to: "2026-08-24T08:00:00Z",
  };
}

function viewAuditExport(client: Partial<ControlPlaneClient>) {
  return render(createElement(AuditView, {
    client: {
      searchAuditEvents: vi.fn().mockResolvedValue(emptyAuditPage()),
      ...client,
    } as ControlPlaneClient,
    locale: "en",
    t: translator("en"),
  }));
}

function fillExportCommand() {
  const exportDetails = screen.getByText("Export authorization").closest("details");
  if (!exportDetails?.open) fireEvent.click(screen.getByText("Export authorization"));
  fireEvent.change(screen.getByRole("textbox", { name: "Audit archive ID" }), {
    target: { value: EXPORT_ARCHIVE_ID },
  });
  fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
    target: { value: EXPORT_APPROVAL_ID },
  });
}

function draft(from: string, to: string): AuditDraftFilters {
  return {
    from,
    to,
    action: " compliance.case.queried ",
    actor: "",
    resourceType: "compliance.case_query",
    resourceId: " case-1 ",
    outcome: "success",
    requestId: "",
  };
}

describe("audit filter boundary", () => {
  it("converts local input to exact ISO instants and trims optional values", () => {
    expect(auditFiltersFromDraft(draft(
      "2026-07-25T08:00",
      "2026-07-26T08:00",
    ))).toMatchObject({
      occurred_from: new Date("2026-07-25T08:00").toISOString(),
      occurred_to: new Date("2026-07-26T08:00").toISOString(),
      action: "compliance.case.queried",
      resource_type: "compliance.case_query",
      resource_id: "case-1",
      outcome: "success",
    });
  });

  it("rejects reversed and over-31-day windows before making a request", () => {
    expect(auditFiltersFromDraft(draft(
      "2026-07-26T08:00",
      "2026-07-25T08:00",
    ))).toBeNull();
    expect(auditFiltersFromDraft(draft(
      "2026-06-01T08:00",
      "2026-07-26T08:00",
    ))).toBeNull();
  });
});

function auditEvent(kind: AuditEvent["actor_kind"]): AuditEvent {
  return {
    event_id: "019b0000-0000-7000-8000-000000000601",
    organization_id: "019b0000-0000-7000-8000-000000000600",
    actor_principal_id: "019b0000-0000-7000-8000-000000000602",
    actor_kind: kind,
    on_behalf_of_principal_id: null,
    credential_id: null,
    subject_principal_id: null,
    action: "compliance.case.queried",
    resource_type: "compliance.case_query",
    resource_id: "019b0000-0000-7000-8000-000000000603",
    outcome: "success",
    trace_id: "trace-601",
    request_id: "request-601",
    source_ip: null,
    client_id: null,
    session_id: null,
    token_jti: null,
    policy_id: null,
    policy_version_id: null,
    approval_request_id: null,
    ticket_reference: null,
    environment: null,
    region: null,
    details: {},
    occurred_at: "2026-08-23T08:00:00Z",
    received_at: "2026-08-23T08:00:01Z",
    chain_sequence: "601",
    hash_version: 1,
    previous_hash: null,
    event_hash: "abc123",
    signature_batch_id: null,
  };
}

function viewAudit(
  event: AuditEvent,
  pageOverrides: { has_more?: boolean; next_cursor?: string | null } = {},
) {
  const client = {
    searchAuditEvents: vi.fn().mockResolvedValue({
      items: [event],
      has_more: false,
      next_cursor: null,
      query_id: "019b0000-0000-7000-8000-000000000604",
      occurred_from: "2026-08-22T08:00:00Z",
      occurred_to: "2026-08-23T08:00:00Z",
      ...pageOverrides,
    }),
  } as unknown as ControlPlaneClient;

  return render(createElement(AuditView, { client, locale: "en", t: translator("en") }));
}

describe("AuditView identity and status markers", () => {
  it("renders a persisted Service actor with its factual label and a bounded system glyph", async () => {
    const event = auditEvent("service");
    viewAudit(event);

    const service = await screen.findByText("Service");
    const serviceBadge = service.closest("[data-identity-kind]");
    expect(serviceBadge).toHaveAttribute("data-identity-kind", "service");
    expect(serviceBadge?.querySelector('[data-icon="system"]')).toBeInTheDocument();
    expect(screen.queryByText("System", { exact: true })).not.toBeInTheDocument();
    expect(screen.getByText("Success", { selector: ".status-indicator__label" }).closest("[data-status-tone]"))
      .toHaveAttribute("data-status-tone", "positive");

    const row = screen.getByRole("button", { name: event.action });
    expect(row).not.toHaveAttribute("aria-current");
    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-current", "true");
    const detailServices = await screen.findAllByText("Service");
    expect(detailServices).toHaveLength(2);
    expect(detailServices[1]!.closest("[data-identity-kind]"))
      .toHaveAttribute("data-identity-kind", "service");
  });

  it("keeps all event evidence collapsed until an auditor explicitly discloses it", async () => {
    const event = auditEvent("agent");
    event.details = { decision: "approved" };
    viewAudit(event);

    fireEvent.click(await screen.findByRole("button", { name: event.action }));

    const hashEvidence = screen.getByText("Hash-chain evidence").closest("details");
    const governanceContext = screen.getByText("Minimal governance context").closest("details");
    const sanitizedDetails = screen.getByText("Sanitized details").closest("details");

    expect(hashEvidence).not.toHaveAttribute("open");
    expect(governanceContext).not.toHaveAttribute("open");
    expect(sanitizedDetails).not.toHaveAttribute("open");
    const queryContext = screen.getByRole("region", { name: "Query context" });
    expect(queryContext).toHaveTextContent(
      "019b0000-0000-7000-8000-000000000604",
    );
    expect(queryContext).toHaveTextContent("No additional filters");
    expect(queryContext).toHaveTextContent("1");
    expect(queryContext).toHaveTextContent("server indicates final page");

    fireEvent.click(screen.getByText("Hash-chain evidence"));
    expect(hashEvidence).toHaveAttribute("open");

    fireEvent.click(screen.getByText("Minimal governance context"));
    expect(governanceContext).toHaveAttribute("open");
    expect(screen.getByText("trace-601")).toBeVisible();

    fireEvent.click(screen.getByText("Sanitized details"));
    expect(sanitizedDetails).toHaveAttribute("open");
    expect(sanitizedDetails).toHaveTextContent("approved");
  });

  it("orders query context, conditions, results queue, and evidence before the export gate", async () => {
    const event = auditEvent("agent");
    viewAudit(event);

    fireEvent.click(await screen.findByRole("button", { name: event.action }));

    const context = screen.getByRole("region", { name: "Query context" });
    const filters = screen.getByRole("form", { name: "Query conditions" });
    const results = screen.getByRole("region", { name: "Results queue" });
    const hashEvidence = screen.getByText("Hash-chain evidence").closest("details");
    const exportGate = screen.getByText("Export authorization").closest("details");

    expect(hashEvidence).not.toBeNull();
    expect(exportGate).not.toBeNull();
    expect(exportGate).toHaveTextContent("High-risk security gate");
    expect(results).toHaveTextContent("Select one event in the hash-chain order returned by the server");
    expect(screen.getByRole("region", { name: "Event facts" })).toHaveTextContent(event.event_id);
    expect(context.compareDocumentPosition(filters) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(filters.compareDocumentPosition(results) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(hashEvidence!.compareDocumentPosition(exportGate!) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(exportGate).not.toHaveAttribute("open");
  });

  it("shows only the exact optional filter contract and server page boundary in query context", async () => {
    const event = auditEvent("human");
    const { container } = viewAudit(event, {
      has_more: true,
      next_cursor: "v1.org.600",
    });

    await screen.findByRole("region", { name: "Query context" });
    fireEvent.click(screen.getByText("More filters"));
    fireEvent.change(screen.getByRole("textbox", { name: "Action" }), {
      target: { value: event.action },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Resource type" }), {
      target: { value: event.resource_type },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Resource ID" }), {
      target: { value: event.resource_id },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Outcome" }), {
      target: { value: event.outcome },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run search" }));

    const queryContext = await screen.findByRole("region", { name: "Query context" });
    expect(queryContext).toHaveTextContent("Action");
    expect(queryContext).toHaveTextContent(event.action);
    expect(queryContext).toHaveTextContent("Resource type");
    expect(queryContext).toHaveTextContent(event.resource_type);
    expect(queryContext).toHaveTextContent("Resource ID");
    expect(queryContext).toHaveTextContent(event.resource_id);
    expect(queryContext).toHaveTextContent("Outcome");
    expect(queryContext).toHaveTextContent("Success");
    expect(queryContext).toHaveTextContent("1");
    expect(queryContext).toHaveTextContent("server indicates more pages");
    expect(queryContext).not.toHaveTextContent("Actor");
    expect(container.querySelector(".audit-query-filter-summary")).not.toBeNull();
  });
});

describe("AuditView export authorization evidence", () => {
  it("shows the exact archive, approval, and grant relationship and invalidates it on either input change", async () => {
    const authorizeAuditExport = vi.fn().mockResolvedValue(exportGrant());
    const downloadAuditExportArtifact = vi.fn();
    viewAuditExport({ authorizeAuditExport, downloadAuditExportArtifact });
    fillExportCommand();

    fireEvent.click(screen.getByRole("button", { name: "Create authorization" }));

    const evidence = await screen.findByRole("status", {
      name: "Export authorization created",
    });
    expect(evidence).toHaveTextContent(EXPORT_ARCHIVE_ID);
    expect(evidence).toHaveTextContent(EXPORT_APPROVAL_ID);
    expect(evidence).toHaveTextContent(EXPORT_GRANT_ID);
    expect(within(evidence).getByRole("button", { name: "Download events" }))
      .toBeEnabled();
    expect(authorizeAuditExport).toHaveBeenCalledWith(
      EXPORT_ARCHIVE_ID,
      EXPORT_APPROVAL_ID,
    );

    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: `${EXPORT_APPROVAL_ID}0` },
    });

    expect(screen.queryByRole("status", { name: "Export authorization created" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Download events" }))
      .not.toBeInTheDocument();
    expect(downloadAuditExportArtifact).not.toHaveBeenCalled();
  });

  it("reconciles an unknown authorization only through its exact GET and never replays the POST", async () => {
    const authorizeAuditExport = vi.fn().mockRejectedValueOnce(new TypeError("connection reset"));
    const getAuditExportGrantByApproval = vi.fn().mockResolvedValue(exportGrant());
    viewAuditExport({ authorizeAuditExport, getAuditExportGrantByApproval });
    fillExportCommand();

    fireEvent.click(screen.getByRole("button", { name: "Create authorization" }));

    const unknown = await screen.findByRole("status", {
      name: EXPORT_AUTHORIZATION_OUTCOME_UNKNOWN,
    });
    const archive = screen.getByRole("textbox", { name: "Audit archive ID" });
    const approval = screen.getByRole("textbox", { name: "Approval request ID" });
    expect(unknown).toHaveTextContent(EXPORT_ARCHIVE_ID);
    expect(unknown).toHaveTextContent(EXPORT_APPROVAL_ID);
    expect(archive).toBeDisabled();
    expect(approval).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create authorization" }))
      .toBeDisabled();
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry exact request" })).not.toBeInTheDocument();
    expect(authorizeAuditExport).toHaveBeenCalledTimes(1);

    fireEvent.click(within(unknown).getByRole("button", { name: "Reconcile read-only state" }));

    const evidence = await screen.findByRole("status", { name: "Export authorization created" });
    expect(evidence).toHaveTextContent(EXPORT_GRANT_ID);
    expect(getAuditExportGrantByApproval).toHaveBeenCalledTimes(1);
    expect(getAuditExportGrantByApproval).toHaveBeenCalledWith(
      EXPORT_ARCHIVE_ID,
      EXPORT_APPROVAL_ID,
    );
    expect(authorizeAuditExport).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("status", {
      name: EXPORT_AUTHORIZATION_OUTCOME_UNKNOWN,
    })).not.toBeInTheDocument();
  });

  it("freezes every failed download and reconciles only the known grant with a GET", async () => {
    const authorizeAuditExport = vi.fn().mockResolvedValue(exportGrant());
    const downloadAuditExportArtifact = vi.fn().mockRejectedValue(new ApiError(400, {
      code: "audit_export_artifact_rejected",
      message: "artifact request rejected",
      request_id: "download-request-1",
      trace_id: "download-trace-1",
      retryable: false,
    }));
    const getAuditExportGrant = vi.fn().mockResolvedValue(exportGrant());
    viewAuditExport({
      authorizeAuditExport,
      downloadAuditExportArtifact,
      getAuditExportGrant,
    });
    fillExportCommand();

    fireEvent.click(screen.getByRole("button", { name: "Create authorization" }));
    const events = await screen.findByRole("button", { name: "Download events" });
    const manifest = screen.getByRole("button", { name: "Download manifest" });
    fireEvent.click(events);

    const frozen = await screen.findByRole("status", {
      name: EXPORT_DOWNLOAD_OUTCOME_UNKNOWN,
    });
    expect(frozen).toHaveTextContent(EXPORT_GRANT_ID);
    expect(events).toBeDisabled();
    expect(manifest).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry exact request" })).not.toBeInTheDocument();
    expect(downloadAuditExportArtifact).toHaveBeenCalledTimes(1);
    expect(downloadAuditExportArtifact).toHaveBeenCalledWith(EXPORT_GRANT_ID, "events");

    fireEvent.click(within(frozen).getByRole("button", { name: "Reconcile read-only state" }));

    expect(await screen.findByRole("status", { name: "Read-only reconciliation evidence" }))
      .toHaveTextContent(EXPORT_GRANT_ID);
    expect(getAuditExportGrant).toHaveBeenCalledTimes(1);
    expect(getAuditExportGrant).toHaveBeenCalledWith(EXPORT_GRANT_ID);
    expect(authorizeAuditExport).toHaveBeenCalledTimes(1);
    expect(downloadAuditExportArtifact).toHaveBeenCalledTimes(1);
    expect(events).toBeDisabled();
    expect(manifest).toBeDisabled();
  });

  it("expires the current grant at expires_at and disables both downloads without another request", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-24T08:00:00Z"));
    const authorizeAuditExport = vi.fn().mockResolvedValue(
      exportGrant("2026-08-24T08:01:00Z"),
    );
    const downloadAuditExportArtifact = vi.fn();
    viewAuditExport({ authorizeAuditExport, downloadAuditExportArtifact });
    fillExportCommand();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Create authorization" }));
      await Promise.resolve();
    });
    const events = screen.getByRole("button", { name: "Download events" });
    const manifest = screen.getByRole("button", { name: "Download manifest" });
    expect(events).toBeEnabled();
    expect(manifest).toBeEnabled();

    act(() => vi.advanceTimersByTime(60_001));

    expect(screen.getByRole("status", { name: "Export authorization expired" }))
      .toHaveTextContent(EXPORT_GRANT_ID);
    expect(events).toBeDisabled();
    expect(manifest).toBeDisabled();
    fireEvent.click(events);
    expect(downloadAuditExportArtifact).not.toHaveBeenCalled();
  });

  it("never delivers a completed download after an input invalidates its grant", async () => {
    let resolveDownload: ((value: { blob: Blob; fileName: string; sha256: string }) => void) | undefined;
    const pending = new Promise<{ blob: Blob; fileName: string; sha256: string }>((resolve) => {
      resolveDownload = resolve;
    });
    const NativeURL = URL;
    const createObjectURL = vi.fn();
    class TestURL extends NativeURL {}
    Object.defineProperties(TestURL, {
      createObjectURL: { value: createObjectURL },
      revokeObjectURL: { value: vi.fn() },
    });
    vi.stubGlobal("URL", TestURL);
    viewAuditExport({
      authorizeAuditExport: vi.fn().mockResolvedValue(exportGrant()),
      downloadAuditExportArtifact: vi.fn().mockReturnValue(pending),
    });
    fillExportCommand();
    fireEvent.click(screen.getByRole("button", { name: "Create authorization" }));
    const events = await screen.findByRole("button", { name: "Download events" });
    fireEvent.click(events);

    fireEvent.change(screen.getByRole("textbox", { name: "Audit archive ID" }), {
      target: { value: `${EXPORT_ARCHIVE_ID}0` },
    });
    resolveDownload?.({
      blob: new Blob(["evidence"]),
      fileName: `${EXPORT_GRANT_ID}-events.jsonl`,
      sha256: "a".repeat(64),
    });
    await waitFor(() => expect(createObjectURL).not.toHaveBeenCalled());
  });
});

describe("AuditView filter validation", () => {
  it("binds the missing resource type to its field, announces the alert, and keeps the search unsent", async () => {
    const searchAuditEvents = vi.fn().mockResolvedValue(emptyAuditPage());
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    viewAuditExport({ searchAuditEvents });
    await screen.findByRole("region", { name: "Query context" });

    fireEvent.click(screen.getByText("More filters"));
    fireEvent.change(screen.getByRole("textbox", { name: "Resource ID" }), {
      target: { value: "case-1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run search" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Resource type is required when filtering by resource ID.",
    );
    expect(alert.id).not.toBe("");
    const resourceType = screen.getByRole("textbox", { name: "Resource type" });
    expect(resourceType).toHaveAttribute("aria-invalid", "true");
    expect(resourceType.getAttribute("aria-describedby") ?? "")
      .toContain(alert.id);
    await waitFor(() => expect(resourceType).toHaveFocus());
    expect(focus.mock.calls.some(([options]) => (
      typeof options === "object"
      && options !== null
      && "focusVisible" in options
      && options.focusVisible === true
    ))).toBe(true);
    focus.mockRestore();
    expect(searchAuditEvents).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("From")).not.toHaveAttribute("aria-invalid");

    fireEvent.change(resourceType, { target: { value: "compliance.case" } });
    fireEvent.click(screen.getByRole("button", { name: "Run search" }));

    await waitFor(() => expect(searchAuditEvents).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(resourceType).not.toHaveAttribute("aria-invalid");
    expect(resourceType.getAttribute("aria-describedby") ?? "")
      .not.toContain(alert.id);
  });

  it("reopens collapsed advanced filters before focusing the invalid field", async () => {
    const searchAuditEvents = vi.fn().mockResolvedValue(emptyAuditPage());
    viewAuditExport({ searchAuditEvents });
    await screen.findByRole("region", { name: "Query context" });

    const details = screen.getByText("More filters").closest("details")!;
    fireEvent.click(screen.getByText("More filters"));
    fireEvent.change(screen.getByRole("textbox", { name: "Resource ID" }), {
      target: { value: "case-1" },
    });
    fireEvent.click(screen.getByText("More filters"));
    expect(details).not.toHaveAttribute("open");

    fireEvent.click(screen.getByRole("button", { name: "Run search" }));
    await screen.findByRole("alert");
    await waitFor(() => expect(details).toHaveAttribute("open"));
    await waitFor(() => expect(
      screen.getByRole("textbox", { name: "Resource type" }),
    ).toHaveFocus());
  });

  it("binds an invalid time range to both boundaries and focuses the start", async () => {
    const searchAuditEvents = vi.fn().mockResolvedValue(emptyAuditPage());
    viewAuditExport({ searchAuditEvents });
    await screen.findByRole("region", { name: "Query context" });

    const from = screen.getByLabelText("From");
    const to = screen.getByLabelText("To");
    fireEvent.change(from, { target: { value: "2026-07-26T08:00" } });
    fireEvent.change(to, { target: { value: "2026-07-25T08:00" } });
    fireEvent.click(screen.getByRole("button", { name: "Run search" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "The time window must be valid and no longer than 31 days.",
    );
    for (const boundary of [from, to]) {
      expect(boundary).toHaveAttribute("aria-invalid", "true");
      expect(boundary.getAttribute("aria-describedby") ?? "")
        .toContain(alert.id);
    }
    await waitFor(() => expect(from).toHaveFocus());
    expect(searchAuditEvents).toHaveBeenCalledTimes(1);

    fireEvent.change(from, { target: { value: "2026-07-24T08:00" } });
    fireEvent.click(screen.getByRole("button", { name: "Run search" }));

    await waitFor(() => expect(searchAuditEvents).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(from).not.toHaveAttribute("aria-invalid");
    expect(to).not.toHaveAttribute("aria-invalid");
  });

  it("announces the same field binding in zh-CN", async () => {
    const client = {
      searchAuditEvents: vi.fn().mockResolvedValue(emptyAuditPage()),
    } as unknown as ControlPlaneClient;
    render(createElement(AuditView, {
      client,
      locale: "zh-CN",
      t: translator("zh-CN"),
    }));
    await screen.findByRole("region", { name: "查询上下文" });

    fireEvent.click(screen.getByText("更多筛选"));
    fireEvent.change(screen.getByRole("textbox", { name: "资源 ID" }), {
      target: { value: "case-1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "执行检索" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("填写资源 ID 时必须同时填写资源类型。");
    const resourceType = screen.getByRole("textbox", { name: "资源类型" });
    expect(resourceType).toHaveAttribute("aria-invalid", "true");
    expect(resourceType.getAttribute("aria-describedby") ?? "")
      .toContain(alert.id);
    await waitFor(() => expect(resourceType).toHaveFocus());
  });
});

describe("AuditView list states", () => {
  it("does not turn an initial 503 into an empty audit result", async () => {
    const searchAuditEvents = vi.fn()
      .mockRejectedValueOnce(new ApiError(503, {
        code: "audit_unavailable",
        message: "audit search is temporarily unavailable",
        request_id: "audit-request-503",
        trace_id: "audit-trace-503",
        retryable: true,
      }))
      .mockResolvedValueOnce({
        items: [],
        has_more: false,
        next_cursor: null,
        query_id: "019b0000-0000-7000-8000-000000000605",
        occurred_from: "2026-08-22T08:00:00Z",
        occurred_to: "2026-08-23T08:00:00Z",
      });
    const client = { searchAuditEvents } as unknown as ControlPlaneClient;

    render(createElement(AuditView, { client, locale: "en", t: translator("en") }));

    expect(await screen.findByRole("alert")).toHaveTextContent("audit_unavailable");
    expect(screen.queryByText("No audit events match these filters", { exact: true }))
      .not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("No audit events match these filters", { exact: true }))
      .toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(searchAuditEvents).toHaveBeenCalledTimes(2);
  });
});
