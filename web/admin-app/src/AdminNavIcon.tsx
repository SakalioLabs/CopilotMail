import { Icon, type IconName } from "@copilotmail/design-system";
import type { SVGProps } from "react";

/**
 * Stable control-plane navigation concepts exposed by the Admin application.
 * Their SVG geometry is owned by the shared product icon registry.
 */
export type AdminNavIconName =
  | "overview"
  | "cases"
  | "holds"
  | "approvals"
  | "roles"
  | "agent-inspector"
  | "audit"
  | "retention"
  | "mail-domains";

const ADMIN_NAV_ICON: Readonly<Record<AdminNavIconName, IconName>> = {
  overview: "admin-overview",
  cases: "admin-cases",
  holds: "admin-holds",
  approvals: "admin-approvals",
  roles: "admin-roles",
  "agent-inspector": "admin-agent-inspector",
  audit: "admin-audit",
  retention: "admin-retention",
  "mail-domains": "admin-mail-domains",
};

/** Thin compatibility wrapper; all paths and rendering rules live in the DS. */
export function AdminNavIcon({
  name,
  size = 20,
  strokeWidth = 1.75,
  ...props
}: Omit<SVGProps<SVGSVGElement>, "children" | "height" | "viewBox" | "width"> & {
  name: AdminNavIconName;
  size?: number | string;
  strokeWidth?: number;
}) {
  return (
    <Icon
      {...props}
      aria-hidden
      name={ADMIN_NAV_ICON[name]}
      size={size}
      strokeWidth={strokeWidth}
    />
  );
}
