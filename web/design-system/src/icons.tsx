import type { ReactElement, ReactNode, SVGProps } from "react";

/**
 * Names for the product-owned icon set. Keep this list intentionally small:
 * an icon is added only when it represents a stable product concept.
 */
export const ICON_NAMES = [
  "compose",
  "add",
  "check",
  "search",
  "menu",
  "refresh",
  "close",
  "star",
  "star-filled",
  "attachment",
  "sign-out",
  "mail",
  "inbox",
  "sent",
  "draft",
  "arrow-left",
  "reply",
  "chevron-down",
  "human",
  "agent",
  "shared",
  "group",
  "system",
  "status-neutral",
  "status-positive",
  "status-warning",
  "status-critical",
  "status-info",
  "send",
  "command",
  "filter",
  "panel",
  "admin-overview",
  "admin-cases",
  "admin-holds",
  "admin-approvals",
  "admin-roles",
  "admin-agent-inspector",
  "admin-audit",
  "admin-retention",
  "admin-mail-domains",
] as const;

export type IconName = (typeof ICON_NAMES)[number];

export type IconProps = Omit<
  SVGProps<SVGSVGElement>,
  "children" | "height" | "viewBox" | "width"
> & {
  /** Stable name from the local icon registry. */
  name: IconName;
  /** CSS-compatible square dimension. Defaults to the 24px system grid. */
  size?: number | string;
  /** Stroke width in the 24 by 24 viewBox. Defaults to 1.75. */
  strokeWidth?: number;
  /** Provides the accessible name when the icon is not decorative. */
  title?: string;
};

export type IconComponentProps = Omit<IconProps, "name">;

/**
 * Every product icon is a standalone component so an application bundles
 * only the glyphs it renders. All components share the same 24px viewBox,
 * round terminals, and 1.75px default stroke via the internal IconSvg shell.
 */
type IconSvgProps = IconComponentProps & { name: string; children: ReactNode };

function IconSvg({
  name,
  size = 24,
  strokeWidth = 1.75,
  title,
  role,
  children,
  "aria-hidden": ariaHidden,
  "aria-label": ariaLabel,
  ...svgProps
}: IconSvgProps) {
  const isDecorative = ariaHidden === true || (!title && !ariaLabel);

  return (
    <svg
      {...svgProps}
      aria-hidden={isDecorative ? true : ariaHidden}
      aria-label={ariaLabel}
      data-icon={name}
      fill="none"
      focusable="false"
      height={size}
      role={role ?? (isDecorative ? undefined : "img")}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={strokeWidth}
      viewBox="0 0 24 24"
      width={size}
    >
      {title ? <title>{title}</title> : null}
      {children}
    </svg>
  );
}

export function ComposeIcon(props: IconComponentProps) {
  return (
    <IconSvg name="compose" {...props}>
    <path d="M5 19h3.25L19 8.25A2.12 2.12 0 0 0 16 5.25L5.25 16v3Z" />
    <path d="m14.75 6.5 2.75 2.75" />
    <path d="M5 5h6" />
    </IconSvg>
  );
}

export function AddIcon(props: IconComponentProps) {
  return (
    <IconSvg name="add" {...props}>
    <path d="M12 5v14" />
    <path d="M5 12h14" />
    </IconSvg>
  );
}

export function CheckIcon(props: IconComponentProps) {
  return (
    <IconSvg name="check" {...props}>
    <path d="m5 12.5 4.4 4.4L19 7.3" />
    </IconSvg>
  );
}

export function SearchIcon(props: IconComponentProps) {
  return (
    <IconSvg name="search" {...props}>
    <circle cx="10.75" cy="10.75" r="5.75" />
    <path d="m15 15 4 4" />
    </IconSvg>
  );
}

  /** Three horizontal bars: opens a navigation drawer, never a generic list. */
export function MenuIcon(props: IconComponentProps) {
  return (
    <IconSvg name="menu" {...props}>
    <path d="M4 6.5h16" />
    <path d="M4 12h16" />
    <path d="M4 17.5h16" />
    </IconSvg>
  );
}

export function RefreshIcon(props: IconComponentProps) {
  return (
    <IconSvg name="refresh" {...props}>
    <path d="M19 9a7.5 7.5 0 1 0 1 6.5" />
    <path d="M19 4v5h-5" />
    </IconSvg>
  );
}

export function CloseIcon(props: IconComponentProps) {
  return (
    <IconSvg name="close" {...props}>
    <path d="m6 6 12 12" />
    <path d="M18 6 6 18" />
    </IconSvg>
  );
}

export function StarIcon(props: IconComponentProps) {
  return (
    <IconSvg name="star" {...props}>
    <path d="m12 3 2.78 5.63 6.22.9-4.5 4.38 1.06 6.19L12 17.18 6.44 20.1 7.5 13.9 3 9.53l6.22-.9L12 3Z" />
    </IconSvg>
  );
}

export function StarFilledIcon(props: IconComponentProps) {
  return (
    <IconSvg name="star-filled" {...props}>
    <path
    d="m12 3 2.78 5.63 6.22.9-4.5 4.38 1.06 6.19L12 17.18 6.44 20.1 7.5 13.9 3 9.53l6.22-.9L12 3Z"
    fill="currentColor"
        />
    </IconSvg>
  );
}

export function AttachmentIcon(props: IconComponentProps) {
  return (
    <IconSvg name="attachment" {...props}>
    <path d="m8.5 12.5 6.82-6.82a3.25 3.25 0 0 1 4.6 4.6L11.7 18.5a5 5 0 1 1-7.07-7.07l8-8" />
    </IconSvg>
  );
}

export function SignOutIcon(props: IconComponentProps) {
  return (
    <IconSvg name="sign-out" {...props}>
    <path d="M11 5H6.75A1.75 1.75 0 0 0 5 6.75v10.5C5 18.22 5.78 19 6.75 19H11" />
    <path d="M13 8.5 16.5 12 13 15.5" />
    <path d="M9 12h7.5" />
    </IconSvg>
  );
}

export function MailIcon(props: IconComponentProps) {
  return (
    <IconSvg name="mail" {...props}>
    <rect x="3.5" y="5" width="17" height="14" rx="2" />
    <path d="m4.5 7 7.5 5.5L19.5 7" />
    </IconSvg>
  );
}

  /** An inbound tray: navigation to received mail, distinct from one message. */
export function InboxIcon(props: IconComponentProps) {
  return (
    <IconSvg name="inbox" {...props}>
    <path d="M4 6h16v11.25A1.75 1.75 0 0 1 18.25 19h-12.5A1.75 1.75 0 0 1 4 17.25V6Z" />
    <path d="M4 14h4.15l1.5 2.5h4.7l1.5-2.5H20" />
    <path d="M8 9h8" />
    </IconSvg>
  );
}

  /** A mail tray with an outbound arrow: accepted outbound mail, not delivery. */
export function SentIcon(props: IconComponentProps) {
  return (
    <IconSvg name="sent" {...props}>
    <path d="M4 9.5h11.5v8.75A1.75 1.75 0 0 1 13.75 20h-8A1.75 1.75 0 0 1 4 18.25V9.5Z" />
    <path d="m4.75 10.5 5 3.6 5-3.6" />
    <path d="M14.5 4.5H20V10" />
    <path d="m20 4.5-7.25 7.25" />
    </IconSvg>
  );
}

  /** A persisted message draft: document structure plus an editing pencil. */
export function DraftIcon(props: IconComponentProps) {
  return (
    <IconSvg name="draft" {...props}>
    <path d="M6 4h8l4 4v12H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" />
    <path d="M14 4v4h4" />
    <path d="M8 12h4" />
    <path d="m10 17 5.5-5.5 1.5 1.5-5.5 5.5L9 19l1-2Z" />
    </IconSvg>
  );
}

export function ArrowLeftIcon(props: IconComponentProps) {
  return (
    <IconSvg name="arrow-left" {...props}>
    <path d="M19 12H5" />
    <path d="m11 6-6 6 6 6" />
    </IconSvg>
  );
}

export function ReplyIcon(props: IconComponentProps) {
  return (
    <IconSvg name="reply" {...props}>
    <path d="M10 7 4.5 12 10 17" />
    <path d="M5 12h8.5a5 5 0 0 1 5 5" />
    </IconSvg>
  );
}

export function ChevronDownIcon(props: IconComponentProps) {
  return (
    <IconSvg name="chevron-down" {...props}>
    <path d="m7 10 5 5 5-5" />
    </IconSvg>
  );
}

export function HumanIcon(props: IconComponentProps) {
  return (
    <IconSvg name="human" {...props}>
    <circle cx="12" cy="8" r="3" />
    <path d="M5.5 20a6.5 6.5 0 0 1 13 0" />
    </IconSvg>
  );
}

export function AgentIcon(props: IconComponentProps) {
  return (
    <IconSvg name="agent" {...props}>
    <rect x="7" y="7" width="10" height="10" rx="3" />
    <path d="M9 12h6" />
    <path d="M9 5h6" />
    <path d="M4 12h3" />
    <path d="M17 12h3" />
    </IconSvg>
  );
}

  /** Two adjacent mail trays: a mailbox that belongs to more than one actor. */
export function SharedIcon(props: IconComponentProps) {
  return (
    <IconSvg name="shared" {...props}>
    <path d="M3.5 7.5h6.75l1.25 3.25V18H3.5V7.5Z" />
    <path d="M3.5 13.5h1.9l1.05 1.5h1.65l1.05-1.5h2.2" />
    <path d="M12.5 7.5h6.75l1.25 3.25V18h-8V7.5Z" />
    <path d="M12.5 13.5h1.9l1.05 1.5h1.65l1.05-1.5h2.2" />
    </IconSvg>
  );
}

  /** A routed mailbox: three route nodes terminate at one mail core. */
export function GroupIcon(props: IconComponentProps) {
  return (
    <IconSvg name="group" {...props}>
    <circle cx="4.5" cy="5" r="1.5" />
    <circle cx="19.5" cy="8" r="1.5" />
    <circle cx="5.5" cy="19" r="1.5" />
    <path d="m5.7 6.15 2.55 2.45" />
    <path d="M18 8.5h-2" />
    <path d="m6.7 17.85 2.55-2.45" />
    <rect x="8" y="9" width="8" height="7" rx="1.5" />
    <path d="m8.75 10.25 3.25 2.45 3.25-2.45" />
    </IconSvg>
  );
}

  /** A bounded control-plane core, never a generic application panel. */
export function SystemIcon(props: IconComponentProps) {
  return (
    <IconSvg name="system" {...props}>
    <rect x="3.5" y="3.5" width="17" height="17" rx="3.25" />
    <rect x="8.5" y="8.5" width="7" height="7" rx="2" />
    <path d="M12 5.5v3" />
    <path d="M12 15.5v3" />
    <path d="M5.5 12h3" />
    <path d="M15.5 12h3" />
    </IconSvg>
  );
}

export function StatusNeutralIcon(props: IconComponentProps) {
  return (
    <IconSvg name="status-neutral" {...props}>
    <circle cx="12" cy="12" r="7.5" />
    <path d="M8.75 12h6.5" />
    </IconSvg>
  );
}

export function StatusPositiveIcon(props: IconComponentProps) {
  return (
    <IconSvg name="status-positive" {...props}>
    <circle cx="12" cy="12" r="7.5" />
    <path d="m8.5 12.25 2.25 2.25 4.75-5" />
    </IconSvg>
  );
}

export function StatusWarningIcon(props: IconComponentProps) {
  return (
    <IconSvg name="status-warning" {...props}>
    <path d="m12 4.5 8 14H4l8-14Z" />
    <path d="M12 9v4.25" />
    <path d="M12 16h.01" />
    </IconSvg>
  );
}

export function StatusCriticalIcon(props: IconComponentProps) {
  return (
    <IconSvg name="status-critical" {...props}>
    <path d="M9 4.5h6l4.25 4.25v6L15 19.5H9L4.75 15V9L9 4.5Z" />
    <path d="m9.5 9.5 5 5" />
    <path d="m14.5 9.5-5 5" />
    </IconSvg>
  );
}

export function StatusInfoIcon(props: IconComponentProps) {
  return (
    <IconSvg name="status-info" {...props}>
    <circle cx="12" cy="12" r="7.5" />
    <path d="M12 10.75V16" />
    <path d="M12 8h.01" />
    </IconSvg>
  );
}

export function SendIcon(props: IconComponentProps) {
  return (
    <IconSvg name="send" {...props}>
    <path d="m21 3-7.5 18-3.25-7.25L3 10.5 21 3Z" />
    <path d="m10.25 13.75 4.25-4.25" />
    </IconSvg>
  );
}

export function CommandIcon(props: IconComponentProps) {
  return (
    <IconSvg name="command" {...props}>
    <path d="M9.5 9.5H7.25A2.25 2.25 0 1 1 9.5 7.25V16.75A2.25 2.25 0 1 1 7.25 14.5H16.75A2.25 2.25 0 1 1 14.5 16.75V7.25A2.25 2.25 0 1 1 16.75 9.5H7.25" />
    </IconSvg>
  );
}

export function FilterIcon(props: IconComponentProps) {
  return (
    <IconSvg name="filter" {...props}>
    <path d="M4 6h16" />
    <path d="M7 12h10" />
    <path d="M10 18h4" />
    </IconSvg>
  );
}

export function PanelIcon(props: IconComponentProps) {
  return (
    <IconSvg name="panel" {...props}>
    <rect x="4" y="4" width="16" height="16" rx="2" />
    <path d="M14 4v16" />
    <path d="M17 9h.01" />
    <path d="M17 12h.01" />
    <path d="M17 15h.01" />
    </IconSvg>
  );
}

export function AdminOverviewIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-overview" {...props}>
    <rect x="4" y="4" width="16" height="16" rx="2" />
    <path d="M4 10h16M10 4v16" />
    </IconSvg>
  );
}

export function AdminCasesIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-cases" {...props}>
    <path d="M7 5.5A1.5 1.5 0 0 1 8.5 4h8A1.5 1.5 0 0 1 18 5.5v13A1.5 1.5 0 0 1 16.5 20h-8A1.5 1.5 0 0 1 7 18.5v-13Z" />
    <path d="M5.5 7H4.75A1.75 1.75 0 0 0 3 8.75v10.5c0 .97.78 1.75 1.75 1.75h8.5" />
    <path d="M10 8h5M10 12h5M10 16h3" />
    </IconSvg>
  );
}

export function AdminHoldsIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-holds" {...props}>
    <rect x="4.5" y="10" width="15" height="10" rx="2" />
    <path d="M8 10V7.75a4 4 0 0 1 8 0V10" />
    <path d="M12 14v2" />
    </IconSvg>
  );
}

export function AdminApprovalsIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-approvals" {...props}>
    <circle cx="12" cy="12" r="8" />
    <path d="m8.5 12 2.3 2.3 4.7-4.7" />
    </IconSvg>
  );
}

export function AdminRolesIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-roles" {...props}>
    <circle cx="12" cy="8" r="2.75" />
    <circle cx="5.75" cy="10" r="2.25" />
    <circle cx="18.25" cy="10" r="2.25" />
    <path d="M6.5 20a5.5 5.5 0 0 1 11 0M1.75 20a4 4 0 0 1 3.5-4M22.25 20a4 4 0 0 0-3.5-4" />
    </IconSvg>
  );
}

export function AdminAgentInspectorIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-agent-inspector" {...props}>
    <path d="M5.5 5.5A2.5 2.5 0 0 1 8 3h8a2.5 2.5 0 0 1 2.5 2.5v13A2.5 2.5 0 0 1 16 21H8a2.5 2.5 0 0 1-2.5-2.5v-13Z" />
    <circle cx="12" cy="9" r="2.25" />
    <path d="M8.5 16.5c.8-1.7 2-2.5 3.5-2.5s2.7.8 3.5 2.5M3 8h2.5M18.5 8H21M3 16h2.5M18.5 16H21" />
    </IconSvg>
  );
}

export function AdminAuditIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-audit" {...props}>
    <path d="M7 4h7l3 3v13H7a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" />
    <path d="M14 4v4h4M8.5 12h7M8.5 16h5" />
    <circle cx="17.5" cy="17.5" r="2.5" />
    <path d="m19.25 19.25 1.75 1.75" />
    </IconSvg>
  );
}

export function AdminRetentionIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-retention" {...props}>
    <path d="M4 8.5h16v10A1.5 1.5 0 0 1 18.5 20h-13A1.5 1.5 0 0 1 4 18.5v-10Z" />
    <path d="M3 8.5 5.25 4h13.5L21 8.5M9.5 13h5" />
    </IconSvg>
  );
}

export function AdminMailDomainsIcon(props: IconComponentProps) {
  return (
    <IconSvg name="admin-mail-domains" {...props}>
    <circle cx="12" cy="12" r="8" />
    <path d="M4.55 12h14.9M12 4a12.5 12.5 0 0 1 0 16M12 4a12.5 12.5 0 0 0 0 16" />
    <path d="M8.1 7.1h7.8M8.1 16.9h7.8" />
    </IconSvg>
  );
}

/**
 * Dynamic-name registry kept for callers that cannot know the icon at author
 * time (navigation maps, command entries). Prefer the standalone components
 * in code paths that know their icon: they tree-shake to a single glyph.
 */
export const ICON_REGISTRY: Readonly<
  Record<IconName, (props: IconComponentProps) => ReactElement>
> = {
  "compose": ComposeIcon,
  "add": AddIcon,
  "check": CheckIcon,
  "search": SearchIcon,
  "menu": MenuIcon,
  "refresh": RefreshIcon,
  "close": CloseIcon,
  "star": StarIcon,
  "star-filled": StarFilledIcon,
  "attachment": AttachmentIcon,
  "sign-out": SignOutIcon,
  "mail": MailIcon,
  "inbox": InboxIcon,
  "sent": SentIcon,
  "draft": DraftIcon,
  "arrow-left": ArrowLeftIcon,
  "reply": ReplyIcon,
  "chevron-down": ChevronDownIcon,
  "human": HumanIcon,
  "agent": AgentIcon,
  "shared": SharedIcon,
  "group": GroupIcon,
  "system": SystemIcon,
  "status-neutral": StatusNeutralIcon,
  "status-positive": StatusPositiveIcon,
  "status-warning": StatusWarningIcon,
  "status-critical": StatusCriticalIcon,
  "status-info": StatusInfoIcon,
  "send": SendIcon,
  "command": CommandIcon,
  "filter": FilterIcon,
  "panel": PanelIcon,
  "admin-overview": AdminOverviewIcon,
  "admin-cases": AdminCasesIcon,
  "admin-holds": AdminHoldsIcon,
  "admin-approvals": AdminApprovalsIcon,
  "admin-roles": AdminRolesIcon,
  "admin-agent-inspector": AdminAgentInspectorIcon,
  "admin-audit": AdminAuditIcon,
  "admin-retention": AdminRetentionIcon,
  "admin-mail-domains": AdminMailDomainsIcon,
};

/**
 * Renders one product icon. Icons are decorative by default; pass `title` or
 * `aria-label` when the SVG is the only accessible label for a control.
 */
export function Icon({ name, ...props }: IconProps) {
  const Glyph = ICON_REGISTRY[name];
  return <Glyph {...props} />;
}
