import { afterEach, describe, expect, it, vi } from "vitest";
import { loadSession, loginUrl, logout, startLogin } from "./auth";
import { sameOriginApiBaseUrl } from "./config";
import type { RuntimeConfig } from "./types";

const config: RuntimeConfig = {
  apiBaseUrl: "",
  mailboxId: "018f0000-0000-7000-8000-000000000001",
};

afterEach(() => vi.unstubAllGlobals());

describe("mail BFF browser session", () => {
  it("navigates to server login without discovery, PKCE, or browser storage", () => {
    const fetchMock = vi.fn();
    const navigate = vi.fn();
    const sessionStorage = { setItem: vi.fn(), getItem: vi.fn(), removeItem: vi.fn() };
    const localStorage = { setItem: vi.fn(), getItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("sessionStorage", sessionStorage);
    vi.stubGlobal("localStorage", localStorage);

    startLogin(config, navigate, "/mail?folder=inbox#latest");

    expect(navigate).toHaveBeenCalledWith(
      "/api/v2/auth/mail/login?return_to=%2Fmail%3Ffolder%3Dinbox%23latest",
    );
    expect(loginUrl(config, "https://identity.example/callback")).toBe(
      "/api/v2/auth/mail/login?return_to=%2F",
    );
    expect(fetchMock).not.toHaveBeenCalled();
    expect(sessionStorage.setItem).not.toHaveBeenCalled();
    expect(localStorage.setItem).not.toHaveBeenCalled();
  });

  it("projects a session without parsing, storing, or sending an access token", async () => {
    const sessionPayload = {
      principal_id: "human-1",
      organization_id: "org-1",
      client: "mail",
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
    const sessionStorage = { setItem: vi.fn() };
    const localStorage = { setItem: vi.fn() };
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("sessionStorage", sessionStorage);
    vi.stubGlobal("localStorage", localStorage);

    await expect(loadSession(config)).resolves.toEqual({
      principal_id: "human-1",
      organization_id: "org-1",
      client: "mail",
      expires_at: "2026-07-27T08:00:00Z",
    });

    const [url, request] = fetchMock.mock.calls[0];
    expect(url).toBe("/api/v2/auth/mail/session");
    expect(request).toMatchObject({
      method: "GET",
      credentials: "same-origin",
      cache: "no-store",
    });
    expect(request.headers).not.toHaveProperty("Authorization");
    expect(sessionStorage.setItem).not.toHaveBeenCalled();
    expect(localStorage.setItem).not.toHaveBeenCalled();
  });

  it("uses POST logout and rejects cross-origin API configuration", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("window", {
      location: {
        origin: "https://mail.example.test",
        pathname: "/",
        search: "",
        hash: "",
        assign: vi.fn(),
      },
    });

    await expect(logout(config)).resolves.toBeUndefined();
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v2/auth/mail/logout",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
        cache: "no-store",
      }),
    );
    expect(() => sameOriginApiBaseUrl("https://control.example.test"))
      .toThrow("api_base_url_must_be_same_origin");
  });
});
