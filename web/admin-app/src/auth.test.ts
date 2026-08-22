import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { loadSession, loginUrl, logout, startLogin } from "./auth";
import type { RuntimeConfig } from "./types";

const config: RuntimeConfig = {
  apiBaseUrl: "",
  organizationId: "018f13b9-8bf2-7bc8-965d-48a28866f073",
};

describe("BFF browser session", () => {
  beforeEach(() => {
    sessionStorage.clear();
    localStorage.clear();
    window.history.replaceState({}, "", "/admin");
  });

  afterEach(() => vi.unstubAllGlobals());

  it("navigates to the server login endpoint without discovery or browser state", () => {
    const fetchMock = vi.fn();
    const navigate = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    startLogin(config, navigate, "/admin?view=audit#latest");

    expect(navigate).toHaveBeenCalledWith(
      "/api/v2/auth/admin/login?return_to=%2Fadmin%3Fview%3Daudit%23latest",
    );
    expect(loginUrl(config, "//identity.example/steal")).toBe(
      "/api/v2/auth/admin/login?return_to=%2F",
    );
    expect(fetchMock).not.toHaveBeenCalled();
    expect(sessionStorage.length).toBe(0);
    expect(localStorage.length).toBe(0);
  });

  it("loads only the public session projection and never retains provider tokens", async () => {
    const sessionPayload = {
      principal_id: "human-1",
      organization_id: config.organizationId,
      client: "admin",
      expires_at: "2026-07-27T08:00:00Z",
    };
    Object.defineProperty(sessionPayload, "access_token", {
      enumerable: true,
      get: () => {
        throw new Error("browser attempted to read a provider token");
      },
    });
    const fetchMock = vi.fn().mockResolvedValue({
      status: 200,
      ok: true,
      json: async () => sessionPayload,
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(loadSession(config)).resolves.toEqual({
      principal_id: "human-1",
      organization_id: config.organizationId,
      client: "admin",
      expires_at: "2026-07-27T08:00:00Z",
    });

    const [url, request] = fetchMock.mock.calls[0];
    expect(url).toBe("/api/v2/auth/admin/session");
    expect(request).toMatchObject({
      method: "GET",
      credentials: "same-origin",
      cache: "no-store",
    });
    expect(request.headers).not.toHaveProperty("Authorization");
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
  });

  it("treats an absent cookie as signed out and clears it through POST logout", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(loadSession(config)).resolves.toBeNull();
    await expect(logout(config)).resolves.toBeUndefined();

    expect(fetchMock.mock.calls[1]).toEqual([
      "/api/v2/auth/admin/logout",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
      }),
    ]);
    expect((fetchMock.mock.calls[1][1] as RequestInit).headers)
      .not.toHaveProperty("Authorization");
  });
});
