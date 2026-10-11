import { sameOriginApiBaseUrl } from "./config";
import type { HumanSessionResponse, RuntimeConfig } from "./types";

type Navigate = (url: string) => void;

function endpoint(config: RuntimeConfig, path: string): string {
  return `${sameOriginApiBaseUrl(config.apiBaseUrl)}${path}`;
}

function safeReturnTo(value: string): string {
  if (!value.startsWith("/") || value.startsWith("//") || /[\u0000-\u001f]/.test(value)) {
    return "/";
  }
  return value;
}

function currentReturnTo(): string {
  return safeReturnTo(
    `${window.location.pathname}${window.location.search}${window.location.hash}`,
  );
}

function parseSession(value: unknown): HumanSessionResponse {
  if (!value || typeof value !== "object") throw new Error("session_response_invalid");
  const record = value as Record<string, unknown>;
  const session = {
    principal_id: record.principal_id,
    organization_id: record.organization_id,
    client: record.client,
    expires_at: record.expires_at,
  };
  if (
    typeof session.principal_id !== "string"
    || session.principal_id.length === 0
    || session.principal_id.length > 200
    || session.principal_id.trim() !== session.principal_id
    || /[\u0000-\u001f\u007f]/.test(session.principal_id)
    || typeof session.organization_id !== "string"
    || session.organization_id.length === 0
    || session.organization_id.length > 200
    || session.organization_id.trim() !== session.organization_id
    || /[\u0000-\u001f\u007f]/.test(session.organization_id)
    || (session.client !== "admin" && session.client !== "mail")
    || typeof session.expires_at !== "string"
    || session.expires_at.length === 0
    || session.expires_at.length > 64
    || !Number.isFinite(Date.parse(session.expires_at))
  ) {
    throw new Error("session_response_invalid");
  }
  return session as HumanSessionResponse;
}

export function loginUrl(
  config: RuntimeConfig,
  returnTo = currentReturnTo(),
): string {
  const query = new URLSearchParams({ return_to: safeReturnTo(returnTo) });
  return endpoint(config, `/api/v2/auth/admin/login?${query}`);
}

export function startLogin(
  config: RuntimeConfig,
  navigate: Navigate = (url) => window.location.assign(url),
  returnTo = currentReturnTo(),
): void {
  navigate(loginUrl(config, returnTo));
}

export async function loadSession(
  config: RuntimeConfig,
  now = Date.now(),
): Promise<HumanSessionResponse | null> {
  const response = await fetch(endpoint(config, "/api/v2/auth/admin/session"), {
    method: "GET",
    credentials: "same-origin",
    cache: "no-store",
    headers: { Accept: "application/json" },
  });
  if (response.status === 401) return null;
  if (!response.ok) throw new Error(`session_${response.status}`);
  const session = parseSession(await response.json());
  if (
    session.client !== "admin"
    || session.organization_id !== config.organizationId
  ) {
    throw new Error("session_context_mismatch");
  }
  if (Date.parse(session.expires_at) <= now) return null;
  return session;
}

export async function logout(config: RuntimeConfig): Promise<void> {
  const response = await fetch(endpoint(config, "/api/v2/auth/admin/logout"), {
    method: "POST",
    credentials: "same-origin",
    headers: { Accept: "application/json" },
  });
  if (!response.ok && response.status !== 401) {
    throw new Error(`logout_${response.status}`);
  }
}
