import {
  useId,
  useState,
  type HTMLAttributes,
  type ReactElement,
  type ReactNode,
} from "react";
import { Icon } from "./icons";
import type { StatusIndicatorProps } from "./index";

/** A fact already authorized for the current screen. ContextBar never infers one. */
export interface ContextBarFact {
  /** Stable key supplied by the consuming page; it is not shown to the user. */
  id: string;
  /** Localized, visible name for the supplied value. */
  label: ReactNode;
  /** The exact DTO-backed value permitted for this screen. */
  value: ReactNode;
}

/**
 * The existing shared StatusIndicator element shown beside a context title.
 * Keeping it caller-owned prevents a design-system barrel dependency cycle and
 * prevents ContextBar from inventing any status wording or tone.
 */
export type ContextBarStatus = ReactElement<StatusIndicatorProps>;

export interface ContextBarProps extends Omit<
  HTMLAttributes<HTMLElement>,
  "aria-labelledby" | "children" | "title"
> {
  /** Visible localized section name. It also names the landmark. */
  title: ReactNode;
  /** Heading depth appropriate to the consuming page hierarchy. */
  headingLevel?: 2 | 3 | 4 | 5 | 6;
  /** Exact label/value facts from the current authorized response. */
  facts: readonly ContextBarFact[];
  /** Optional factual state signal, never an inferred delivery outcome. */
  status?: ContextBarStatus;
  /** Optional caller-owned controls, such as the page's one primary action. */
  actions?: ReactNode;
}

/**
 * A compact semantic region for the currently selected organization, resource,
 * actor, and other already-authorized facts. Facts are rendered as a definition
 * list so the label/value relationship survives both visual and assistive use.
 */
export function ContextBar({
  title,
  headingLevel = 2,
  facts,
  status,
  actions,
  className = "",
  ...props
}: ContextBarProps) {
  const titleId = useId();
  const Heading = `h${headingLevel}` as "h2" | "h3" | "h4" | "h5" | "h6";

  return (
    <section
      {...props}
      aria-labelledby={titleId}
      className={`context-bar ${className}`.trim()}
    >
      <div className="context-bar__header">
        <div className="context-bar__title-group">
          <Heading className="context-bar__title" id={titleId}>{title}</Heading>
          {status ? <div className="context-bar__status">{status}</div> : null}
        </div>
        {actions ? <div className="context-bar__actions">{actions}</div> : null}
      </div>
      <dl className="context-bar__facts">
        {facts.map((fact) => (
          <div key={fact.id} className="context-bar__fact">
            <dt className="context-bar__label">{fact.label}</dt>
            <dd className="context-bar__value">{fact.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

export interface EvidenceInspectorProps extends Omit<
  HTMLAttributes<HTMLDetailsElement>,
  "children" | "onToggle" | "open"
> {
  /** Localized, visible summary of the evidence that can be expanded. */
  summary: ReactNode;
  /** Optional exact count supplied by the consuming response. */
  count?: number;
  children: ReactNode;
  /** Controlled expansion state. When present, the caller owns every change. */
  open?: boolean;
  /** Initial state for uncontrolled use. Defaults to closed for evidence safety. */
  defaultOpen?: boolean;
  /** Reports a user-initiated expansion change; it does not persist evidence. */
  onOpenChange?: (open: boolean) => void;
}

/**
 * A deliberately collapsed native disclosure for request IDs, approvals,
 * immutable events, and other evidence. It keeps evidence in the DOM only for
 * the current render; storage and fetching remain the caller's responsibility.
 */
export function EvidenceInspector({
  summary,
  count,
  children,
  open,
  defaultOpen = false,
  onOpenChange,
  className = "",
  ...props
}: EvidenceInspectorProps) {
  const summaryId = useId();
  const contentId = useId();
  const [uncontrolledOpen, setUncontrolledOpen] = useState(defaultOpen);
  const isControlled = open !== undefined;
  const resolvedOpen = isControlled ? open : uncontrolledOpen;

  return (
    <details
      {...props}
      aria-labelledby={summaryId}
      className={`evidence-inspector ${className}`.trim()}
      open={resolvedOpen}
      onToggle={(event) => {
        const nextOpen = event.currentTarget.open;

        if (isControlled) {
          // Controlled changes are requested from the summary click below.
          // Ignore a native toggle (including one produced by a script) and
          // let the caller's next render remain the single source of truth.
          return;
        }

        setUncontrolledOpen(nextOpen);
        onOpenChange?.(nextOpen);
      }}
    >
      <summary
        aria-controls={contentId}
        // Native <summary> exposes its parent <details> expansion state to
        // assistive technology. In controlled mode the native default is
        // prevented, so expose the caller-owned value explicitly instead.
        aria-expanded={isControlled ? resolvedOpen : undefined}
        className="evidence-inspector__summary"
        id={summaryId}
        onClick={(event) => {
          if (!isControlled) return;

          event.preventDefault();
          onOpenChange?.(!resolvedOpen);
        }}
      >
        <span className="evidence-inspector__summary-label">{summary}</span>
        {count !== undefined ? (
          <span className="evidence-inspector__count">{count}</span>
        ) : null}
        <Icon
          aria-hidden
          className="evidence-inspector__indicator"
          name="chevron-down"
          size={16}
        />
      </summary>
      <div className="evidence-inspector__content" id={contentId}>
        {children}
      </div>
    </details>
  );
}
