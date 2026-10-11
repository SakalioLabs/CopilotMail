// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AgentInspectorView from "./AgentInspectorView";
import { ApiError, type ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type { AgentInspector } from "./types";

afterEach(() => cleanup());

function snapshot(agentId: string, displayName: string): AgentInspector {
  return {
    agent_id: agentId,
    display_name: displayName,
    principal_status: "active",
    owner: {
      principal_id: "019b0000-0000-7000-8000-000000000102",
      display_name: "Operations owner",
      principal_status: "active",
    },
    profile: {
      environment: "production",
      risk_level: "medium",
      purpose: "Triage inbound support mail",
      review_due_at: "2026-09-01T00:00:00Z",
      review_state: "scheduled",
    },
    token_access: {
      state: "effective",
      effective_scopes: ["organization.read", "agent.operate"],
      active_credential_count: 2,
      next_credential_expiry_at: "2026-09-15T00:00:00Z",
      last_authenticated_at: null,
    },
    control_plane: {
      mail_admission_state: "active",
      active_processing_subscription_count: 2,
      paused_processing_subscription_count: 1,
      retired_processing_subscription_count: 3,
      last_processing_activity_at: null,
    },
    observed_at: "2026-08-23T09:01:00Z",
  };
}

function view(getAgentInspector: ReturnType<typeof vi.fn>) {
  return render(
    <AgentInspectorView
      client={{ getAgentInspector } as unknown as ControlPlaneClient}
      locale="en"
      t={translator("en")}
    />,
  );
}

function submit(agentId: string) {
  fireEvent.change(screen.getByRole("textbox", { name: /^Agent ID/ }), { target: { value: agentId } });
  fireEvent.click(screen.getByRole("button", { name: "Inspect Agent" }));
}

describe("AgentInspectorView", () => {
  it("does not prefetch or invent a static Agent state", () => {
    const getAgentInspector = vi.fn();
    view(getAgentInspector);

    expect(screen.getByRole("heading", { name: "Agent control-plane inspector" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /^Agent ID/ })).toHaveValue("");
    expect(screen.getByText(/does not prefetch, poll, or build an Agent directory/)).toBeInTheDocument();
    expect(getAgentInspector).not.toHaveBeenCalled();
    expect(screen.queryByText("Inbound triage")).not.toBeInTheDocument();
  });

  it("rejects malformed or reserved IDs locally without creating an audited read", () => {
    const getAgentInspector = vi.fn();
    view(getAgentInspector);

    submit("not-a-uuid");

    const input = screen.getByRole("textbox", { name: /^Agent ID/ });
    expect(getAgentInspector).not.toHaveBeenCalled();
    expect(input).toHaveAttribute("aria-invalid", "true");
    const descriptionIds = input.getAttribute("aria-describedby")?.split(/\s+/u) ?? [];
    expect(descriptionIds).not.toHaveLength(0);
    expect(descriptionIds.some((id) => (
      document.getElementById(id)?.textContent === "Enter a canonical Agent UUID."
    ))).toBe(true);
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a canonical Agent UUID.");

    submit("00000000-0000-0000-0000-000000000000");
    expect(getAgentInspector).not.toHaveBeenCalled();

    submit("019B0000-0000-7000-8000-000000000101");
    expect(getAgentInspector).not.toHaveBeenCalled();
  });

  it("renders only the bounded control-plane contract with meaningful absent records", async () => {
    const result = snapshot("019b0000-0000-7000-8000-000000000101", "Inbound triage");
    const getAgentInspector = vi.fn().mockResolvedValue(result);
    view(getAgentInspector);

    submit(result.agent_id);

    expect(screen.getByText("Loading…")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Inbound triage" })).toBeInTheDocument();
    const context = screen.getByRole("region", { name: "Inbound triage" });
    expect(context).toHaveTextContent("019b0000-0000-7000-8000-000000000101");
    expect(context).toHaveTextContent("Operations owner");
    expect(context).toHaveTextContent("Production");
    expect(context).toHaveTextContent("Snapshot observed");
    // Environment belongs to the selected snapshot context. Repeating it in
    // the profile card makes a bounded record look like two separate facts.
    expect(screen.getAllByText("Production", { exact: true })).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Boundary and review" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Token access intersection" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Mail control plane" })).toBeInTheDocument();
    expect(screen.getByText("organization.read")).toBeInTheDocument();
    expect(screen.getByText("No authentication record")).toBeInTheDocument();
    expect(screen.getByText("No control-plane processing record")).toBeInTheDocument();
    const identityBadges = screen.getAllByText(/^(Agent|Human)$/).map((element) => element.closest(".identity-badge"));
    expect(identityBadges.some((badge) => badge?.getAttribute("data-identity-kind") === "agent")).toBe(true);
    expect(identityBadges.some((badge) => badge?.getAttribute("data-identity-kind") === "human")).toBe(true);
    expect(screen.getByText("Not due").closest(".status-indicator")).toHaveAttribute("data-status-tone", "neutral");
    expect(screen.queryByText("—")).not.toBeInTheDocument();
    expect(screen.queryByText("credential_fingerprint")).not.toBeInTheDocument();
  });

  it("keeps a deliberately uniform unavailable-target response non-retryable and its support request ID", async () => {
    const getAgentInspector = vi.fn().mockRejectedValue(new ApiError(404, {
      code: "agent_inspector_target_unavailable",
      message: "the target has a missing profile",
      request_id: "request-404",
      trace_id: "trace-404",
      retryable: false,
    }));
    view(getAgentInspector);

    submit("019b0000-0000-7000-8000-000000000101");

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("This target cannot be shown");
    expect(alert).toHaveTextContent("request-404");
    expect(alert).not.toHaveTextContent("missing profile");
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });

  it("does not offer a retry for a 403 authorization refusal", async () => {
    const getAgentInspector = vi.fn().mockRejectedValue(new ApiError(403, {
      code: "permission_denied",
      message: "not authorized",
      request_id: "request-403",
      trace_id: "trace-403",
      retryable: false,
    }));
    view(getAgentInspector);

    submit("019b0000-0000-7000-8000-000000000101");

    expect(await screen.findByRole("alert")).toHaveTextContent("Inspector access is not available");
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });

  it("offers retry only when the API marks the failure retryable", async () => {
    const result = snapshot("019b0000-0000-7000-8000-000000000101", "Inbound triage");
    const getAgentInspector = vi.fn()
      .mockRejectedValueOnce(new ApiError(503, {
        code: "unavailable",
        message: "temporary store issue",
        request_id: "request-503",
        trace_id: "trace-503",
        retryable: true,
      }))
      .mockResolvedValueOnce(result);
    view(getAgentInspector);

    submit(result.agent_id);

    const retry = await screen.findByRole("button", { name: "Retry" });
    fireEvent.click(retry);

    expect(await screen.findByRole("heading", { name: "Inbound triage" })).toBeInTheDocument();
    expect(getAgentInspector).toHaveBeenCalledTimes(2);
  });

  it("does not reflect unbounded request IDs or truthy non-boolean retry flags", async () => {
    const getAgentInspector = vi.fn().mockRejectedValue(new ApiError(503, {
      code: "unavailable",
      message: "private infrastructure detail",
      request_id: "request id with spaces and private context",
      trace_id: "trace-private",
      retryable: "true" as unknown as boolean,
    }));
    view(getAgentInspector);

    submit("019b0000-0000-7000-8000-000000000101");

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Snapshot is temporarily unavailable");
    expect(alert).not.toHaveTextContent("request id with spaces");
    expect(alert).not.toHaveTextContent("private infrastructure detail");
    expect(alert).not.toHaveTextContent("trace-private");
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });

  it("keeps the snapshot and refresh control focused while a refresh is in progress", async () => {
    const result = snapshot("019b0000-0000-7000-8000-000000000101", "Inbound triage");
    let resolveRefresh: (value: AgentInspector) => void = () => {};
    const refresh = new Promise<AgentInspector>((resolve) => { resolveRefresh = resolve; });
    const getAgentInspector = vi.fn().mockResolvedValueOnce(result).mockReturnValueOnce(refresh);
    view(getAgentInspector);

    submit(result.agent_id);
    await screen.findByRole("heading", { name: "Inbound triage" });

    const refreshButton = screen.getByRole("button", { name: "Refresh snapshot" });
    refreshButton.focus();
    fireEvent.click(refreshButton);

    expect(screen.getByRole("heading", { name: "Inbound triage" })).toBeInTheDocument();
    expect(refreshButton).toHaveFocus();
    expect(refreshButton).toHaveAttribute("aria-disabled", "true");

    await act(async () => { resolveRefresh({ ...result, observed_at: "2026-08-23T09:02:00Z" }); });

    await waitFor(() => expect(refreshButton).toHaveFocus());
    expect(screen.getByText("Agent snapshot refreshed.")).toBeInTheDocument();
  });

  it("labels a retained snapshot as a prior observation after refresh failure", async () => {
    const result = snapshot("019b0000-0000-7000-8000-000000000101", "Inbound triage");
    const getAgentInspector = vi.fn()
      .mockResolvedValueOnce(result)
      .mockRejectedValueOnce(new ApiError(503, {
        code: "unavailable",
        message: "temporary store issue",
        request_id: "request-refresh-503",
        trace_id: "trace-refresh-503",
        retryable: false,
      }));
    view(getAgentInspector);

    submit(result.agent_id);
    await screen.findByRole("heading", { name: "Inbound triage" });
    const refresh = screen.getByRole("button", { name: "Refresh snapshot" });
    refresh.focus();
    fireEvent.click(refresh);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The last confirmed snapshot remains visible (observed");
    expect(alert).toHaveTextContent("It is not live status.");
    expect(screen.getByRole("heading", { name: "Inbound triage" })).toBeInTheDocument();
    expect(refresh).toHaveFocus();
  });

  it("does not let a late response replace the latest explicitly requested Agent", async () => {
    let resolveFirst: (value: AgentInspector) => void = () => {};
    let resolveSecond: (value: AgentInspector) => void = () => {};
    const first = new Promise<AgentInspector>((resolve) => { resolveFirst = resolve; });
    const second = new Promise<AgentInspector>((resolve) => { resolveSecond = resolve; });
    const firstId = "019b0000-0000-7000-8000-000000000101";
    const secondId = "019b0000-0000-7000-8000-000000000102";
    const getAgentInspector = vi.fn().mockReturnValueOnce(first).mockReturnValueOnce(second);
    view(getAgentInspector);

    submit(firstId);
    submit(secondId);
    await act(async () => { resolveSecond(snapshot(secondId, "Second Agent")); });

    expect(await screen.findByRole("heading", { name: "Second Agent" })).toBeInTheDocument();
    await act(async () => { resolveFirst(snapshot(firstId, "First Agent")); });

    expect(screen.getByRole("heading", { name: "Second Agent" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "First Agent" })).not.toBeInTheDocument();
  });
});
