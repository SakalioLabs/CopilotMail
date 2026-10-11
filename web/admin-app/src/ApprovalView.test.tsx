// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, ControlPlaneClient } from "./api";
import ApprovalView from "./ApprovalView";
import { translator } from "./i18n";
import type { ApprovalRequest, ApprovalRequestDetail, Page } from "./types";

const en = translator("en");

function approvalRequest(
  id: string,
  action = "audit.export",
): ApprovalRequest {
  return {
    approval_request_id: id,
    organization_id: "organization-1",
    policy_version_id: "policy-version-1",
    action,
    resource_kind: "audit_archive",
    resource_id: `archive-for-${id}`,
    requested_by: "principal-requester",
    reason: `Reason for ${id}`,
    ticket_reference: null,
    required_approvals: 2,
    approval_count: 1,
    rejection_count: 0,
    status: "pending",
    created_at: "2026-08-24T01:00:00Z",
    expires_at: "2026-08-24T02:00:00Z",
    completed_at: null,
    cancelled_at: null,
  };
}

function detail(request: ApprovalRequest): ApprovalRequestDetail {
  return { request, steps: [] };
}

function page(items: ApprovalRequest[]): Page<ApprovalRequest> {
  return { items, next_cursor: null, has_more: false };
}

function futureLocalDateTime(hours = 24): string {
  const value = new Date(Date.now() + hours * 60 * 60 * 1_000);
  const local = new Date(value.getTime() - value.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function fillApprovalCreateForm(expiresAt = futureLocalDateTime()): void {
  fireEvent.change(screen.getByRole("textbox", { name: /^Policy key/ }), {
    target: { value: "audit.export" },
  });
  fireEvent.change(screen.getByLabelText("High-risk action"), {
    target: { value: "audit.export" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: /^Resource kind/ }), {
    target: { value: "audit_archive" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: /^Resource ID/ }), {
    target: { value: "archive-42" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: /^Reason/ }), {
    target: { value: "Export evidence for case AC-42" },
  });
  fireEvent.change(screen.getByLabelText("Ticket reference (optional)"), {
    target: { value: "AC-42" },
  });
  fireEvent.change(screen.getByLabelText(/^Expires/), {
    target: { value: expiresAt },
  });
}

function unavailable(code: string): ApiError {
  return new ApiError(503, {
    code,
    message: "internal postgres host db.production.example leaked",
    request_id: "request-safe-503",
    trace_id: "trace-secret-503",
    retryable: true,
  });
}

function installNarrowMatchMedia(): void {
  vi.stubGlobal("matchMedia", vi.fn(() => ({
    matches: true,
    media: "(max-width: 1080px)",
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })));
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("ApprovalView truthful asynchronous states", () => {
  it("creates an exact request, switches to My requests, and reads it by server ID", async () => {
    const createdId = "approval-request-created-by-server";
    const createdSummary = {
      ...approvalRequest(createdId),
      resource_id: "archive-42",
      reason: "Export evidence for case AC-42",
      ticket_reference: "AC-42",
    };
    const listApprovalRequests = vi.fn().mockResolvedValue(page([]));
    const createApprovalRequest = vi.fn().mockResolvedValue({
      approval_request_id: createdId,
      status: "pending",
    });
    const getApprovalRequest = vi.fn().mockResolvedValue(detail(createdSummary));
    const client = {
      listApprovalRequests,
      createApprovalRequest,
      getApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    const trigger = screen.getByRole("button", { name: "Request approval" });
    fireEvent.click(trigger);

    expect(screen.getByRole("textbox", { name: /^Policy key/ })).toHaveFocus();
    const expiresAt = futureLocalDateTime();
    fillApprovalCreateForm(expiresAt);
    fireEvent.click(screen.getByRole("button", { name: "Submit approval request" }));

    await waitFor(() => {
      expect(createApprovalRequest).toHaveBeenCalledWith({
        policy_key: "audit.export",
        action: "audit.export",
        resource_kind: "audit_archive",
        resource_id: "archive-42",
        reason: "Export evidence for case AC-42",
        ticket_reference: "AC-42",
        expires_at: new Date(expiresAt).toISOString(),
      });
    });
    const createdNotice = await screen.findByRole("status", {
      name: "Approval request created",
    });
    expect(createdNotice).toHaveTextContent(createdId);
    expect(createdNotice).toHaveTextContent("Pending");
    expect(createdNotice).toHaveFocus();
    expect(screen.getByRole("combobox", { name: "Work view" })).toHaveValue(
      "requested",
    );
    expect(await screen.findByText("Export evidence for case AC-42")).toBeVisible();
    expect(getApprovalRequest).toHaveBeenCalledWith(createdId);
    expect(listApprovalRequests).toHaveBeenLastCalledWith({
      view: "requested",
      status: undefined,
      cursor: undefined,
      limit: 50,
    });
  });

  it("does not select a late created-request result after returning to actionable work", async () => {
    const createdId = "approval-request-late-created";
    const createdSummary = approvalRequest(createdId);
    let resolveRequested: ((value: Page<ApprovalRequest>) => void) | undefined;
    const requestedPage = new Promise<Page<ApprovalRequest>>((resolve) => {
      resolveRequested = resolve;
    });
    const listApprovalRequests = vi.fn((input: { view: string }) => (
      input.view === "requested" ? requestedPage : Promise.resolve(page([]))
    ));
    const getApprovalRequest = vi.fn().mockResolvedValue(detail(createdSummary));
    const client = {
      listApprovalRequests,
      createApprovalRequest: vi.fn().mockResolvedValue({
        approval_request_id: createdId,
        status: "pending",
      }),
      getApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.click(screen.getByRole("button", { name: "Request approval" }));
    fillApprovalCreateForm();
    fireEvent.click(screen.getByRole("button", { name: "Submit approval request" }));

    await screen.findByRole("status", { name: "Approval request created" });
    await waitFor(() => {
      expect(listApprovalRequests).toHaveBeenCalledWith({
        view: "requested",
        status: undefined,
        cursor: undefined,
        limit: 50,
      });
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "actionable" },
    });
    expect(await screen.findByText("No approval requests")).toBeVisible();

    resolveRequested?.(page([createdSummary]));
    await waitFor(() => {
      expect(getApprovalRequest).not.toHaveBeenCalled();
      expect(screen.queryByText(createdSummary.reason)).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Reject" })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("combobox", { name: "Work view" })).toHaveValue(
      "actionable",
    );
  });

  it("freezes an uncertain create without resending when no exact read target exists", async () => {
    const createApprovalRequest = vi.fn().mockRejectedValue(
      unavailable("approval_request_unavailable"),
    );
    const listApprovalRequests = vi.fn().mockResolvedValue(page([]));
    const client = {
      listApprovalRequests,
      createApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.click(screen.getByRole("button", { name: "Request approval" }));
    fillApprovalCreateForm();
    fireEvent.click(screen.getByRole("button", { name: "Submit approval request" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("approval_request_unavailable");
    expect(alert).toHaveTextContent("request-safe-503");
    expect(alert).not.toHaveTextContent("db.production.example");
    expect(alert).not.toHaveTextContent("trace-secret-503");
    expect(screen.getByRole("heading", { name: "Create approval request" })).toBeVisible();
    expect(screen.getByRole("textbox", { name: /^Policy key/ })).toHaveValue(
      "audit.export",
    );
    expect(screen.getByRole("status", { name: en("approvalWriteOutcomeUnknown") }))
      .toHaveAttribute("data-write-state", "unknown");
    expect(screen.getByRole("textbox", { name: /^Policy key/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Work view" })).toBeDisabled();

    expect(createApprovalRequest).toHaveBeenCalledTimes(1);
    expect(listApprovalRequests).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Reconcile read-only state" }))
      .not.toBeInTheDocument();
    expect(screen.getByText(/did not return an exact request ID/i)).toBeVisible();
    expect(screen.getByRole("status", { name: en("approvalWriteOutcomeUnknown") }))
      .toBeVisible();
  });

  it("rejects an invalid policy key locally and restores trigger focus on cancel", async () => {
    const createApprovalRequest = vi.fn();
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([])),
      createApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    const trigger = screen.getByRole("button", { name: "Request approval" });
    fireEvent.click(trigger);
    fillApprovalCreateForm();
    fireEvent.change(screen.getByRole("textbox", { name: /^Policy key/ }), {
      target: { value: "Audit Export" },
    });
    fireEvent.submit(screen.getByRole("button", {
      name: "Submit approval request",
    }).closest("form")!);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A policy key may contain only lowercase letters",
    );
    expect(createApprovalRequest).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(screen.queryByRole("heading", {
      name: "Create approval request",
    })).not.toBeInTheDocument();
  });

  it("shows only a bounded retryable error after an initial list failure", async () => {
    const listApprovalRequests = vi.fn()
      .mockRejectedValueOnce(unavailable("approval_list_unavailable"))
      .mockResolvedValueOnce(page([]));
    const client = { listApprovalRequests } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("approval_list_unavailable");
    expect(alert).toHaveTextContent("request-safe-503");
    expect(alert).not.toHaveTextContent("db.production.example");
    expect(alert).not.toHaveTextContent("trace-secret-503");
    expect(screen.queryByText("No approval requests")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("No approval requests")).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(listApprovalRequests).toHaveBeenCalledTimes(2);
  });

  it("replaces a failed detail spinner with a retry and then confirmed data", async () => {
    const request = approvalRequest("approval-request-detail");
    const listApprovalRequests = vi.fn().mockResolvedValue(page([request]));
    const getApprovalRequest = vi.fn()
      .mockRejectedValueOnce(unavailable("approval_detail_unavailable"))
      .mockResolvedValueOnce(detail(request));
    const client = {
      listApprovalRequests,
      getApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("approval_detail_unavailable");
    expect(alert).not.toHaveTextContent("db.production.example");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("Reason for approval-request-detail")).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(getApprovalRequest).toHaveBeenCalledTimes(2);
  });

  it("keeps the last confirmed page visible when a manual refresh fails", async () => {
    const request = approvalRequest("approval-request-confirmed");
    const listApprovalRequests = vi.fn()
      .mockResolvedValueOnce(page([request]))
      .mockRejectedValueOnce(unavailable("approval_refresh_unavailable"));
    const client = { listApprovalRequests } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    expect(await screen.findByRole("button", { name: "audit.export" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "approval_refresh_unavailable",
    );
    expect(screen.getByRole("button", { name: "audit.export" })).toBeVisible();
    expect(screen.queryByText("No approval requests")).not.toBeInTheDocument();
  });

  it("clears a page from the previous view while the new query is unresolved", async () => {
    const prior = approvalRequest("approval-request-prior-view");
    let resolveRequested: ((value: Page<ApprovalRequest>) => void) | undefined;
    const requestedPage = new Promise<Page<ApprovalRequest>>((resolve) => {
      resolveRequested = resolve;
    });
    const listApprovalRequests = vi.fn()
      .mockResolvedValueOnce(page([prior]))
      .mockReturnValueOnce(requestedPage);
    const client = { listApprovalRequests } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    expect(await screen.findByRole("button", { name: "audit.export" })).toBeVisible();

    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });

    expect(screen.queryByRole("button", { name: "audit.export" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading");

    resolveRequested?.(page([]));
    expect(await screen.findByText("No approval requests")).toBeVisible();
    expect(listApprovalRequests).toHaveBeenLastCalledWith({
      view: "requested",
      status: undefined,
      cursor: undefined,
      limit: 50,
    });
  });

  it("puts exact approval facts before actions and keeps immutable evidence closed", async () => {
    const request = {
      ...approvalRequest("approval-request-decision-first"),
      resource_id: "archive-decision-first",
      requested_by: "principal-requester-exact",
      reason: "Export the signed archive after an independent review.",
    };
    const requestDetail: ApprovalRequestDetail = {
      request,
      steps: [{
        approval_step_id: "approval-step-evidence-1",
        approver_principal_id: "principal-independent-approver",
        decision: "approved",
        reason: "Independent evidence checked.",
        decided_at: "2026-08-24T01:15:00Z",
      }],
    };
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest: vi.fn().mockResolvedValue(requestDetail),
    } as unknown as ControlPlaneClient;

    const { container } = render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));

    const context = await screen.findByRole("region", {
      name: "Current approval context",
    });
    expect(within(context).getByText("Needs my decision")).toBeVisible();
    expect(within(context).getByText("Pending", {
      selector: ".context-bar__value",
    })).toBeVisible();
    expect(within(context).getByText("audit_archive")).toBeVisible();
    expect(within(context).getByText("archive-decision-first")).toBeVisible();
    expect(within(context).getByText("principal-requester-exact")).toBeVisible();
    expect(within(context).getByRole("button", { name: "Request approval" })).toBeVisible();

    const impact = screen.getByRole("heading", {
      level: 3,
      name: "What this decision changes",
    });
    const boundary = screen.getByRole("heading", {
      level: 3,
      name: "Resource and decision boundary",
    });
    const evidence = container.querySelector<HTMLDetailsElement>(
      ".approval-evidence-inspector",
    );
    const summary = evidence?.querySelector<HTMLElement>("summary");
    if (!evidence || !summary) throw new Error("expected approval evidence disclosure");

    expect(impact.compareDocumentPosition(boundary) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(boundary.compareDocumentPosition(evidence) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(evidence).not.toHaveAttribute("open");
    expect(screen.getByText("approval-step-evidence-1")).not.toBeVisible();

    fireEvent.click(summary);
    expect(evidence).toHaveAttribute("open");
    expect(screen.getByText(request.approval_request_id)).toBeVisible();
    expect(screen.getByText(request.policy_version_id)).toBeVisible();
    expect(screen.getByText("approval-step-evidence-1")).toBeVisible();
    expect(screen.getByText("principal-independent-approver")).toBeVisible();
    expect(screen.getByText("Independent evidence checked.")).toBeVisible();
  });

  it("keeps My requests separate from decision actions using the requested server view", async () => {
    const request = approvalRequest("approval-request-requested-view");
    const listApprovalRequests = vi.fn(({ view }: { view: string }) => Promise.resolve(
      page(view === "requested" ? [request] : []),
    ));
    const client = {
      listApprovalRequests,
      getApprovalRequest: vi.fn().mockResolvedValue(detail(request)),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));

    const context = await screen.findByRole("region", {
      name: "Current approval context",
    });
    expect(within(context).getByText("My requests")).toBeVisible();
    expect(within(context).getByText("All")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Available action" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Withdraw request" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Record this decision" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Decision reason" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reject" })).not.toBeInTheDocument();
    expect(listApprovalRequests).toHaveBeenLastCalledWith({
      view: "requested",
      status: undefined,
      cursor: undefined,
      limit: 50,
    });
  });

  it("ignores an older detail response after the operator selects another request", async () => {
    const first = approvalRequest("approval-request-first", "case.close");
    const second = approvalRequest("approval-request-second", "hold.release");
    let resolveFirst: ((value: ApprovalRequestDetail) => void) | undefined;
    const firstDetail = new Promise<ApprovalRequestDetail>((resolve) => {
      resolveFirst = resolve;
    });
    const getApprovalRequest = vi.fn((id: string) => (
      id === first.approval_request_id
        ? firstDetail
        : Promise.resolve(detail(second))
    ));
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([first, second])),
      getApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "case.close" }));
    fireEvent.click(screen.getByRole("button", { name: "hold.release" }));

    expect(await screen.findByText("Reason for approval-request-second")).toBeVisible();
    resolveFirst?.(detail(first));
    await waitFor(() => {
      expect(screen.queryByText("Reason for approval-request-first")).not.toBeInTheDocument();
    });
    expect(screen.getByText("Reason for approval-request-second")).toBeVisible();
  });

  it("dismisses withdrawal safely and restores the original trigger without an API call", async () => {
    const request = approvalRequest("approval-request-withdraw-cancel");
    const cancelApprovalRequest = vi.fn();
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest: vi.fn().mockResolvedValue(detail(request)),
      cancelApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    const trigger = await screen.findByRole("button", { name: "Withdraw request" });
    trigger.focus();
    fireEvent.click(trigger);

    const dialog = screen.getByRole("dialog", { name: "Withdraw request" });
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    await waitFor(() => expect(cancel).toHaveFocus());
    fireEvent.click(cancel);

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(trigger).toHaveFocus();
    expect(cancelApprovalRequest).not.toHaveBeenCalled();
  });

  it("withdraws the captured request exactly once and cannot dismiss the busy dialog", async () => {
    const request = approvalRequest("approval-request-withdraw-confirm");
    let resolveCancellation: ((value: {
      approval_request_id: string;
      status: "cancelled";
      approval_count: number;
    }) => void) | undefined;
    const cancelApprovalRequest = vi.fn().mockImplementation(() => new Promise((resolve) => {
      resolveCancellation = resolve;
    }));
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest: vi.fn().mockResolvedValue(detail(request)),
      cancelApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.click(await screen.findByRole("button", { name: "Withdraw request" }));
    const dialog = screen.getByRole("dialog", { name: "Withdraw request" });
    const confirm = within(dialog).getByRole("button", { name: "Withdraw request" });

    fireEvent.click(confirm);
    fireEvent.click(confirm);
    fireEvent.keyDown(document, { key: "Escape" });

    expect(cancelApprovalRequest).toHaveBeenCalledTimes(1);
    expect(cancelApprovalRequest).toHaveBeenCalledWith(request.approval_request_id);
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();

    resolveCancellation?.({
      approval_request_id: request.approval_request_id,
      status: "cancelled",
      approval_count: request.approval_count,
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(cancelApprovalRequest).toHaveBeenCalledTimes(1);
  });

  it("freezes every write after an uncertain decision and reconciles without resending", async () => {
    const request = {
      ...approvalRequest("approval-request-uncertain-decision"),
      approval_count: 0,
      required_approvals: 3,
    };
    const decideApprovalRequest = vi.fn()
      .mockRejectedValueOnce(new TypeError("connection closed"));
    const reconciled = { ...request, approval_count: 1 };
    const getApprovalRequest = vi.fn()
      .mockResolvedValueOnce(detail(request))
      .mockResolvedValueOnce(detail(reconciled));
    const createApprovalRequest = vi.fn();
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest,
      decideApprovalRequest,
      createApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "Request approval" }));
    fillApprovalCreateForm();
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    const reason = await screen.findByRole("textbox", { name: "Decision reason" });
    fireEvent.change(reason, { target: { value: "Verified immutable evidence" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    const notice = await screen.findByRole("status", {
      name: en("approvalWriteOutcomeUnknown"),
    });
    expect(notice).toHaveTextContent(request.approval_request_id);
    expect(notice).toHaveTextContent("Verified immutable evidence");
    expect(reason).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Work view" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "audit.export" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: /^Policy key/ })).toBeDisabled();
    const createSubmit = screen.getByRole("button", { name: "Submit approval request" });
    expect(createSubmit).toBeDisabled();
    fireEvent.submit(createSubmit.closest("form")!);
    expect(createApprovalRequest).not.toHaveBeenCalled();

    fireEvent.click(within(notice).getByRole("button", {
      name: "Reconcile read-only state",
    }));
    await waitFor(() => expect(getApprovalRequest).toHaveBeenCalledTimes(2));
    expect(decideApprovalRequest).toHaveBeenCalledTimes(1);
    expect(decideApprovalRequest).toHaveBeenCalledWith(
      request.approval_request_id,
      "approved",
      "Verified immutable evidence",
      { approval_count: 0, rejection_count: 0, required_approvals: 3 },
    );
    expect(createApprovalRequest).not.toHaveBeenCalled();
    expect(screen.getByText(/Pending · 1/)).toBeVisible();
    expect(screen.getByRole("status", { name: en("approvalWriteOutcomeUnknown") }))
      .toBeVisible();
  });

  it("does not freeze a definitively rejected decision", async () => {
    const request = approvalRequest("approval-request-denied-decision");
    const decideApprovalRequest = vi.fn().mockRejectedValue(new ApiError(409, {
      code: "approval_conflict",
      message: "private server details",
      request_id: "request-decision-conflict",
      trace_id: "private-trace",
      retryable: false,
    }));
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest: vi.fn().mockResolvedValue(detail(request)),
      decideApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    const reason = await screen.findByRole("textbox", { name: "Decision reason" });
    fireEvent.change(reason, { target: { value: "Evidence checked" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("approval_conflict");
    expect(screen.queryByRole("status", { name: en("approvalWriteOutcomeUnknown") }))
      .not.toBeInTheDocument();
    expect(reason).toBeEnabled();
    expect(screen.getByRole("combobox", { name: "Work view" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
  });

  it("freezes an uncertain withdrawal and reconciles it without a second POST", async () => {
    const request = approvalRequest("approval-request-uncertain-withdrawal");
    const cancelApprovalRequest = vi.fn()
      .mockRejectedValueOnce(new TypeError("connection closed"));
    const cancelled = {
      ...request,
      status: "cancelled" as const,
      cancelled_at: "2026-08-24T01:30:00Z",
    };
    const getApprovalRequest = vi.fn()
      .mockResolvedValueOnce(detail(request))
      .mockResolvedValueOnce(detail(cancelled));
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest,
      cancelApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.click(await screen.findByRole("button", { name: "Withdraw request" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Withdraw request" }))
      .getByRole("button", { name: "Withdraw request" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    const notice = screen.getByRole("status", {
      name: en("approvalWriteOutcomeUnknown"),
    });
    expect(notice).toHaveTextContent(request.approval_request_id);
    expect(screen.getByRole("combobox", { name: "Work view" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "audit.export" })).toBeDisabled();

    fireEvent.click(within(notice).getByRole("button", {
      name: "Reconcile read-only state",
    }));
    await waitFor(() => expect(getApprovalRequest).toHaveBeenCalledTimes(2));
    expect(cancelApprovalRequest).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status", { name: en("approvalWriteOutcomeUnknown") }))
      .toBeVisible();
    expect(screen.getByText(/Cancelled · 1/)).toBeVisible();
    expect(screen.getAllByText("Cancelled").length).toBeGreaterThan(0);
  });

  it("keeps a committed decision authoritative when its detail refresh fails", async () => {
    const request = {
      ...approvalRequest("approval-request-decision-committed"),
      approval_count: 0,
      required_approvals: 3,
    };
    const getApprovalRequest = vi.fn()
      .mockResolvedValueOnce(detail(request))
      .mockRejectedValueOnce(unavailable("approval_detail_refresh_unavailable"));
    const decideApprovalRequest = vi.fn().mockResolvedValue({
      approval_request_id: request.approval_request_id,
      status: "pending",
      approval_count: 1,
    });
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest,
      decideApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Decision reason" }), {
      target: { value: "Verified independent evidence" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    expect(await screen.findByText("Your decision was recorded")).toBeVisible();
    expect(screen.getByText("1/3")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reject" })).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "approval_detail_refresh_unavailable",
    );
    expect(decideApprovalRequest).toHaveBeenCalledWith(
      request.approval_request_id,
      "approved",
      "Verified independent evidence",
      { approval_count: 0, rejection_count: 0, required_approvals: 3 },
    );
    await waitFor(() => {
      expect(document.querySelector(".approval-detail .admin-list-detail__title"))
        .toHaveFocus();
    });
  });

  it("keeps a committed withdrawal visible and focuses stable detail when refresh fails", async () => {
    const request = approvalRequest("approval-request-cancel-committed");
    const getApprovalRequest = vi.fn()
      .mockResolvedValueOnce(detail(request))
      .mockRejectedValueOnce(unavailable("approval_cancel_refresh_unavailable"));
    const cancelApprovalRequest = vi.fn().mockResolvedValue({
      approval_request_id: request.approval_request_id,
      status: "cancelled",
      approval_count: request.approval_count,
    });
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest,
      cancelApprovalRequest,
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.click(await screen.findByRole("button", { name: "Withdraw request" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Withdraw request" }))
      .getByRole("button", { name: "Withdraw request" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getAllByText("Cancelled").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "Withdraw request" }))
      .not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "approval_cancel_refresh_unavailable",
    );
    await waitFor(() => {
      expect(document.querySelector(".approval-detail .admin-list-detail__title"))
        .toHaveFocus();
    });
    expect(cancelApprovalRequest).toHaveBeenCalledWith(
      request.approval_request_id,
    );
  });

  it("cannot run an actionable reload captured before a decision after the query switches", async () => {
    const request = {
      ...approvalRequest("approval-request-decision-query-race"),
      required_approvals: 3,
    };
    const requested = approvalRequest(
      "approval-request-current-requested-query",
      "hold.release",
    );
    let resolveRefresh: ((value: ApprovalRequestDetail) => void) | undefined;
    const refresh = new Promise<ApprovalRequestDetail>((resolve) => {
      resolveRefresh = resolve;
    });
    const listApprovalRequests = vi.fn((input: { view: string }) => (
      Promise.resolve(input.view === "actionable" ? page([request]) : page([requested]))
    ));
    const getApprovalRequest = vi.fn()
      .mockResolvedValueOnce(detail(request))
      .mockReturnValueOnce(refresh);
    const client = {
      listApprovalRequests,
      getApprovalRequest,
      decideApprovalRequest: vi.fn().mockResolvedValue({
        approval_request_id: request.approval_request_id,
        status: "pending",
        approval_count: 2,
      }),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Decision reason" }), {
      target: { value: "Independent evidence verified" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(getApprovalRequest).toHaveBeenCalledTimes(2));

    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    expect(await screen.findByRole("button", { name: "hold.release" })).toBeVisible();

    await act(async () => {
      resolveRefresh?.(detail({
        ...request,
        approval_count: 0,
      }));
      await refresh;
      await Promise.resolve();
    });

    expect(listApprovalRequests).toHaveBeenCalledTimes(2);
    expect(listApprovalRequests).toHaveBeenLastCalledWith({
      view: "requested",
      status: undefined,
      cursor: undefined,
      limit: 50,
    });
    expect(screen.getByRole("button", { name: "hold.release" })).toBeVisible();
    expect(screen.queryByText(request.reason)).not.toBeInTheDocument();
  });

  it("cannot let an in-flight post-decision list reload overwrite a newer query", async () => {
    const request = {
      ...approvalRequest("approval-request-inflight-list-race"),
      required_approvals: 3,
    };
    const requested = approvalRequest(
      "approval-request-newer-query",
      "hold.release",
    );
    let actionableReads = 0;
    let resolveOldReload: ((value: Page<ApprovalRequest>) => void) | undefined;
    const oldReload = new Promise<Page<ApprovalRequest>>((resolve) => {
      resolveOldReload = resolve;
    });
    const listApprovalRequests = vi.fn((input: { view: string }) => {
      if (input.view === "requested") return Promise.resolve(page([requested]));
      actionableReads += 1;
      return actionableReads === 1 ? Promise.resolve(page([request])) : oldReload;
    });
    const client = {
      listApprovalRequests,
      getApprovalRequest: vi.fn().mockResolvedValue(detail(request)),
      decideApprovalRequest: vi.fn().mockResolvedValue({
        approval_request_id: request.approval_request_id,
        status: "pending",
        approval_count: 2,
      }),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Decision reason" }), {
      target: { value: "Reload race verified" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(listApprovalRequests).toHaveBeenCalledTimes(2));

    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    expect(await screen.findByRole("button", { name: "hold.release" })).toBeVisible();

    await act(async () => {
      resolveOldReload?.(page([request]));
      await oldReload;
      await Promise.resolve();
    });

    expect(listApprovalRequests).toHaveBeenCalledTimes(3);
    expect(screen.getByRole("button", { name: "hold.release" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "audit.export" }))
      .not.toBeInTheDocument();
  });

  it("cannot run a requested reload captured before withdrawal after the query switches", async () => {
    const request = approvalRequest("approval-request-withdraw-query-race");
    let resolveRefresh: ((value: ApprovalRequestDetail) => void) | undefined;
    const refresh = new Promise<ApprovalRequestDetail>((resolve) => {
      resolveRefresh = resolve;
    });
    const listApprovalRequests = vi.fn((input: { view: string }) => (
      Promise.resolve(input.view === "requested" ? page([request]) : page([]))
    ));
    const getApprovalRequest = vi.fn()
      .mockResolvedValueOnce(detail(request))
      .mockReturnValueOnce(refresh);
    const client = {
      listApprovalRequests,
      getApprovalRequest,
      cancelApprovalRequest: vi.fn().mockResolvedValue({
        approval_request_id: request.approval_request_id,
        status: "cancelled",
        approval_count: request.approval_count,
      }),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    const workView = screen.getByRole("combobox", { name: "Work view" });
    fireEvent.change(workView, { target: { value: "requested" } });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.click(await screen.findByRole("button", { name: "Withdraw request" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Withdraw request" }))
      .getByRole("button", { name: "Withdraw request" }));
    await waitFor(() => expect(getApprovalRequest).toHaveBeenCalledTimes(2));

    // The modal makes the background inert in a browser. Dispatching the
    // controlled transition directly proves that even host-driven navigation
    // invalidates the write's old query closure.
    fireEvent.change(workView, { target: { value: "actionable" } });
    expect(await screen.findByText("No approval requests")).toBeVisible();

    await act(async () => {
      resolveRefresh?.(detail({
        ...request,
        status: "pending",
        approval_count: 0,
      }));
      await refresh;
      await Promise.resolve();
    });

    expect(listApprovalRequests).toHaveBeenCalledTimes(3);
    expect(listApprovalRequests).toHaveBeenLastCalledWith({
      view: "actionable",
      status: undefined,
      cursor: undefined,
      limit: 50,
    });
    expect(screen.queryByRole("button", { name: "audit.export" }))
      .not.toBeInTheDocument();
  });

  it("advances a committed decision to a newer terminal detail and rejects stale list rollback", async () => {
    const request = {
      ...approvalRequest("approval-request-monotonic-decision"),
      approval_count: 1,
      required_approvals: 3,
    };
    const terminal = {
      ...request,
      approval_count: 3,
      status: "approved" as const,
      completed_at: "2026-08-24T01:30:00Z",
    };
    const stale = {
      ...request,
      approval_count: 0,
      status: "pending" as const,
    };
    const listApprovalRequests = vi.fn()
      .mockResolvedValueOnce(page([request]))
      .mockResolvedValueOnce(page([stale]));
    const getApprovalRequest = vi.fn()
      .mockResolvedValueOnce(detail(request))
      .mockResolvedValueOnce(detail(terminal));
    const client = {
      listApprovalRequests,
      getApprovalRequest,
      decideApprovalRequest: vi.fn().mockResolvedValue({
        approval_request_id: request.approval_request_id,
        status: "pending",
        approval_count: 2,
      }),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Decision reason" }), {
      target: { value: "Second approval recorded" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(listApprovalRequests).toHaveBeenCalledTimes(2));
    expect(screen.getAllByText("Approved").length).toBeGreaterThan(0);
    expect(screen.getByText("3/3")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reject" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "audit.export" }))
      .not.toBeInTheDocument();
  });

  it("reconciles a stale selected detail against the terminal state already observed in its list", async () => {
    const terminal = {
      ...approvalRequest("approval-request-terminal-before-detail"),
      status: "approved" as const,
      approval_count: 2,
      completed_at: "2026-08-24T01:30:00Z",
    };
    const staleDetail = {
      ...terminal,
      status: "pending" as const,
      approval_count: 0,
      completed_at: null,
    };
    const client = {
      listApprovalRequests: vi.fn((input: { view: string }) => Promise.resolve(
        input.view === "requested" ? page([terminal]) : page([]),
      )),
      getApprovalRequest: vi.fn().mockResolvedValue(detail(staleDetail)),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));

    expect(await screen.findByText("2/2")).toBeVisible();
    expect(screen.getAllByText("Approved").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "Withdraw request" }))
      .not.toBeInTheDocument();
  });

  it("propagates a newer terminal detail back into the stale visible list row", async () => {
    const stale = {
      ...approvalRequest("approval-request-detail-advances-list"),
      status: "pending" as const,
      approval_count: 0,
    };
    const terminal = {
      ...stale,
      status: "approved" as const,
      approval_count: 2,
      completed_at: "2026-08-24T01:30:00Z",
    };
    const client = {
      listApprovalRequests: vi.fn((input: { view: string }) => Promise.resolve(
        input.view === "requested" ? page([stale]) : page([]),
      )),
      getApprovalRequest: vi.fn().mockResolvedValue(detail(terminal)),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));

    await waitFor(() => {
      expect(screen.getAllByText("Approved", { selector: ".badge" })).toHaveLength(2);
      expect(screen.getAllByText("2/2")).toHaveLength(2);
    });
    expect(screen.queryByRole("button", { name: "Withdraw request" }))
      .not.toBeInTheDocument();
  });

  it("keeps a committed cancellation terminal across stale detail, list, and status-filter reads", async () => {
    const stale = {
      ...approvalRequest("approval-request-monotonic-cancellation"),
      approval_count: 0,
      status: "pending" as const,
    };
    const listApprovalRequests = vi.fn((input: { view: string }) => Promise.resolve(
      input.view === "requested" ? page([stale]) : page([]),
    ));
    const getApprovalRequest = vi.fn().mockResolvedValue(detail(stale));
    const client = {
      listApprovalRequests,
      getApprovalRequest,
      cancelApprovalRequest: vi.fn().mockResolvedValue({
        approval_request_id: stale.approval_request_id,
        status: "cancelled",
        approval_count: 1,
      }),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    fireEvent.click(await screen.findByRole("button", { name: "Withdraw request" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Withdraw request" }))
      .getByRole("button", { name: "Withdraw request" }));

    await waitFor(() => expect(listApprovalRequests).toHaveBeenCalledTimes(3));
    expect(screen.getAllByText("Cancelled").length).toBeGreaterThan(0);
    expect(screen.getAllByText("1/2").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "Withdraw request" }))
      .not.toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), {
      target: { value: "pending" },
    });
    expect(await screen.findByText("No approval requests")).toBeVisible();
    expect(screen.queryByRole("button", { name: "audit.export" }))
      .not.toBeInTheDocument();
  });

  it("advances approved to expired on refresh and retains it under the Expired filter", async () => {
    const approved = {
      ...approvalRequest("approval-request-approved-then-expired"),
      status: "approved" as const,
      approval_count: 2,
      completed_at: "2026-08-24T01:30:00Z",
    };
    const expired = {
      ...approved,
      status: "expired" as const,
    };
    let requestedReads = 0;
    const listApprovalRequests = vi.fn((input: {
      view: string;
      status?: string;
    }) => {
      if (input.view !== "requested") return Promise.resolve(page([]));
      requestedReads += 1;
      if (input.status === "expired") {
        // Even a lagging projection cannot roll the lifecycle back or make
        // the row disappear from the requested/expired result.
        return Promise.resolve(page([approved]));
      }
      return Promise.resolve(page([requestedReads === 1 ? approved : expired]));
    });
    const client = {
      listApprovalRequests,
      getApprovalRequest: vi.fn().mockResolvedValue(detail(approved)),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));
    expect(await screen.findByText(approved.reason)).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(listApprovalRequests).toHaveBeenCalledTimes(3);
      expect(screen.getAllByText("Expired", { selector: ".badge" })).toHaveLength(2);
    });

    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), {
      target: { value: "expired" },
    });
    expect(await screen.findByRole("button", { name: "audit.export" })).toBeVisible();
    expect(screen.getAllByText("Expired", { selector: ".badge" })).toHaveLength(1);
    expect(listApprovalRequests).toHaveBeenLastCalledWith({
      view: "requested",
      status: "expired",
      cursor: undefined,
      limit: 50,
    });
  });

  it.each([
    { initial: "rejected" as const, observed: "expired" as const, label: "Rejected" },
    { initial: "cancelled" as const, observed: "approved" as const, label: "Cancelled" },
    { initial: "expired" as const, observed: "approved" as const, label: "Expired" },
  ])("fails closed when $initial conflicts with a later $observed detail", async ({
    initial,
    observed,
    label,
  }) => {
    const request = {
      ...approvalRequest(`approval-request-frozen-${initial}`),
      status: initial,
      approval_count: 1,
    };
    const conflictingDetail = {
      ...request,
      status: observed,
      approval_count: 2,
    };
    const client = {
      listApprovalRequests: vi.fn((input: { view: string }) => Promise.resolve(
        input.view === "requested" ? page([request]) : page([]),
      )),
      getApprovalRequest: vi.fn().mockResolvedValue(detail(conflictingDetail)),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    await screen.findByText("No approval requests");
    fireEvent.change(screen.getByRole("combobox", { name: "Work view" }), {
      target: { value: "requested" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "audit.export" }));

    await waitFor(() => {
      expect(screen.getAllByText(label, { selector: ".badge" })).toHaveLength(2);
      expect(screen.getAllByText("1/2")).toHaveLength(2);
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Approval state evidence conflicts",
    );
    expect(screen.getByRole("combobox", { name: "Work view" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Withdraw request" }))
      .not.toBeInTheDocument();
  });

  it("keeps a stable narrow detail shell and ignores a late response after Back", async () => {
    installNarrowMatchMedia();
    const request = approvalRequest("approval-request-mobile-late-detail");
    let resolveDetail: ((value: ApprovalRequestDetail) => void) | undefined;
    const pendingDetail = new Promise<ApprovalRequestDetail>((resolve) => {
      resolveDetail = resolve;
    });
    const client = {
      listApprovalRequests: vi.fn().mockResolvedValue(page([request])),
      getApprovalRequest: vi.fn().mockReturnValue(pendingDetail),
    } as unknown as ControlPlaneClient;

    render(<ApprovalView client={client} locale="en" t={en} />);
    const row = await screen.findByRole("button", { name: "audit.export" });
    expect(row).not.toHaveAttribute("aria-current");
    row.focus();
    fireEvent.click(row);

    const list = document.querySelector(".admin-list-detail__list");
    const detailPanel = document.querySelector(".admin-list-detail__detail");
    const detailTitle = document.querySelector(".admin-list-detail__title");
    expect(list).toHaveAttribute("hidden");
    expect(detailPanel).not.toHaveAttribute("hidden");
    expect(detailTitle).toHaveFocus();
    expect(row).toHaveAttribute("aria-current", "true");
    expect(within(detailPanel as HTMLElement).getByRole("status"))
      .toHaveTextContent("Loading");

    fireEvent.click(screen.getByRole("button", { name: "Back to approvals" }));
    expect(list).not.toHaveAttribute("hidden");
    expect(detailPanel).toHaveAttribute("hidden");
    expect(row).toHaveFocus();
    expect(row).not.toHaveAttribute("aria-current");

    resolveDetail?.(detail(request));
    await waitFor(() => {
      expect(detailPanel).toHaveAttribute("hidden");
      expect(screen.queryByText(request.reason)).not.toBeInTheDocument();
    });
    expect(row).toHaveFocus();
  });
});
