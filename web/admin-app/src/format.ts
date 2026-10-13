import type { Locale } from "./types";

/** Shared date rendering for admin tables; all views show medium+short. */
export function formatDate(value: string, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

/** Nullable variant: empty evidence renders as a placeholder, never a crash. */
export function formatNullableDate(
  value: string | null,
  locale: Locale,
  empty = "—",
): string {
  if (!value) return empty;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? empty : formatDate(value, locale);
}

/**
 * Evidence screens keep the raw token when a timestamp cannot be parsed —
 * the unformatted value is itself evidence and must not be replaced.
 */
export function formatEvidenceDate(value: string, locale: Locale): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : formatDate(value, locale);
}

/** Compacts long identifiers for dense table cells. */
export function shortId(value: string): string {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
}

/** A UUID that names a real entity rather than the all-zero sentinel. */
export function isCanonicalUuid(value: string): boolean {
  return value !== "00000000-0000-0000-0000-000000000000"
    && /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/.test(value);
}

/** Focuses an element, drawing the focus ring even after pointer input. */
export function focusWithVisibleRing(element: HTMLElement | null | undefined) {
  if (!element) return;
  const options: FocusOptions & { focusVisible: boolean } = { focusVisible: true };
  element.focus(options);
}
