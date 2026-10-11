import {
  useId,
  useLayoutEffect,
  useRef,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { Icon } from "./icons";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not(:disabled)",
  "input:not(:disabled)",
  "select:not(:disabled)",
  "textarea:not(:disabled)",
  "summary",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

interface AttributeSnapshot {
  element: HTMLElement;
  ariaHidden: string | null;
  inert: string | null;
}

function focusableElements(dialog: HTMLElement): HTMLElement[] {
  return Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
    .filter((element) => {
      if (element.hidden || element.closest("[hidden]")) return false;
      if (element.closest("[aria-hidden='true'], [inert]")) return false;

      const closedDetails = element.closest("details:not([open])");
      return !closedDetails || element.tagName === "SUMMARY";
    });
}

function restoreAttribute(
  element: HTMLElement,
  name: "aria-hidden" | "inert",
  value: string | null,
) {
  if (value === null) element.removeAttribute(name);
  else element.setAttribute(name, value);
}

export interface DialogProps {
  /** Required localized title used as the accessible dialog name. */
  title: string;
  /** Required localized label for the close icon. */
  closeLabel: string;
  /** Called for the close icon, Escape, and a direct backdrop press. */
  onClose: () => void;
  children: ReactNode;
  /** Optional visible explanation, also exposed through aria-describedby. */
  description?: string;
  /** Override the normal first-control focus target for safety-sensitive flows. */
  initialFocusRef?: RefObject<HTMLElement | null>;
  /** Prevent every dismissal path while an irreversible request is in flight. */
  dismissDisabled?: boolean;
  /** Expose an in-flight operation to assistive technology. */
  busy?: boolean;
  /** Focus fallback used only when the original opener was removed. */
  returnFocusRef?: RefObject<HTMLElement | null>;
  /** Product layout hook; modal semantics and focus ownership remain shared. */
  className?: string;
  /** Product backdrop layout hook; visual isolation remains shared. */
  backdropClassName?: string;
  /** Use alertdialog only when the surface requires an immediate decision. */
  role?: "dialog" | "alertdialog";
  /** Omit the duplicate icon dismissal when explicit decision actions suffice. */
  showCloseButton?: boolean;
}

/**
 * Shared modal dialog primitive. It portals to the document body, isolates only
 * explicitly declared application backgrounds, traps/reclaims focus, and
 * restores both background attributes and opener focus exactly on unmount.
 */
export function Dialog({
  title,
  closeLabel,
  onClose,
  children,
  description,
  initialFocusRef,
  dismissDisabled = false,
  busy = false,
  returnFocusRef,
  className = "",
  backdropClassName = "",
  role = "dialog",
  showCloseButton = true,
}: DialogProps) {
  const headingId = useId();
  const descriptionId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const dismissDisabledRef = useRef(dismissDisabled);
  const initialFocusRefRef = useRef(initialFocusRef);
  const returnFocusRefRef = useRef(returnFocusRef);
  const backdropPointerRef = useRef<number | null>(null);
  const backdropClickArmedRef = useRef(false);
  onCloseRef.current = onClose;
  dismissDisabledRef.current = dismissDisabled;
  initialFocusRefRef.current = initialFocusRef;
  returnFocusRefRef.current = returnFocusRef;

  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    const activeBeforeOpen = document.activeElement;
    const previous = activeBeforeOpen instanceof HTMLElement
      && activeBeforeOpen !== document.body
      && activeBeforeOpen !== document.documentElement
      ? activeBeforeOpen
      : null;
    const backgrounds = Array.from(
      document.querySelectorAll<HTMLElement>("[data-dialog-background]"),
      (element): AttributeSnapshot => ({
        element,
        ariaHidden: element.getAttribute("aria-hidden"),
        inert: element.getAttribute("inert"),
      }),
    );
    for (const { element } of backgrounds) {
      element.setAttribute("inert", "");
      element.setAttribute("aria-hidden", "true");
    }

    const focusInitial = () => {
      const requested = initialFocusRefRef.current?.current;
      const target = requested && dialog.contains(requested) && !requested.matches(":disabled, [hidden], [aria-hidden='true']")
        ? requested
        : (focusableElements(dialog)[0] ?? dialog);
      target.focus();
    };
    focusInitial();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (!dismissDisabledRef.current) onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = focusableElements(dialog);
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      const activeIsFocusable = active instanceof HTMLElement && focusable.includes(active);
      if (event.shiftKey && (active === first || !activeIsFocusable)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !activeIsFocusable)) {
        event.preventDefault();
        first.focus();
      }
    };
    const onFocusIn = (event: FocusEvent) => {
      if (event.target instanceof Node && dialog.contains(event.target)) return;
      focusInitial();
    };

    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("focusin", onFocusIn, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("focusin", onFocusIn, true);
      for (const { element, ariaHidden, inert } of backgrounds) {
        restoreAttribute(element, "aria-hidden", ariaHidden);
        restoreAttribute(element, "inert", inert);
      }
      const fallbackRef = returnFocusRefRef.current;
      const fallback = fallbackRef?.current;
      const returnTarget = previous?.isConnected ? previous : fallback;
      if (returnTarget?.isConnected) returnTarget.focus();
      if (previous && fallbackRef) {
        queueMicrotask(() => {
          const deferredFallback = fallbackRef.current;
          const active = document.activeElement;
          if (
            !previous.isConnected
            && deferredFallback?.isConnected
            && (active === document.body || !active?.isConnected)
          ) deferredFallback.focus();
        });
      }
    };
  }, []);

  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (!busy || !dialog) return;

    const active = document.activeElement;
    const activeControl = active instanceof HTMLElement ? active : null;
    if (
      !activeControl
      || !dialog.contains(activeControl)
      || activeControl.matches(":disabled, [hidden], [aria-hidden='true']")
    ) {
      dialog.focus();
    }
  }, [busy]);

  const beginBackdropPress = (event: PointerEvent<HTMLDivElement>) => {
    backdropClickArmedRef.current = false;
    backdropPointerRef.current = event.target === event.currentTarget
      ? event.pointerId
      : null;
  };

  const finishBackdropPress = (event: PointerEvent<HTMLDivElement>) => {
    backdropClickArmedRef.current = event.target === event.currentTarget
      && backdropPointerRef.current === event.pointerId;
    backdropPointerRef.current = null;
  };

  const cancelBackdropPress = () => {
    backdropPointerRef.current = null;
    backdropClickArmedRef.current = false;
  };

  const closeFromBackdrop = (event: MouseEvent<HTMLDivElement>) => {
    const directCompletedPress = event.target === event.currentTarget
      && backdropClickArmedRef.current;
    backdropClickArmedRef.current = false;
    if (directCompletedPress && !dismissDisabledRef.current) {
      onCloseRef.current();
    }
  };

  return createPortal(
    <div
      className={`dialog-backdrop ${backdropClassName}`.trim()}
      onClick={closeFromBackdrop}
      onPointerCancel={cancelBackdropPress}
      onPointerDown={beginBackdropPress}
      onPointerUp={finishBackdropPress}
    >
      <div
        ref={dialogRef}
        aria-busy={busy || undefined}
        aria-describedby={description ? descriptionId : undefined}
        aria-labelledby={headingId}
        aria-modal="true"
        className={`dialog ${className}`.trim()}
        role={role}
        tabIndex={-1}
      >
        <div className="dialog__header">
          <h2 id={headingId}>{title}</h2>
          {showCloseButton ? (
            <button
              aria-label={closeLabel}
              className="icon-button"
              disabled={dismissDisabled}
              title={closeLabel}
              type="button"
              onClick={() => {
                if (!dismissDisabledRef.current) onCloseRef.current();
              }}
            >
              <Icon aria-hidden name="close" size={20} />
            </button>
          ) : null}
        </div>
        {description ? (
          <p className="dialog__description" id={descriptionId}>{description}</p>
        ) : null}
        {children}
      </div>
    </div>,
    document.body,
  );
}

export interface ConfirmDialogProps {
  /** Explicit localized name of the operation being confirmed. */
  title: string;
  /** Explicit localized consequence or prerequisite for the operation. */
  description: string;
  closeLabel: string;
  cancelLabel: string;
  confirmLabel: string;
  onCancel: () => void;
  onConfirm: () => void;
  children?: ReactNode;
  confirmVariant?: "primary" | "danger";
  /** Lock every dismissal and submission path while the operation is in flight. */
  busy?: boolean;
  /** Focus fallback used only when the original opener was removed. */
  returnFocusRef?: RefObject<HTMLElement | null>;
  /** Use alertdialog only when an immediate response is required. */
  role?: "dialog" | "alertdialog";
  /** Omit the icon dismissal when cancel already expresses the safe choice. */
  showCloseButton?: boolean;
}

/**
 * Confirmation surface for consequential actions. The non-destructive cancel
 * action always receives initial focus; confirmation is never implicit and is
 * never delegated to a browser-native confirmation prompt.
 */
export function ConfirmDialog({
  title,
  description,
  closeLabel,
  cancelLabel,
  confirmLabel,
  onCancel,
  onConfirm,
  children,
  confirmVariant = "danger",
  busy = false,
  returnFocusRef,
  role = "dialog",
  showCloseButton = true,
}: ConfirmDialogProps) {
  const cancelRef = useRef<HTMLButtonElement>(null);

  return (
    <Dialog
      closeLabel={closeLabel}
      description={description}
      dismissDisabled={busy}
      initialFocusRef={cancelRef}
      title={title}
      busy={busy}
      returnFocusRef={returnFocusRef}
      role={role}
      showCloseButton={showCloseButton}
      onClose={onCancel}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy) onConfirm();
        }}
      >
        {children ? <div className="dialog__body confirm-dialog__body">{children}</div> : null}
        <div className="dialog__actions">
          <button
            ref={cancelRef}
            className="button button--secondary"
            disabled={busy}
            type="button"
            onClick={() => {
              if (!busy) onCancel();
            }}
          >
            {cancelLabel}
          </button>
          <button
            className={`button button--${confirmVariant}`}
            disabled={busy}
            type="submit"
          >
            {confirmLabel}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
