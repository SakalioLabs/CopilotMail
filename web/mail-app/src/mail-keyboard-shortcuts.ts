import { useEffect, useRef } from "react";

export type ShortcutAction = () => boolean;
export type MoveMessageAction = (direction: -1 | 1) => boolean;

export interface MailKeyboardShortcutBindings {
  enabled: boolean;
  modalOpen: boolean;
  onOpenCommandPalette: ShortcutAction;
  onCompose: ShortcutAction;
  onFocusInboxSearch: ShortcutAction;
  onMoveMessage: MoveMessageAction;
  onOpenSelectedMessage: ShortcutAction;
  onCloseSelectedMessage: ShortcutAction;
  onReply: ShortcutAction;
  onToggleFlag: ShortcutAction;
  onViewDrafts: ShortcutAction;
  onViewInbox: ShortcutAction;
  onViewSent: ShortcutAction;
}

const GO_SHORTCUT_TIMEOUT_MS = 750;

function isEditableTarget(target: EventTarget | null) {
  if (!(target instanceof Element)) return false;
  return target.closest(
    "input, textarea, select, [contenteditable='true'], [role='textbox']",
  ) !== null;
}

/**
 * Installs the application's global, keyboard-first mail navigation.
 *
 * The hook deliberately does not execute shortcuts while a modal is open or
 * while the user is entering text. Security-sensitive actions remain behind
 * the caller's permission and submission-state checks.
 */
export function useMailKeyboardShortcuts(bindings: MailKeyboardShortcutBindings) {
  const bindingsRef = useRef(bindings);
  const pendingGoShortcut = useRef<number | undefined>(undefined);
  bindingsRef.current = bindings;

  useEffect(() => {
    const clearGoShortcut = () => {
      if (pendingGoShortcut.current !== undefined) {
        window.clearTimeout(pendingGoShortcut.current);
        pendingGoShortcut.current = undefined;
      }
    };

    const run = (action: ShortcutAction, event: KeyboardEvent) => {
      if (action()) event.preventDefault();
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      const current = bindingsRef.current;
      if (
        !current.enabled
        || current.modalOpen
        || event.isComposing
      ) {
        return;
      }

      // The command palette deliberately remains available from a focused
      // search field. It is the only modified shortcut the mail shell owns.
      if (
        (event.ctrlKey || event.metaKey)
        && !event.altKey
        && !event.shiftKey
        && event.key.toLowerCase() === "k"
      ) {
        run(current.onOpenCommandPalette, event);
        return;
      }

      if (
        event.altKey
        || event.ctrlKey
        || event.metaKey
        || event.shiftKey
        || isEditableTarget(event.target)
      ) {
        return;
      }

      const key = event.key.toLowerCase();
      const hasPendingGoShortcut = pendingGoShortcut.current !== undefined;
      if (hasPendingGoShortcut) {
        clearGoShortcut();
        if (key === "i") {
          run(current.onViewInbox, event);
          return;
        }
        if (key === "d") {
          run(current.onViewDrafts, event);
          return;
        }
        if (key === "s") {
          run(current.onViewSent, event);
          return;
        }
      }

      if (key === "g") {
        if (event.repeat) return;
        clearGoShortcut();
        pendingGoShortcut.current = window.setTimeout(clearGoShortcut, GO_SHORTCUT_TIMEOUT_MS);
        event.preventDefault();
        return;
      }

      if (event.repeat && key !== "j" && key !== "k") return;

      switch (key) {
        case "c":
          run(current.onCompose, event);
          break;
        case "/":
          run(current.onFocusInboxSearch, event);
          break;
        case "j":
          if (current.onMoveMessage(1)) event.preventDefault();
          break;
        case "k":
          if (current.onMoveMessage(-1)) event.preventDefault();
          break;
        case "enter":
          run(current.onOpenSelectedMessage, event);
          break;
        case "escape":
          run(current.onCloseSelectedMessage, event);
          break;
        case "r":
          run(current.onReply, event);
          break;
        case "s":
          run(current.onToggleFlag, event);
          break;
        default:
          break;
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      clearGoShortcut();
    };
  }, []);
}
