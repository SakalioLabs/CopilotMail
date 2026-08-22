import { useId, useLayoutEffect, useRef, type ChangeEvent } from "react";
import type { Translator } from "../i18n";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
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
      if (element.getAttribute("aria-hidden") === "true") return false;

      const closedDetails = element.closest("details:not([open])");
      return !closedDetails || element.tagName === "SUMMARY";
    });
}

function restoreAttribute(
  element: HTMLElement,
  name: "aria-hidden" | "inert",
  value: string | null,
) {
  if (value === null) {
    element.removeAttribute(name);
  } else {
    element.setAttribute(name, value);
  }
}

const hasRecipient = (value: string) =>
  value.split(/[;,]/).some((recipient) => recipient.trim().length > 0);

export interface ComposerDialogProps {
  t: Translator;
  replyMode: boolean;
  restoredMode?: boolean;
  initialFocus?: "recipient" | "body";
  fromAddress: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  textBody: string;
  draftStatus: string;
  submissionLocked: boolean;
  sendBusy: boolean;
  terminalSubmission: boolean;
  onToChange: (value: string) => void;
  onCcChange: (value: string) => void;
  onBccChange: (value: string) => void;
  onSubjectChange: (value: string) => void;
  onTextBodyChange: (value: string) => void;
  onClose: () => void;
  onSend: () => void | Promise<void>;
}

export function ComposerDialog({
  t,
  replyMode,
  restoredMode = false,
  initialFocus = "recipient",
  fromAddress,
  to,
  cc,
  bcc,
  subject,
  textBody,
  draftStatus,
  submissionLocked,
  sendBusy,
  terminalSubmission,
  onToChange,
  onCcChange,
  onBccChange,
  onSubjectChange,
  onTextBodyChange,
  onClose,
  onSend,
}: ComposerDialogProps) {
  const titleId = `${useId()}-compose-title`;
  const dialogRef = useRef<HTMLElement>(null);
  const recipientRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const onCloseRef = useRef(onClose);
  const sendBusyRef = useRef(sendBusy);
  onCloseRef.current = onClose;
  sendBusyRef.current = sendBusy;

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
    const preferred = initialFocus === "body" ? bodyRef.current : recipientRef.current;
    if (preferred && !preferred.disabled) {
      preferred.focus();
    } else {
      (focusableElements(dialog)[0] ?? dialog).focus();
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (sendBusyRef.current) return;
        event.preventDefault();
        event.stopPropagation();
        onCloseRef.current();
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
      if (event.shiftKey && (active === first || !dialog.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !dialog.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", handleKeyDown, true);
    return () => {
      document.removeEventListener("keydown", handleKeyDown, true);
      for (const { element, ariaHidden, inert } of backgrounds) {
        restoreAttribute(element, "aria-hidden", ariaHidden);
        restoreAttribute(element, "inert", inert);
      }
      if (previouslyFocused?.isConnected) {
        previouslyFocused.focus();
      }
    };
  }, [initialFocus]);

  const valueFrom = (callback: (value: string) => void) =>
    (event: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      callback(event.target.value);
    };

  return (
    <section
      ref={dialogRef}
      className="composer"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      tabIndex={-1}
    >
      <header>
        <h2 id={titleId}>
          {t(restoredMode
            ? "compose.editDraftTitle"
            : replyMode
              ? "compose.replyTitle"
              : "compose.newTitle")}
        </h2>
        <button
          type="button"
          disabled={sendBusy}
          aria-label={t("compose.closeAria")}
          onClick={onClose}
        >
          {t("icon.close")}
        </button>
      </header>
      {replyMode && <p className="reply-context">{t("compose.replyContext")}</p>}
      <label>
        {t("compose.from")}
        <input
          aria-readonly="true"
          readOnly
          value={fromAddress}
        />
      </label>
      <label>
        {t("compose.to")}
        <input
          ref={recipientRef}
          disabled={submissionLocked}
          value={to}
          onChange={valueFrom(onToChange)}
          placeholder={t("compose.toPlaceholder")}
        />
      </label>
      <details>
        <summary>{t("compose.ccBcc")}</summary>
        <label>
          {t("compose.cc")}
          <input
            disabled={submissionLocked}
            value={cc}
            onChange={valueFrom(onCcChange)}
          />
        </label>
        <label>
          {t("compose.bcc")}
          <input
            disabled={submissionLocked}
            value={bcc}
            onChange={valueFrom(onBccChange)}
          />
        </label>
      </details>
      <label>
        {t("compose.subject")}
        <input
          disabled={submissionLocked}
          value={subject}
          maxLength={998}
          onChange={valueFrom(onSubjectChange)}
        />
      </label>
      <label className="body-label">
        <span className="sr-only">{t("compose.body")}</span>
        <textarea
          ref={bodyRef}
          disabled={submissionLocked}
          value={textBody}
          maxLength={262_144}
          onChange={valueFrom(onTextBodyChange)}
          placeholder={t("compose.bodyPlaceholder")}
        />
      </label>
      <footer>
        <button
          type="button"
          disabled={sendBusy || terminalSubmission || !hasRecipient(to)}
          onClick={() => {
            void onSend();
          }}
        >
          {sendBusy
            ? t("compose.sending")
            : submissionLocked
              ? t("compose.retrySend")
              : t("compose.send")}
        </button>
        <span role="status">{draftStatus}</span>
      </footer>
    </section>
  );
}
