import type { SVGProps } from "react";

const INFO = "var(--cm-info)";
const SURFACE = "var(--cm-surface)";

/**
 * The character logomark: a brand-colour tile carrying the CopilotMail "C".
 * Product-owned geometry; it is decoration next to the visible wordmark.
 * Kept in its own module so importing it never pins the empty-state
 * illustration family into an eager bundle.
 */
export function BrandMark({
  size = 22,
  title,
  role,
  style,
  "aria-hidden": ariaHidden,
  "aria-label": ariaLabel,
  ...svgProps
}: Omit<SVGProps<SVGSVGElement>, "children" | "viewBox"> & {
  size?: number | string;
  title?: string;
}) {
  const isDecorative = ariaHidden === true || (!title && !ariaLabel);
  return (
    <svg
      {...svgProps}
      aria-hidden={isDecorative ? true : ariaHidden}
      aria-label={ariaLabel}
      data-brand-mark=""
      focusable="false"
      height={size}
      role={role ?? (isDecorative ? undefined : "img")}
      style={{ alignSelf: "center", flex: "0 0 auto", ...style }}
      viewBox="0 0 24 24"
      width={size}
    >
      {title ? <title>{title}</title> : null}
      <rect width="24" height="24" rx="6" fill={INFO} />
      <path
        d="M16.6 8.7a5 5 0 1 0 0 6.6"
        fill="none"
        stroke={SURFACE}
        strokeLinecap="round"
        strokeWidth="2.3"
      />
    </svg>
  );
}
