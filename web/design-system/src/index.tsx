import type {
  ButtonHTMLAttributes,
  HTMLAttributes,
  InputHTMLAttributes,
  ReactElement,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from "react";
import { Children, cloneElement, forwardRef, useId } from "react";
import { Icon, type IconName } from "./icons";

export * from "./icons";
export * from "./command-palette";
export * from "./context-bar";
export * from "./dialog";
export * from "./unknown-write-lock";

export type BadgeTone = "neutral" | "positive" | "warning" | "critical" | "info";

export const Button = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & {
    variant?: "primary" | "secondary" | "danger" | "quiet";
  }
>(function Button({ variant = "primary", className = "", ...props }, ref) {
  return (
    <button
      ref={ref}
      className={`button button--${variant} ${className}`}
      {...props}
    />
  );
});

export function Badge({
  tone = "neutral",
  children,
}: {
  tone?: BadgeTone;
  children: ReactNode;
}) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

export function IconButton({
  icon,
  label,
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  icon: IconName;
  label: string;
}) {
  return (
    <button
      aria-label={label}
      className={`icon-button ${className}`}
      title={props.title ?? label}
      type={props.type ?? "button"}
      {...props}
    >
      <Icon aria-hidden name={icon} size={20} />
    </button>
  );
}

/**
 * Stable identity categories shared by the Human mail client and control plane.
 * A caller must supply localized visible text; the kind never renders a hidden
 * English fallback because identity cannot rely on colour or an icon alone.
 */
export type IdentityKind = "human" | "agent" | "shared" | "group" | "service" | "system";

/** @deprecated Prefer IdentityKind in new code. */
export type EntityKind = IdentityKind;

const identityIcon: Record<IdentityKind, IconName> = {
  human: "human",
  agent: "agent",
  shared: "shared",
  group: "group",
  // A persisted Service principal is not a System actor. It uses the bounded
  // system-core glyph as a visual grammar, while its required label remains
  // the localized factual word "Service".
  service: "system",
  system: "system",
};

export type IdentityBadgeProps = Omit<HTMLAttributes<HTMLSpanElement>, "children"> & {
  /** Identity grammar to show through shape and a product-owned SVG. */
  kind: IdentityKind;
  /** Required localized visible label, such as “Agent” or a person’s name. */
  label: string;
};

/**
 * Compact, factual identity marker. It deliberately has no avatar, initials,
 * inferred ownership, or status: those facts require an explicit API field.
 */
export function IdentityBadge({
  kind,
  label,
  className = "",
  ...props
}: IdentityBadgeProps) {
  return (
    <span
      {...props}
      className={`identity-badge entity-badge identity-badge--${kind} ${className}`}
      data-identity-kind={kind}
    >
      <span className="identity-badge__mark" aria-hidden="true">
        <Icon aria-hidden name={identityIcon[kind]} size={16} />
      </span>
      <span className="identity-badge__label">{label}</span>
    </span>
  );
}

/**
 * Compatibility name for existing app consumers. It intentionally renders the
 * exact IdentityBadge markup, so upgrades remain visual rather than semantic.
 */
export function EntityBadge(props: IdentityBadgeProps) {
  return <IdentityBadge {...props} />;
}

const statusIcon: Record<BadgeTone, IconName> = {
  neutral: "status-neutral",
  positive: "status-positive",
  warning: "status-warning",
  critical: "status-critical",
  info: "status-info",
};

export type StatusIndicatorProps = Omit<
  HTMLAttributes<HTMLSpanElement>,
  "aria-live" | "children"
> & {
  /** Semantic state treatment; it is always paired with a glyph and text. */
  tone?: BadgeTone;
  /** Required localized, factual status text. */
  label: string;
  /** Opt into an announcement only when this exact state changes asynchronously. */
  live?: "off" | "polite" | "assertive";
};

/**
 * A compact status signal with a semantic SVG and visible localized text.
 * Use it for API-backed states only; it must not stand in for an inferred
 * runtime state, delivery result, or permission decision.
 */
export function StatusIndicator({
  tone = "neutral",
  label,
  live = "off",
  className = "",
  role,
  ...props
}: StatusIndicatorProps) {
  const announces = live !== "off";

  return (
    <span
      {...props}
      aria-atomic={announces ? "true" : undefined}
      aria-live={announces ? live : undefined}
      className={`status-indicator status-indicator--${tone} ${className}`}
      data-status-tone={tone}
      role={announces ? "status" : role}
    >
      <span className="status-indicator__mark" aria-hidden="true">
        <Icon aria-hidden name={statusIcon[tone]} size={16} />
      </span>
      <span className="status-indicator__label">{label}</span>
    </span>
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactElement<{
    id?: string;
    "aria-describedby"?: string;
  }>;
}) {
  const generatedId = useId();
  const control = Children.only(children);
  const controlId = control.props.id ?? `${generatedId}-control`;
  const hintId = `${generatedId}-hint`;
  const describedBy = [
    control.props["aria-describedby"],
    hint ? hintId : undefined,
  ].filter(Boolean).join(" ") || undefined;

  return (
    <div className="field">
      <label className="field__label" htmlFor={controlId}>{label}</label>
      {cloneElement(control, {
        id: controlId,
        "aria-describedby": describedBy,
      })}
      {hint ? <span className="field__hint" id={hintId}>{hint}</span> : null}
    </div>
  );
}

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className = "", ...props }, ref) {
    return <input {...props} ref={ref} className={`control ${className}`.trim()} />;
  },
);

export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(
  function Select({ className = "", ...props }, ref) {
    return <select {...props} ref={ref} className={`control ${className}`.trim()} />;
  },
);

export const Textarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(function Textarea({ className = "", ...props }, ref) {
  return <textarea {...props} ref={ref} className={`control ${className}`.trim()} />;
});

export type PanelProps = Omit<HTMLAttributes<HTMLElement>, "title"> & {
  /**
   * Optional visible section title. Unlike the native `title` attribute, this
   * heading is available to touch, keyboard, and assistive-technology users.
   */
  title?: ReactNode;
};

export function Panel({
  className = "",
  title,
  children,
  "aria-labelledby": ariaLabelledBy,
  ...props
}: PanelProps) {
  const titleId = useId();
  const labelledBy = title
    ? [ariaLabelledBy, titleId].filter(Boolean).join(" ")
    : ariaLabelledBy;

  return (
    <section
      {...props}
      aria-labelledby={labelledBy || undefined}
      className={`panel ${className}`}
    >
      {title ? (
        <header className="panel__header">
          <h2 className="panel__title" id={titleId}>{title}</h2>
        </header>
      ) : null}
      {children}
    </section>
  );
}

type EmptyStateBaseProps = {
  title: string;
  description: string;
  action?: ReactNode;
  icon?: IconName;
};

/**
 * An illustration is content, not decoration: callers must explicitly supply
 * its alternative text. Passing an empty string remains possible when the
 * caller has deliberately made the illustration decorative.
 */
export type EmptyStateProps = EmptyStateBaseProps & (
  | {
    illustration?: never;
    illustrationAlt?: never;
  }
  | {
    illustration: string;
    illustrationAlt: string;
  }
);

export function EmptyState({
  title,
  description,
  action,
  icon = "mail",
  illustration,
  illustrationAlt,
}: EmptyStateProps) {
  return (
    <div className="empty-state">
      {illustration ? (
        <img className="empty-state__illustration" src={illustration} alt={illustrationAlt} />
      ) : (
        <div className="empty-state__mark" aria-hidden="true"><Icon name={icon} size={28} /></div>
      )}
      <h3>{title}</h3>
      <p>{description}</p>
      {action}
    </div>
  );
}

export function Spinner({ label }: { label: string }) {
  return (
    <div className="loading" role="status">
      <span className="spinner" aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}
