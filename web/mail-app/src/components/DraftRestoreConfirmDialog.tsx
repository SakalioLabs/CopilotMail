import { useId, useLayoutEffect, useRef } from "react";
import type { Translator } from "../i18n";

interface AttributeSnapshot {
  element: HTMLElement;
  ariaHidden: string | null;
  inert: string | null;
}

export interface DraftRestoreConfirmDialogProps {
  busy?: boolean;
  t: Translator;
  onCancel: () => void;
  onConfirm: () => void;
}

export function DraftRestoreConfirmDialog({
  busy = false,
  t,
  onCancel,
  onConfirm,
}: DraftRestoreConfirmDialogProps) {
  const titleId = `${useId()}-draft-confirm-title`;
  const descriptionId = `${useId()}-draft-confirm-description`;
  const dialogRef = useRef<HTMLElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const onCancelRef = useRef(onCancel);
  const busyRef = useRef(busy);
  onCancelRef.current = onCancel;
  busyRef.current = busy;

  useLayoutEffect(() => {
    if (busy) dialogRef.current?.focus();
  }, [busy]);

  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const previouslyFocused = document.activeElement instanceof HTMLElement
      ? document.activeElement
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
    cancelRef.current?.focus();

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (busyRef.current) return;
        event.preventDefault();
        onCancelRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      if (busyRef.current) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const buttons = Array.from(dialog.querySelectorAll<HTMLButtonElement>("button"));
      const first = buttons[0];
      const last = buttons.at(-1);
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown, true);
    return () => {
      document.removeEventListener("keydown", handleKeyDown, true);
      for (const { element, ariaHidden, inert } of backgrounds) {
        if (ariaHidden === null) element.removeAttribute("aria-hidden");
        else element.setAttribute("aria-hidden", ariaHidden);
        if (inert === null) element.removeAttribute("inert");
        else element.setAttribute("inert", inert);
      }
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, []);

  return (
    <div className="confirm-layer">
      <section
        ref={dialogRef}
        className="confirm-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        aria-busy={busy || undefined}
        tabIndex={-1}
      >
        <h2 id={titleId}>{t("drafts.confirmTitle")}</h2>
        <p id={descriptionId}>{t("drafts.confirmDescription")}</p>
        <div>
          <button ref={cancelRef} type="button" disabled={busy} onClick={onCancel}>
            {t("drafts.confirmCancel")}
          </button>
          <button className="confirm-primary" type="button" disabled={busy} onClick={onConfirm}>
            {busy ? t("drafts.restoring") : t("drafts.confirmAction")}
          </button>
        </div>
      </section>
    </div>
  );
}
