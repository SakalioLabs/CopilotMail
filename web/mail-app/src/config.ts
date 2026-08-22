import type { RuntimeConfig } from "./types";

const canonicalUuidPattern =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

declare global {
  interface Window {
    __COPILOTMAIL_MAIL_CONFIG__?: Partial<RuntimeConfig>;
  }
}

export function readRuntimeConfig(): RuntimeConfig {
  const value = window.__COPILOTMAIL_MAIL_CONFIG__;
  const mailboxPreference = value?.mailboxId?.trim();
  return {
    apiBaseUrl: value?.apiBaseUrl?.replace(/\/+$/, "") ?? "",
    mailboxId: mailboxPreference && canonicalUuidPattern.test(mailboxPreference)
      ? mailboxPreference
      : undefined,
  };
}

export function sameOriginApiBaseUrl(value: string): string {
  const normalized = value.replace(/\/+$/, "");
  if (!normalized) return "";
  const url = new URL(normalized, window.location.origin);
  if (
    url.origin !== window.location.origin
    || url.username
    || url.password
    || url.search
    || url.hash
  ) {
    throw new Error("api_base_url_must_be_same_origin");
  }
  const path = url.pathname.replace(/\/+$/, "");
  return normalized.startsWith("/") ? path : `${url.origin}${path}`;
}
