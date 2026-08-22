import type { RuntimeConfig } from "./types";

declare global {
  interface Window {
    __COPILOTMAIL_CONFIG__?: Partial<RuntimeConfig>;
  }
}

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function readRuntimeConfig(): RuntimeConfig {
  const source = window.__COPILOTMAIL_CONFIG__;
  return {
    apiBaseUrl: source?.apiBaseUrl?.replace(/\/+$/, "") ?? "",
    organizationId: source?.organizationId ?? "",
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

export function configurationProblems(config: RuntimeConfig): string[] {
  const problems: string[] = [];
  if (!UUID_PATTERN.test(config.organizationId)) problems.push("organizationId");
  try {
    sameOriginApiBaseUrl(config.apiBaseUrl);
  } catch {
    problems.push("apiBaseUrl.sameOrigin");
  }
  return [...new Set(problems)];
}
