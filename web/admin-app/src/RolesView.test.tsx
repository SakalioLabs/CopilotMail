// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import RolesView from "./RolesView";
import type { ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type { Role, RoleAssignment } from "./types";

afterEach(() => cleanup());

const role: Role = {
  role_id: "019b0000-0000-7000-8000-000000000401",
  organization_id: "019b0000-0000-7000-8000-000000000400",
  system_role: "mail_admin",
  display_name: "Mail administrator",
  permissions: ["mail.domain.manage"],
  created_at: "2026-08-23T00:00:00Z",
};

function assignment(
  principalKind: RoleAssignment["principal_kind"],
  active: boolean,
): RoleAssignment {
  const suffix = principalKind === "human" ? "1" : principalKind === "agent" ? "2" : "3";

  return {
    role_assignment_id: `019b0000-0000-7000-8000-0000000004${suffix}`,
    organization_id: role.organization_id,
    principal_id: `019b0000-0000-7000-8000-0000000005${suffix}`,
    principal_kind: principalKind,
    principal_status: active ? "active" : "suspended",
    principal_display_name: `${principalKind} principal`,
    role_id: role.role_id,
    system_role: role.system_role,
    role_display_name: role.display_name,
    created_by: "019b0000-0000-7000-8000-000000000499",
    approval_request_id: null,
    valid_from: "2026-08-20T00:00:00Z",
    valid_until: "2026-09-20T00:00:00Z",
    revoked_at: null,
    created_at: "2026-08-20T00:00:00Z",
    active,
  };
}

function view(items: RoleAssignment[]) {
  const client = {
    listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
    listRoleAssignments: vi.fn().mockResolvedValue({ items, has_more: false, next_cursor: null }),
  } as unknown as ControlPlaneClient;

  return render(<RolesView client={client} locale="en" t={translator("en")} />);
}

function reviewGrant() {
  fireEvent.click(screen.getByRole("button", { name: "Review time-bound grant" }));
  return screen.getByRole("dialog", { name: "Review time-bound grant" });
}

function confirmGrant() {
  const dialog = reviewGrant();
  fireEvent.click(within(dialog).getByRole("button", { name: "Grant time-bound role" }));
  return dialog;
}

describe("RolesView identity markers", () => {
  it("renders the current factual context, decision, and collapsed role evidence", async () => {
    view([]);

    await screen.findByText("mail.domain.manage");
    for (const name of [
      "Built-in role catalog",
      "Current assignment decision",
      "Role assignments",
    ]) {
      const heading = screen.getByRole("heading", { level: 2, name });
      const panel = heading.closest("section");
      expect(heading).toBeVisible();
      expect(panel).toHaveAttribute("aria-labelledby", heading.id);
      expect(panel).not.toHaveAttribute("title");
      expect(screen.getByRole("region", { name })).toBe(panel);
    }
    const context = screen.getByRole("region", { name: "Current role-assignment context" });
    expect(context).toHaveTextContent(role.organization_id);
    expect(context).toHaveTextContent("All visible principals");
    expect(context).toHaveTextContent("Active records only");
    expect(context).toHaveTextContent("Assignments returned on this page");
    expect(screen.getByRole("group", { name: "Role-catalog evidence" }))
      .not.toHaveAttribute("open");
    expect(screen.queryByRole("navigation", { name: "Role assignment pagination" }))
      .not.toBeInTheDocument();
  });

  it("shows assignment pagination only when the response exposes a real page boundary", async () => {
    const listRoleAssignments = vi.fn()
      .mockResolvedValueOnce({
        items: [assignment("human", true)],
        has_more: true,
        next_cursor: "roles-page-2",
      })
      .mockResolvedValueOnce({
        items: [assignment("agent", true)],
        has_more: false,
        next_cursor: null,
      });
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    const pagination = await screen.findByRole("navigation", {
      name: "Role assignment pagination",
    });
    expect(within(pagination).getByRole("button", { name: "Previous" })).toBeDisabled();
    expect(within(pagination).getByRole("button", { name: "Next" })).toBeEnabled();

    fireEvent.click(within(pagination).getByRole("button", { name: "Next" }));
    expect(await screen.findByText("agent principal")).toBeVisible();
    expect(within(pagination).getByRole("button", { name: "Previous" })).toBeEnabled();
    expect(within(pagination).getByRole("button", { name: "Next" })).toBeDisabled();
  });

  it("renders localized Human, Agent, and Service identity markers without relabeling Service as System", async () => {
    view([
      assignment("human", true),
      assignment("agent", false),
      assignment("service", true),
    ]);

    await screen.findByText("human principal");

    expect(screen.getByText("Human").closest("[data-identity-kind]"))
      .toHaveAttribute("data-identity-kind", "human");
    expect(screen.getByText("Agent").closest("[data-identity-kind]"))
      .toHaveAttribute("data-identity-kind", "agent");
    const service = screen.getByText("Service").closest("[data-identity-kind]");
    expect(service).toHaveAttribute("data-identity-kind", "service");
    expect(service?.querySelector('[data-icon="system"]')).toBeInTheDocument();
    expect(screen.queryByText("System", { exact: true })).not.toBeInTheDocument();
    expect(translator("zh-CN")("identityService")).toBe("服务");
  });

  it("uses factual assignment activity for a semantic status indicator", async () => {
    view([assignment("human", true), assignment("agent", false)]);

    await waitFor(() => expect(screen.getByText("Active")).toBeInTheDocument());

    expect(screen.getByText("Active").closest("[data-status-tone]"))
      .toHaveAttribute("data-status-tone", "positive");
    expect(screen.getByText("Inactive").closest("[data-status-tone]"))
      .toHaveAttribute("data-status-tone", "neutral");
  });

  it("states an unbounded assignment's actual no-expiry fact instead of a dash", async () => {
    view([{ ...assignment("human", true), valid_until: null }]);

    await screen.findByText("human principal");
    expect(screen.getByText(/No expiry/)).toBeVisible();
    expect(screen.queryByText("—", { exact: true })).not.toBeInTheDocument();
    expect(translator("zh-CN")("noExpiry")).toBe("无到期时间");
  });

  it("cancels a revoke in the shared dialog and restores focus without calling the API", async () => {
    const revokeRoleAssignment = vi.fn();
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments: vi.fn().mockResolvedValue({
        items: [assignment("human", true)],
        has_more: false,
        next_cursor: null,
      }),
      revokeRoleAssignment,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    const trigger = await screen.findByRole("button", { name: "Revoke" });
    trigger.focus();
    fireEvent.click(trigger);

    const dialog = screen.getByRole("dialog", { name: "Revoke" });
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    await waitFor(() => expect(cancel).toHaveFocus());
    fireEvent.click(cancel);

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(trigger).toHaveFocus();
    expect(revokeRoleAssignment).not.toHaveBeenCalled();
  });

  it("revokes the captured assignment exactly once and locks every dismissal path while busy", async () => {
    let resolveRevoke: ((value: RoleAssignment) => void) | undefined;
    const revokeRoleAssignment = vi.fn().mockImplementation(() => new Promise<RoleAssignment>((resolve) => {
      resolveRevoke = resolve;
    }));
    const target = assignment("agent", true);
    const revoked = { ...target, active: false, revoked_at: "2026-08-24T02:00:00Z" };
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments: vi.fn().mockResolvedValue({
        items: [target],
        has_more: false,
        next_cursor: null,
      }),
      revokeRoleAssignment,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }));
    const dialog = screen.getByRole("dialog", { name: "Revoke" });
    const confirm = within(dialog).getByRole("button", { name: "Revoke" });

    fireEvent.click(confirm);
    fireEvent.click(confirm);
    fireEvent.keyDown(document, { key: "Escape" });

    expect(revokeRoleAssignment).toHaveBeenCalledTimes(1);
    expect(revokeRoleAssignment).toHaveBeenCalledWith(target.role_assignment_id);
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();

    resolveRevoke?.(revoked);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(revokeRoleAssignment).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Revoke" })).not.toBeInTheDocument();
    await waitFor(() => expect(
      screen.getByRole("textbox", { name: "Filter by principal ID" }),
    ).toHaveFocus());
  });

  it("keeps the newest filter result when an older request resolves last", async () => {
    let resolveFirst: ((value: { items: RoleAssignment[]; has_more: false; next_cursor: null }) => void) | undefined;
    let resolveSecond: ((value: { items: RoleAssignment[]; has_more: false; next_cursor: null }) => void) | undefined;
    const firstResult = new Promise<{ items: RoleAssignment[]; has_more: false; next_cursor: null }>((resolve) => {
      resolveFirst = resolve;
    });
    const secondResult = new Promise<{ items: RoleAssignment[]; has_more: false; next_cursor: null }>((resolve) => {
      resolveSecond = resolve;
    });
    const listRoleAssignments = vi.fn((input: { principal_id?: string }) => {
      if (input.principal_id === "principal-a") return firstResult;
      if (input.principal_id === "principal-b") return secondResult;
      return Promise.resolve({ items: [], has_more: false as const, next_cursor: null });
    });
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    await screen.findByText("No role assignments match the current filters.");
    const filter = screen.getByRole("textbox", { name: "Filter by principal ID" });
    fireEvent.change(filter, { target: { value: "principal-a" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply filters" }));
    await waitFor(() => expect(listRoleAssignments).toHaveBeenCalledWith(
      expect.objectContaining({ principal_id: "principal-a" }),
    ));
    fireEvent.change(filter, { target: { value: "principal-b" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply filters" }));
    await waitFor(() => expect(listRoleAssignments).toHaveBeenCalledWith(
      expect.objectContaining({ principal_id: "principal-b" }),
    ));

    resolveSecond?.({ items: [assignment("agent", true)], has_more: false, next_cursor: null });
    expect(await screen.findByText("agent principal")).toBeVisible();
    resolveFirst?.({ items: [assignment("human", true)], has_more: false, next_cursor: null });

    await waitFor(() => expect(screen.queryByText("human principal")).not.toBeInTheDocument());
    expect(screen.getByText("agent principal")).toBeVisible();
    expect(filter).toHaveValue("principal-b");
  });

  it("keeps a committed revocation authoritative when the following list refresh fails", async () => {
    const target = assignment("service", true);
    const revoked = { ...target, active: false, revoked_at: "2026-08-24T02:00:00Z" };
    const listRoleAssignments = vi.fn()
      .mockResolvedValueOnce({ items: [target], has_more: false, next_cursor: null })
      .mockRejectedValueOnce(new Error("read replica unavailable"));
    const revokeRoleAssignment = vi.fn().mockResolvedValue(revoked);
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments,
      revokeRoleAssignment,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }));
    const dialog = screen.getByRole("dialog", { name: "Revoke" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(revokeRoleAssignment).toHaveBeenCalledWith(target.role_assignment_id);
    expect(screen.queryByRole("button", { name: "Revoke" })).not.toBeInTheDocument();
    expect(screen.queryByText("service principal")).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("unexpected_error");
    expect(screen.getByRole("textbox", { name: "Filter by principal ID" })).toHaveFocus();
  });

  it("keeps exact grant evidence visible when the following list refresh fails", async () => {
    const granted = {
      ...assignment("agent", true),
      approval_request_id: "approval-grant-agent",
    };
    const listRoleAssignments = vi.fn()
      .mockResolvedValueOnce({ items: [], has_more: false, next_cursor: null })
      .mockRejectedValueOnce(new Error("read replica unavailable"));
    const grantRoleAssignment = vi.fn().mockResolvedValue(granted);
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments,
      grantRoleAssignment,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    await screen.findByText("No role assignments match the current filters.");
    fireEvent.change(screen.getByRole("textbox", { name: "Target principal ID" }), {
      target: { value: granted.principal_id },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-grant-agent" },
    });
    const review = reviewGrant();
    expect(review).toHaveTextContent(granted.principal_id);
    expect(review).toHaveTextContent(granted.role_display_name);
    expect(review).toHaveTextContent("mail.domain.manage");
    expect(review).toHaveTextContent("approval-grant-agent");
    expect(grantRoleAssignment).not.toHaveBeenCalled();
    fireEvent.click(within(review).getByRole("button", { name: "Grant time-bound role" }));

    const evidence = await screen.findByRole("status", {
      name: "Role assignment committed",
    });
    expect(evidence).toHaveTextContent(granted.role_assignment_id);
    expect(evidence).toHaveTextContent(granted.principal_id);
    expect(evidence).toHaveTextContent(granted.role_display_name);
    expect(await screen.findByRole("alert")).toHaveTextContent("unexpected_error");
    expect(grantRoleAssignment).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("textbox", { name: "Target principal ID" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Approval request ID" })).toHaveValue("");
    expect(screen.getByRole("button", { name: "Review time-bound grant" })).toBeDisabled();
  });

  it("locks query changes during a grant and keeps evidence across an empty stale refresh", async () => {
    const granted = {
      ...assignment("human", true),
      approval_request_id: "approval-grant-human",
    };
    let resolveGrant: ((value: RoleAssignment) => void) | undefined;
    const pendingGrant = new Promise<RoleAssignment>((resolve) => {
      resolveGrant = resolve;
    });
    const listRoleAssignments = vi.fn().mockResolvedValue({
      items: [],
      has_more: false,
      next_cursor: null,
    });
    const grantRoleAssignment = vi.fn().mockReturnValue(pendingGrant);
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments,
      grantRoleAssignment,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    await screen.findByText("No role assignments match the current filters.");
    const filter = screen.getByRole("textbox", { name: "Filter by principal ID" });
    fireEvent.change(filter, { target: { value: "principal-a" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply filters" }));
    await waitFor(() => expect(listRoleAssignments).toHaveBeenLastCalledWith(
      expect.objectContaining({ principal_id: "principal-a" }),
    ));

    fireEvent.change(screen.getByRole("textbox", { name: "Target principal ID" }), {
      target: { value: granted.principal_id },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-grant-human" },
    });
    const dialog = reviewGrant();
    const grantButton = within(dialog).getByRole("button", { name: "Grant time-bound role" });
    fireEvent.click(grantButton);
    fireEvent.click(grantButton);

    expect(grantRoleAssignment).toHaveBeenCalledTimes(1);
    expect(filter).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: "Include expired or revoked records" }))
      .toBeDisabled();
    expect(screen.getByRole("button", { name: "Apply filters" })).toBeDisabled();

    resolveGrant?.(granted);
    const evidence = await screen.findByRole("status", {
      name: "Role assignment committed",
    });
    await waitFor(() => expect(listRoleAssignments).toHaveBeenLastCalledWith(
      expect.objectContaining({ principal_id: "principal-a", cursor: undefined }),
    ));
    expect(evidence).toHaveTextContent(granted.role_assignment_id);
    expect(screen.getByText("No role assignments match the current filters.")).toBeVisible();
    expect(screen.getByRole("status", { name: "Role assignment committed" }))
      .toBeInTheDocument();
  });

  it("freezes role mutations after an unknown grant outcome without offering a fabricated lookup", async () => {
    const grantRoleAssignment = vi.fn().mockRejectedValue(new Error("network interrupted"));
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments: vi.fn().mockResolvedValue({ items: [], has_more: false, next_cursor: null }),
      grantRoleAssignment,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    await screen.findByText("No role assignments match the current filters.");
    fireEvent.change(screen.getByRole("textbox", { name: "Target principal ID" }), {
      target: { value: "019b0000-0000-7000-8000-0000000005aa" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-unknown-grant" },
    });
    const dialog = confirmGrant();
    expect(dialog).toHaveTextContent("mail.domain.manage");

    const lock = await screen.findByRole("status", {
      name: "Role-assignment write outcome is unknown",
    });
    expect(lock).toHaveTextContent("019b0000-0000-7000-8000-0000000005aa");
    expect(lock).toHaveTextContent("approval-unknown-grant");
    expect(grantRoleAssignment).toHaveBeenCalledOnce();
    expect(screen.getByRole("textbox", { name: "Target principal ID" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Review time-bound grant" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Apply filters" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: /reconcile|load/i })).not.toBeInTheDocument();
  });

  it("freezes role mutations after an unknown revocation outcome", async () => {
    const target = assignment("service", true);
    const revokeRoleAssignment = vi.fn().mockRejectedValue(new Error("network interrupted"));
    const client = {
      listRoles: vi.fn().mockResolvedValue({ items: [role], has_more: false, next_cursor: null }),
      listRoleAssignments: vi.fn().mockResolvedValue({
        items: [target],
        has_more: false,
        next_cursor: null,
      }),
      revokeRoleAssignment,
    } as unknown as ControlPlaneClient;

    render(<RolesView client={client} locale="en" t={translator("en")} />);
    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }));
    const dialog = screen.getByRole("dialog", { name: "Revoke" });
    expect(dialog).toHaveTextContent(target.role_assignment_id);
    expect(dialog).toHaveTextContent(target.principal_id);
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));

    const lock = await screen.findByRole("status", {
      name: "Role-assignment write outcome is unknown",
    });
    expect(lock).toHaveTextContent(target.role_assignment_id);
    expect(lock).toHaveTextContent(target.principal_id);
    expect(revokeRoleAssignment).toHaveBeenCalledWith(target.role_assignment_id);
    expect(screen.getByRole("button", { name: "Apply filters" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Revoke" })).not.toBeInTheDocument();
  });
});
