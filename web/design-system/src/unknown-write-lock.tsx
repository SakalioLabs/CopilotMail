import type { HTMLAttributes, ReactNode } from "react";
import { StatusWarningIcon } from "./icons";

export interface UnknownWriteLockProps extends Omit<
  HTMLAttributes<HTMLElement>,
  "children" | "role"
> {
  /** Localized, factual statement of the unresolved write outcome. */
  title: string;
  /** Explains why the caller is frozen and what must not be attempted. */
  description: string;
  /**
   * The only allowed follow-up affordance. Callers provide a read-only,
   * exact reconciliation action; this primitive deliberately provides no
   * generic retry action or mutation callback. Omit it when the unresolved
   * request has no exact resource that can be read safely.
   */
  reconciliationAction?: ReactNode;
  /** Optional already-safe evidence, such as a public request identifier. */
  evidence?: ReactNode;
  /** Announces a newly entered lock state without requiring focus movement. */
  live?: "off" | "polite" | "assertive";
}

/**
 * A fact-first treatment for a write whose server-side outcome cannot be
 * proved. It has no retry semantics: the supplied follow-up must reconcile
 * through an exact, read-only request before the calling feature unlocks.
 */
export function UnknownWriteLock({
  title,
  description,
  reconciliationAction,
  evidence,
  live = "polite",
  className = "",
  ...props
}: UnknownWriteLockProps) {
  const announces = live !== "off";

  return (
    <section
      {...props}
      aria-atomic={announces ? "true" : undefined}
      aria-live={announces ? live : undefined}
      className={`unknown-write-lock ${className}`.trim()}
      data-write-state="unknown"
      role={announces ? "status" : undefined}
    >
      <div className="unknown-write-lock__mark" aria-hidden="true">
        <StatusWarningIcon aria-hidden size={18} />
      </div>
      <div className="unknown-write-lock__copy">
        <strong>{title}</strong>
        <p>{description}</p>
        {evidence ? <div className="unknown-write-lock__evidence">{evidence}</div> : null}
      </div>
      {reconciliationAction !== undefined && reconciliationAction !== null ? (
        <div className="unknown-write-lock__action">{reconciliationAction}</div>
      ) : null}
    </section>
  );
}
