import { sameOriginApiBaseUrl } from "./config";
import type { HumanSessionResponse, RuntimeConfig } from "./types";

type Navigate = (url: string) => void;

const rfc3339Pattern =
  /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(?:Z|[+-](\d{2}):(\d{2}))$/;

function validBoundedIdentifier(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value.length <= 200
    && value.trim() === value
    && !/[\u0000-\u001f\u007f]/.test(value);
}

function validRfc3339Timestamp(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0 || value.length > 64) return false;
  const match = rfc3339Pattern.exec(value);
  if (!match || !Number.isFinite(Date.parse(value))) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  const offsetHours = match[7] === undefined ? 0 : Number(match[7]);
  const offsetMinutes = match[8] === undefined ? 0 : Number(match[8]);
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const daysInMonth = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return year > 0
    && month >= 1
    && month <= 12
    && day >= 1
    && day <= daysInMonth[month - 1]!
    && hour <= 23
    && minute <= 59
    && second <= 59
    && offsetHours <= 23
    && offsetMinutes <= 59;
}

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
    !validBoundedIdentifier(session.principal_id)
    || !validBoundedIdentifier(session.organization_id)
    || (session.client !== "admin" && session.client !== "mail")
    || !validRfc3339Timestamp(session.expires_at)
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
  return endpoint(config, `/api/v2/auth/mail/login?${query}`);
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
  const response = await fetch(endpoint(config, "/api/v2/auth/mail/session"), {
    method: "GET",
    credentials: "same-origin",
    cache: "no-store",
    headers: { Accept: "application/json" },
  });
  if (response.status === 401) return null;
  if (!response.ok) throw new Error(`session_${response.status}`);
  const session = parseSession(await response.json());
  if (session.client !== "mail") throw new Error("session_context_mismatch");
  if (Date.parse(session.expires_at) <= now) return null;
  return session;
}

export async function logout(config: RuntimeConfig): Promise<void> {
  const response = await fetch(endpoint(config, "/api/v2/auth/mail/logout"), {
    method: "POST",
    credentials: "same-origin",
    cache: "no-store",
    headers: { Accept: "application/json" },
  });
  if (!response.ok && response.status !== 401) {
    throw new Error(`logout_${response.status}`);
  }
}
