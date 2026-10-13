import type { ReactNode, SVGProps } from "react";

/**
 * Product-owned empty-state scenes. Every illustration is drawn by hand on a
 * 200 by 150 grid out of the shared colour tokens only: soft surface discs,
 * muted ink outlines, and restrained info/agent accents. No third-party path
 * data, raster asset, or external file is involved.
 *
 * Each scene is its own exported component so an application only bundles
 * the scenes it actually mounts. The scenes are decorative by contract: the
 * EmptyState title, description, and action already carry every fact an
 * assistive-technology user needs, so the SVG renders aria-hidden unless the
 * caller explicitly supplies a title.
 */
export type EmptyArtProps = Omit<
  SVGProps<SVGSVGElement>,
  "children" | "viewBox"
> & {
  /** Supplies an accessible name; omit for decorative scenes. */
  title?: string;
};

const INK = "var(--cm-muted-strong)";
const LINE = "var(--cm-line-strong)";
const SURFACE = "var(--cm-surface)";
const SUBTLE = "var(--cm-surface-subtle)";
const INFO = "var(--cm-info)";
const INFO_SOFT = "var(--cm-info-soft)";
const AGENT = "var(--cm-agent)";
const AGENT_SOFT = "var(--cm-agent-soft)";
const POSITIVE = "var(--cm-positive)";
const POSITIVE_SOFT = "var(--cm-positive-soft)";

/**
 * Micro-factories keep every scene to its geometry: attribute names do not
 * minify, so spelling out stroke props per element would dominate the bundle.
 * `o` = ink outline, `s` = fine line detail, `f` = flat fill,
 * `fo` = filled shape with ink outline, `b` = box, `d` = disc.
 */
function o(d: string, w = 3, stroke = INK) {
  return (
    <path
      d={d}
      fill="none"
      stroke={stroke}
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={w}
    />
  );
}

function s(d: string, dash = false) {
  return (
    <path
      d={d}
      fill="none"
      stroke={LINE}
      strokeDasharray={dash ? "2 8" : undefined}
      strokeLinecap="round"
      strokeWidth={5}
    />
  );
}


function f(d: string, fill: string) {
  return <path d={d} fill={fill} />;
}

function fo(d: string, fill: string, w = 3) {
  return <path d={d} fill={fill} stroke={INK} strokeLinejoin="round" strokeWidth={w} />;
}

function b(x: number, y: number, w: number, h: number, rx: number, fill: string, stroke?: string, dash = false) {
  return (
    <rect
      x={x}
      y={y}
      width={w}
      height={h}
      rx={rx}
      fill={fill}
      stroke={stroke}
      strokeWidth={stroke ? 3 : undefined}
      strokeDasharray={dash ? "4 6" : undefined}
    />
  );
}

function d(cx: number, cy: number, r: number, fill: string, stroke?: string, w = 3) {
  return <circle cx={cx} cy={cy} r={r} fill={fill} stroke={stroke} strokeWidth={stroke ? w : undefined} />;
}

/**
 * One shared backdrop keeps the family coherent: a soft disc behind the
 * subject, two quiet accent dots, and a single brand spark.
 */
function Backdrop() {
  return (
    <>
      {d(100, 78, 60, INFO_SOFT)}
      {d(158, 30, 9, AGENT_SOFT)}
      {f("M160 96l2.2 5.4 5.4 2.2-5.4 2.2-2.2 5.4-2.2-5.4-5.4-2.2 5.4-2.2z", INFO)}
    </>
  );
}

function EmptyArt({
  title,
  role,
  children,
  "aria-hidden": ariaHidden,
  "aria-label": ariaLabel,
  ...svgProps
}: EmptyArtProps & { children: ReactNode }) {
  const isDecorative = ariaHidden === true || (!title && !ariaLabel);

  return (
    <svg
      {...svgProps}
      aria-hidden={isDecorative ? true : ariaHidden}
      aria-label={ariaLabel}
      data-empty-art=""
      focusable="false"
      role={role ?? (isDecorative ? undefined : "img")}
      viewBox="0 0 200 150"
    >
      {title ? <title>{title}</title> : null}
      {children}
    </svg>
  );
}

/** A cleared tray with one quiet envelope above it. */
export function EmptyInboxArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {b(84, 22, 34, 23, 4, SURFACE, INK)}
      {o("m87 27 14 10 14-10", 2.5)}
      {fo("M48 82l13-26h78l13 26v28a6 6 0 0 1-6 6H54a6 6 0 0 1-6-6Z", SURFACE)}
      {o("M48 82h26l8 11h36l8-11h26")}
    </EmptyArt>
  );
}

/** A document under a magnifier: search returned nothing. */
export function EmptySearchArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {b(60, 30, 64, 86, 8, SURFACE, INK)}
      {s("M74 52h36M74 66h26M74 80h32")}
      {d(132, 94, 22, INFO_SOFT, INK)}
      {o("m148 110 16 16", 6)}
    </EmptyArt>
  );
}

/** A folded-corner draft sheet with a pencil resting on it. */
export function EmptyDraftsArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {fo("M70 38a8 8 0 0 1 8-8h30l22 22v58a8 8 0 0 1-8 8H78a8 8 0 0 1-8-8Z", SURFACE)}
      {o("M108 30v22h22")}
      {s("M82 70h26M82 84h18")}
      {fo("m118 108 24-24 8 8-24 24-11 3z", AGENT_SOFT)}
    </EmptyArt>
  );
}

/** A paper plane leaving an outbound tray, trail dotted behind it. */
export function EmptySentArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {fo("M56 84 150 36l-30 62-22-22z", SURFACE)}
      {o("M98 76 150 36", 2.5)}
      {s("M44 104c8 8 20 12 34 12", true)}
      {s("M52 116h12")}
    </EmptyArt>
  );
}

/** A compact queue on the left, an arrow, then the open reading pane. */
export function SelectMessageArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {b(34, 46, 42, 10, 5, LINE)}
      {b(34, 64, 42, 10, 5, LINE)}
      {b(34, 82, 42, 10, 5, LINE)}
      {o("M82 69h14m-5-5 5 5-5 5", 2.5)}
      {b(104, 44, 62, 50, 7, SURFACE, INK)}
      {o("m107 52 28 19 28-19", 2.5)}
      {s("M114 104h42")}
    </EmptyArt>
  );
}

/** A record list with one highlighted row beside an empty detail card. */
export function SelectRecordArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {b(38, 42, 54, 66, 7, SURFACE, INK)}
      {s("M48 56h34M48 90h34")}
      {b(44, 66, 42, 14, 4, INFO_SOFT)}
      {b(104, 42, 58, 66, 7, SUBTLE, LINE, true)}
      {s("M114 58h38M114 70h28")}
    </EmptyArt>
  );
}

/** A review clipboard with a completed check badge. */
export function EmptyApprovalsArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {b(64, 32, 66, 88, 8, SURFACE, INK)}
      {b(84, 24, 26, 13, 6.5, SUBTLE, INK)}
      {s("M78 58h38M78 72h28")}
      {d(136, 102, 19, POSITIVE_SOFT, POSITIVE)}
      {o("m127 102 6 6 11-12", 3, POSITIVE)}
    </EmptyArt>
  );
}

/** A ledger sheet sealed by a small evidence shield. */
export function EmptyAuditArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {b(56, 30, 64, 88, 8, SURFACE, INK)}
      {s("M70 52h36M70 66h36M70 80h24")}
      {fo("M138 88l16 6v11c0 11-7 17-16 21-9-4-16-10-16-21V94z", AGENT_SOFT)}
      {o("m131 106 5 5 9-10", 2.5)}
    </EmptyArt>
  );
}

/** A compliance folder carrying a sealed evidence shield. */
export function EmptyComplianceArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {fo("M50 68a6 6 0 0 1 6-6h27l9 10h52a6 6 0 0 1 6 6v32a6 6 0 0 1-6 6H56a6 6 0 0 1-6-6Z", SURFACE)}
      {o("M50 80h100", 2.5)}
      {b(62, 88, 30, 10, 5, INFO_SOFT)}
      {fo("M136 44l13 5v9c0 9-6 14-13 17-7-3-13-8-13-17v-9z", AGENT_SOFT, 2.5)}
    </EmptyArt>
  );
}

/** A routed globe with a mail badge on its edge. */
export function EmptyDomainsArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {d(96, 76, 40, SURFACE, INK)}
      {o("M56 76h80", 2.5)}
      <ellipse
        cx={96}
        cy={76}
        rx={19}
        ry={40}
        fill="none"
        stroke={INK}
        strokeWidth={2.5}
      />
      {o("M64 54c8 5 20 8 32 8s24-3 32-8M64 98c8-5 20-8 32-8s24 3 32 8", 2.5)}
      {b(128, 98, 32, 22, 4, SURFACE, INK)}
      {o("m130 103 14 9 14-9", 2.5)}
    </EmptyArt>
  );
}

/** Two principals standing for unassigned role grants. */
export function EmptyRolesArt(props: EmptyArtProps) {
  return (
    <EmptyArt {...props}>
      <Backdrop />
      {d(80, 60, 15, SURFACE, INK)}
      {fo("M52 112a28 28 0 0 1 56 0", INFO_SOFT)}
      {d(126, 64, 12, SURFACE, INK)}
      {o("M106 112a23 23 0 0 1 34-11")}
      {d(142, 44, 12, AGENT_SOFT, AGENT, 2.5)}
      {o("M142 39v10m-5-5h10", 2.5, AGENT)}
    </EmptyArt>
  );
}
