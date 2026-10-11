import type { ReactNode, SVGProps } from "react";

/**
 * Names for the product-owned icon set. Keep this list intentionally small:
 * an icon is added only when it represents a stable product concept.
 */
export const ICON_NAMES = [
  "compose",
  "add",
  "check",
  "search",
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

type IconGlyph = () => ReactNode;

/**
 * Product-owned paths only. Every glyph inherits the same 24px viewBox,
 * round terminals, and 1.75px default stroke from Icon.
 */
export const ICON_REGISTRY: Readonly<Record<IconName, IconGlyph>> = {
  compose: () => (
    <>
      <path d="M5 19h3.25L19 8.25A2.12 2.12 0 0 0 16 5.25L5.25 16v3Z" />
      <path d="m14.75 6.5 2.75 2.75" />
      <path d="M5 5h6" />
    </>
  ),
  add: () => (
    <>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </>
  ),
  check: () => <path d="m5 12.5 4.4 4.4L19 7.3" />,
  search: () => (
    <>
      <circle cx="10.75" cy="10.75" r="5.75" />
      <path d="m15 15 4 4" />
    </>
  ),
  refresh: () => (
    <>
      <path d="M19 9a7.5 7.5 0 1 0 1 6.5" />
      <path d="M19 4v5h-5" />
    </>
  ),
  close: () => (
    <>
      <path d="m6 6 12 12" />
      <path d="M18 6 6 18" />
    </>
  ),
  star: () => <path d="m12 3 2.78 5.63 6.22.9-4.5 4.38 1.06 6.19L12 17.18 6.44 20.1 7.5 13.9 3 9.53l6.22-.9L12 3Z" />,
  "star-filled": () => (
    <path
      d="m12 3 2.78 5.63 6.22.9-4.5 4.38 1.06 6.19L12 17.18 6.44 20.1 7.5 13.9 3 9.53l6.22-.9L12 3Z"
      fill="currentColor"
    />
  ),
  attachment: () => <path d="m8.5 12.5 6.82-6.82a3.25 3.25 0 0 1 4.6 4.6L11.7 18.5a5 5 0 1 1-7.07-7.07l8-8" />,
  "sign-out": () => (
    <>
      <path d="M11 5H6.75A1.75 1.75 0 0 0 5 6.75v10.5C5 18.22 5.78 19 6.75 19H11" />
      <path d="M13 8.5 16.5 12 13 15.5" />
      <path d="M9 12h7.5" />
    </>
  ),
  mail: () => (
    <>
      <rect x="3.5" y="5" width="17" height="14" rx="2" />
      <path d="m4.5 7 7.5 5.5L19.5 7" />
    </>
  ),
  /** An inbound tray: navigation to received mail, distinct from one message. */
  inbox: () => (
    <>
      <path d="M4 6h16v11.25A1.75 1.75 0 0 1 18.25 19h-12.5A1.75 1.75 0 0 1 4 17.25V6Z" />
      <path d="M4 14h4.15l1.5 2.5h4.7l1.5-2.5H20" />
      <path d="M8 9h8" />
    </>
  ),
  /** A mail tray with an outbound arrow: accepted outbound mail, not delivery. */
  sent: () => (
    <>
      <path d="M4 9.5h11.5v8.75A1.75 1.75 0 0 1 13.75 20h-8A1.75 1.75 0 0 1 4 18.25V9.5Z" />
      <path d="m4.75 10.5 5 3.6 5-3.6" />
      <path d="M14.5 4.5H20V10" />
      <path d="m20 4.5-7.25 7.25" />
    </>
  ),
  /** A persisted message draft: document structure plus an editing pencil. */
  draft: () => (
    <>
      <path d="M6 4h8l4 4v12H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" />
      <path d="M14 4v4h4" />
      <path d="M8 12h4" />
      <path d="m10 17 5.5-5.5 1.5 1.5-5.5 5.5L9 19l1-2Z" />
    </>
  ),
  "arrow-left": () => (
    <>
      <path d="M19 12H5" />
      <path d="m11 6-6 6 6 6" />
    </>
  ),
  reply: () => (
    <>
      <path d="M10 7 4.5 12 10 17" />
      <path d="M5 12h8.5a5 5 0 0 1 5 5" />
    </>
  ),
  "chevron-down": () => <path d="m7 10 5 5 5-5" />,
  human: () => (
    <>
      <circle cx="12" cy="8" r="3" />
      <path d="M5.5 20a6.5 6.5 0 0 1 13 0" />
    </>
  ),
  agent: () => (
    <>
      <rect x="7" y="7" width="10" height="10" rx="3" />
      <path d="M9 12h6" />
      <path d="M9 5h6" />
      <path d="M4 12h3" />
      <path d="M17 12h3" />
    </>
  ),
  /** Two adjacent mail trays: a mailbox that belongs to more than one actor. */
  shared: () => (
    <>
      <path d="M3.5 7.5h6.75l1.25 3.25V18H3.5V7.5Z" />
      <path d="M3.5 13.5h1.9l1.05 1.5h1.65l1.05-1.5h2.2" />
      <path d="M12.5 7.5h6.75l1.25 3.25V18h-8V7.5Z" />
      <path d="M12.5 13.5h1.9l1.05 1.5h1.65l1.05-1.5h2.2" />
    </>
  ),
  /** A routed mailbox: three route nodes terminate at one mail core. */
  group: () => (
    <>
      <circle cx="4.5" cy="5" r="1.5" />
      <circle cx="19.5" cy="8" r="1.5" />
      <circle cx="5.5" cy="19" r="1.5" />
      <path d="m5.7 6.15 2.55 2.45" />
      <path d="M18 8.5h-2" />
      <path d="m6.7 17.85 2.55-2.45" />
      <rect x="8" y="9" width="8" height="7" rx="1.5" />
      <path d="m8.75 10.25 3.25 2.45 3.25-2.45" />
    </>
  ),
  /** A bounded control-plane core, never a generic application panel. */
  system: () => (
    <>
      <rect x="3.5" y="3.5" width="17" height="17" rx="3.25" />
      <rect x="8.5" y="8.5" width="7" height="7" rx="2" />
      <path d="M12 5.5v3" />
      <path d="M12 15.5v3" />
      <path d="M5.5 12h3" />
      <path d="M15.5 12h3" />
    </>
  ),
  "status-neutral": () => (
    <>
      <circle cx="12" cy="12" r="7.5" />
      <path d="M8.75 12h6.5" />
    </>
  ),
  "status-positive": () => (
    <>
      <circle cx="12" cy="12" r="7.5" />
      <path d="m8.5 12.25 2.25 2.25 4.75-5" />
    </>
  ),
  "status-warning": () => (
    <>
      <path d="m12 4.5 8 14H4l8-14Z" />
      <path d="M12 9v4.25" />
      <path d="M12 16h.01" />
    </>
  ),
  "status-critical": () => (
    <>
      <path d="M9 4.5h6l4.25 4.25v6L15 19.5H9L4.75 15V9L9 4.5Z" />
      <path d="m9.5 9.5 5 5" />
      <path d="m14.5 9.5-5 5" />
    </>
  ),
  "status-info": () => (
    <>
      <circle cx="12" cy="12" r="7.5" />
      <path d="M12 10.75V16" />
      <path d="M12 8h.01" />
    </>
  ),
  send: () => (
    <>
      <path d="m21 3-7.5 18-3.25-7.25L3 10.5 21 3Z" />
      <path d="m10.25 13.75 4.25-4.25" />
    </>
  ),
  command: () => (
    <>
      <path d="M9.5 9.5H7.25A2.25 2.25 0 1 1 9.5 7.25V16.75A2.25 2.25 0 1 1 7.25 14.5H16.75A2.25 2.25 0 1 1 14.5 16.75V7.25A2.25 2.25 0 1 1 16.75 9.5H7.25" />
    </>
  ),
  filter: () => (
    <>
      <path d="M4 6h16" />
      <path d="M7 12h10" />
      <path d="M10 18h4" />
    </>
  ),
  panel: () => (
    <>
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <path d="M14 4v16" />
      <path d="M17 9h.01" />
      <path d="M17 12h.01" />
      <path d="M17 15h.01" />
    </>
  ),
  "admin-overview": () => (
    <>
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <path d="M4 10h16M10 4v16" />
    </>
  ),
  "admin-cases": () => (
    <>
      <path d="M7 5.5A1.5 1.5 0 0 1 8.5 4h8A1.5 1.5 0 0 1 18 5.5v13A1.5 1.5 0 0 1 16.5 20h-8A1.5 1.5 0 0 1 7 18.5v-13Z" />
      <path d="M5.5 7H4.75A1.75 1.75 0 0 0 3 8.75v10.5c0 .97.78 1.75 1.75 1.75h8.5" />
      <path d="M10 8h5M10 12h5M10 16h3" />
    </>
  ),
  "admin-holds": () => (
    <>
      <rect x="4.5" y="10" width="15" height="10" rx="2" />
      <path d="M8 10V7.75a4 4 0 0 1 8 0V10" />
      <path d="M12 14v2" />
    </>
  ),
  "admin-approvals": () => (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="m8.5 12 2.3 2.3 4.7-4.7" />
    </>
  ),
  "admin-roles": () => (
    <>
      <circle cx="12" cy="8" r="2.75" />
      <circle cx="5.75" cy="10" r="2.25" />
      <circle cx="18.25" cy="10" r="2.25" />
      <path d="M6.5 20a5.5 5.5 0 0 1 11 0M1.75 20a4 4 0 0 1 3.5-4M22.25 20a4 4 0 0 0-3.5-4" />
    </>
  ),
  "admin-agent-inspector": () => (
    <>
      <path d="M5.5 5.5A2.5 2.5 0 0 1 8 3h8a2.5 2.5 0 0 1 2.5 2.5v13A2.5 2.5 0 0 1 16 21H8a2.5 2.5 0 0 1-2.5-2.5v-13Z" />
      <circle cx="12" cy="9" r="2.25" />
      <path d="M8.5 16.5c.8-1.7 2-2.5 3.5-2.5s2.7.8 3.5 2.5M3 8h2.5M18.5 8H21M3 16h2.5M18.5 16H21" />
    </>
  ),
  "admin-audit": () => (
    <>
      <path d="M7 4h7l3 3v13H7a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" />
      <path d="M14 4v4h4M8.5 12h7M8.5 16h5" />
      <circle cx="17.5" cy="17.5" r="2.5" />
      <path d="m19.25 19.25 1.75 1.75" />
    </>
  ),
  "admin-retention": () => (
    <>
      <path d="M4 8.5h16v10A1.5 1.5 0 0 1 18.5 20h-13A1.5 1.5 0 0 1 4 18.5v-10Z" />
      <path d="M3 8.5 5.25 4h13.5L21 8.5M9.5 13h5" />
    </>
  ),
  "admin-mail-domains": () => (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M4.55 12h14.9M12 4a12.5 12.5 0 0 1 0 16M12 4a12.5 12.5 0 0 0 0 16" />
      <path d="M8.1 7.1h7.8M8.1 16.9h7.8" />
    </>
  ),
};

/**
 * Renders one product icon. Icons are decorative by default; pass `title` or
 * `aria-label` when the SVG is the only accessible label for a control.
 */
export function Icon({
  name,
  size = 24,
  strokeWidth = 1.75,
  title,
  role,
  "aria-hidden": ariaHidden,
  "aria-label": ariaLabel,
  ...svgProps
}: IconProps) {
  const Glyph = ICON_REGISTRY[name];
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
      <Glyph />
    </svg>
  );
}

export function ComposeIcon(props: IconComponentProps) {
  return <Icon name="compose" {...props} />;
}

export function AddIcon(props: IconComponentProps) {
  return <Icon name="add" {...props} />;
}

export function CheckIcon(props: IconComponentProps) {
  return <Icon name="check" {...props} />;
}

export function SearchIcon(props: IconComponentProps) {
  return <Icon name="search" {...props} />;
}

export function RefreshIcon(props: IconComponentProps) {
  return <Icon name="refresh" {...props} />;
}

export function CloseIcon(props: IconComponentProps) {
  return <Icon name="close" {...props} />;
}

export function StarIcon(props: IconComponentProps) {
  return <Icon name="star" {...props} />;
}

export function StarFilledIcon(props: IconComponentProps) {
  return <Icon name="star-filled" {...props} />;
}

export function AttachmentIcon(props: IconComponentProps) {
  return <Icon name="attachment" {...props} />;
}

export function SignOutIcon(props: IconComponentProps) {
  return <Icon name="sign-out" {...props} />;
}

export function MailIcon(props: IconComponentProps) {
  return <Icon name="mail" {...props} />;
}

export function ArrowLeftIcon(props: IconComponentProps) {
  return <Icon name="arrow-left" {...props} />;
}

export function ReplyIcon(props: IconComponentProps) {
  return <Icon name="reply" {...props} />;
}

export function ChevronDownIcon(props: IconComponentProps) {
  return <Icon name="chevron-down" {...props} />;
}

export function HumanIcon(props: IconComponentProps) {
  return <Icon name="human" {...props} />;
}

export function AgentIcon(props: IconComponentProps) {
  return <Icon name="agent" {...props} />;
}

export function SharedIcon(props: IconComponentProps) {
  return <Icon name="shared" {...props} />;
}

export function GroupIcon(props: IconComponentProps) {
  return <Icon name="group" {...props} />;
}

export function SystemIcon(props: IconComponentProps) {
  return <Icon name="system" {...props} />;
}

export function StatusNeutralIcon(props: IconComponentProps) {
  return <Icon name="status-neutral" {...props} />;
}

export function StatusPositiveIcon(props: IconComponentProps) {
  return <Icon name="status-positive" {...props} />;
}

export function StatusWarningIcon(props: IconComponentProps) {
  return <Icon name="status-warning" {...props} />;
}

export function StatusCriticalIcon(props: IconComponentProps) {
  return <Icon name="status-critical" {...props} />;
}

export function StatusInfoIcon(props: IconComponentProps) {
  return <Icon name="status-info" {...props} />;
}

export function SendIcon(props: IconComponentProps) {
  return <Icon name="send" {...props} />;
}

export function CommandIcon(props: IconComponentProps) {
  return <Icon name="command" {...props} />;
}

export function FilterIcon(props: IconComponentProps) {
  return <Icon name="filter" {...props} />;
}

export function PanelIcon(props: IconComponentProps) {
  return <Icon name="panel" {...props} />;
}
