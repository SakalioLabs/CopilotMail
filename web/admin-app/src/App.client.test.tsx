// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const organizationId = "018f13b9-8bf2-7bc8-965d-48a28866f073";

function successfulJson(body: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: async () => body,
  } as Response;
}

describe("App control-plane client lifetime", () => {
  beforeEach(() => {
    vi.resetModules();
    localStorage.clear();
    localStorage.setItem("copilotmail.admin.locale.v1", "en");
    window.__COPILOTMAIL_CONFIG__ = {
      apiBaseUrl: "",
      organizationId,
    };
  });

  afterEach(() => {
    cleanup();
    localStorage.clear();
    delete window.__COPILOTMAIL_CONFIG__;
    vi.unstubAllGlobals();
  });

  it("keeps one client across locale and parent rerenders while navigation loads views on demand", async () => {
    const requestedUrls: string[] = [];
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === "string"
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url;
      requestedUrls.push(url);

      if (url === "/api/v2/auth/admin/session") {
        return successfulJson({
          principal_id: "human-admin-1",
          organization_id: organizationId,
          client: "admin",
          expires_at: "2099-08-25T08:00:00Z",
        });
      }
      if (url.includes("/compliance-cases")) {
        return successfulJson({ items: [], next_cursor: null, has_more: false });
      }
      if (url.includes("/legal-holds")) {
        return successfulJson({ items: [], next_cursor: null, has_more: false });
      }
      if (url.endsWith("/roles")) {
        return successfulJson({ items: [] });
      }
      if (url.includes("/role-assignments")) {
        return successfulJson({ items: [], next_cursor: null, has_more: false });
      }
      if (url.includes("/approval-requests")) {
        return successfulJson({ items: [], next_cursor: null, has_more: false });
      }
      throw new Error(`unexpected_test_request:${url}`);
    });
    vi.stubGlobal("fetch", fetchMock);

    const { default: App } = await import("./App");
    const { rerender } = render(<App />);
    const countRequests = (fragment: string) => (
      requestedUrls.filter((url) => url.includes(fragment)).length
    );

    expect(await screen.findByRole("heading", {
      name: "Control-plane decisions",
    })).toBeVisible();
    await waitFor(() => {
      expect(countRequests("/compliance-cases")).toBe(1);
      expect(countRequests("/legal-holds")).toBe(1);
    });
    expect(countRequests("/roles")).toBe(0);
    expect(countRequests("/role-assignments")).toBe(0);
    expect(countRequests("/approval-requests")).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "Role access" }));
    expect(await screen.findByRole("heading", {
      name: "Roles and assignments",
    })).toBeVisible();
    await waitFor(() => {
      expect(countRequests("/roles")).toBe(1);
      expect(countRequests("/role-assignments")).toBe(1);
    });

    fireEvent.change(screen.getByRole("combobox", { name: "Language" }), {
      target: { value: "zh-CN" },
    });
    expect(await screen.findByRole("heading", { name: "角色与授权分配" }))
      .toBeVisible();
    await act(async () => Promise.resolve());
    expect(countRequests("/roles")).toBe(1);
    expect(countRequests("/role-assignments")).toBe(1);

    rerender(<App />);
    await act(async () => Promise.resolve());
    expect(countRequests("/roles")).toBe(1);
    expect(countRequests("/role-assignments")).toBe(1);
    expect(countRequests("/approval-requests")).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "审批中心" }));
    expect(await screen.findByRole("heading", { name: "企业审批中心" }))
      .toBeVisible();
    await waitFor(() => expect(countRequests("/approval-requests")).toBe(1));

    fireEvent.change(screen.getByRole("combobox", { name: "语言" }), {
      target: { value: "en" },
    });
    expect(await screen.findByRole("heading", { name: "Enterprise approvals" }))
      .toBeVisible();
    await act(async () => Promise.resolve());
    expect(countRequests("/approval-requests")).toBe(1);
    expect(countRequests("/roles")).toBe(1);
    expect(countRequests("/role-assignments")).toBe(1);
  });

  it("preserves terminal evidence across navigation and clears it on focus-detected identity rotation", async () => {
    const staleOpenCase = {
      case_id: "case-session-ledger-1",
      organization_id: organizationId,
      external_reference: "SEC-SESSION-1",
      title: "Session-scoped evidence",
      purpose: "Do not reopen after navigation",
      status: "open",
      created_by: "human-admin-1",
      created_at: "2026-08-24T01:00:00Z",
      closed_by: null,
      closed_at: null,
    } as const;
    const closedCase = {
      ...staleOpenCase,
      status: "closed",
      closed_by: "human-admin-1",
      closed_at: "2026-08-24T02:00:00Z",
    } as const;
    let sessionReads = 0;
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const rawUrl = typeof input === "string"
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url;
      const url = new URL(rawUrl, "https://admin.example");
      const method = init?.method ?? "GET";
      if (url.pathname === "/api/v2/auth/admin/session") {
        sessionReads += 1;
        return successfulJson({
          principal_id: sessionReads === 1 ? "human-admin-1" : "human-admin-2",
          organization_id: organizationId,
          client: "admin",
          expires_at: sessionReads === 1
            ? "2099-08-25T08:00:00Z"
            : "2099-08-26T08:00:00Z",
        });
      }
      if (url.pathname.endsWith("/compliance-cases") && url.searchParams.get("limit") === "100") {
        return successfulJson({ items: [], next_cursor: null, has_more: false });
      }
      if (url.pathname.endsWith("/legal-holds")) {
        return successfulJson({ items: [], next_cursor: null, has_more: false });
      }
      if (url.pathname.endsWith("/compliance-cases") && url.searchParams.get("limit") === "25") {
        return successfulJson({ items: [staleOpenCase], next_cursor: null, has_more: false });
      }
      if (
        method === "POST"
        && url.pathname === `/api/v2/compliance-cases/${staleOpenCase.case_id}/closure`
      ) {
        return successfulJson(closedCase);
      }
      throw new Error(`unexpected_test_request:${method}:${url.pathname}${url.search}`);
    });
    vi.stubGlobal("fetch", fetchMock);

    const { default: App } = await import("./App");
    render(<App />);
    expect(await screen.findByRole("heading", {
      name: "Control-plane decisions",
    })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Compliance cases" }));
    fireEvent.click(await screen.findByRole("button", { name: staleOpenCase.title }));
    fireEvent.click(screen.getByRole("button", { name: "Close case" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm close" }));
    expect(await screen.findByRole("status", { name: "Case closed" }))
      .toHaveTextContent(staleOpenCase.case_id);

    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    fireEvent.click(screen.getByRole("button", { name: "Compliance cases" }));
    expect(await screen.findByRole("status", { name: "Case closed" }))
      .toHaveTextContent(staleOpenCase.case_id);
    expect(screen.queryByRole("button", { name: staleOpenCase.title }))
      .not.toBeInTheDocument();

    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(sessionReads).toBe(2));
    expect(await screen.findByRole("heading", {
      name: "Control-plane decisions",
    })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Compliance cases" }));
    expect(await screen.findByRole("button", { name: staleOpenCase.title })).toBeVisible();
    expect(screen.queryByRole("status", { name: "Case closed" })).not.toBeInTheDocument();
  });

  it("revalidates and signs out when a protected request returns 401", async () => {
    let sessionReads = 0;
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const rawUrl = typeof input === "string"
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url;
      const url = new URL(rawUrl, "https://admin.example");
      if (url.pathname === "/api/v2/auth/admin/session") {
        sessionReads += 1;
        if (sessionReads > 1) return new Response(null, { status: 401 });
        return successfulJson({
          principal_id: "human-admin-1",
          organization_id: organizationId,
          client: "admin",
          expires_at: "2099-08-25T08:00:00Z",
        });
      }
      if (url.pathname.endsWith("/compliance-cases")) {
        return new Response(JSON.stringify({
          code: "unauthenticated",
          message: "expired",
          request_id: "request-401",
          trace_id: "trace-401",
          retryable: false,
        }), { status: 401, headers: { "Content-Type": "application/json" } });
      }
      if (url.pathname.endsWith("/legal-holds")) {
        return successfulJson({ items: [], next_cursor: null, has_more: false });
      }
      throw new Error(`unexpected_test_request:${url.pathname}${url.search}`);
    });
    vi.stubGlobal("fetch", fetchMock);

    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByRole("button", { name: "Sign in with enterprise SSO" }))
      .toBeVisible();
    expect(sessionReads).toBe(2);
  });
});
