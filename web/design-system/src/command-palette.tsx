import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
} from "react";
import { createPortal } from "react-dom";
import { Icon, type IconName } from "./icons";

/** A product action that can be offered by a command palette. */
export interface CommandPaletteCommand {
  /** Stable action identifier owned by the consuming application. */
  id: string;
  /** Localized visible and accessible action name. */
  label: string;
  /** Optional localized explanation of what the action will do. */
  description?: string;
  /** Localized aliases used only for command filtering. */
  keywords?: readonly string[];
  /** Product-owned icon from the shared icon registry. */
  icon: IconName;
  /** A display-only shortcut hint; the application owns key handling. */
  shortcut?: string;
  /** Keep unavailable actions visible only when their state is useful to explain. */
  disabled?: boolean;
  /** Localized reason shown alongside a disabled action. */
  disabledReason?: string;
}

/**
 * Determines who owns focus after an enabled command closes the palette.
 * Commands that create a surface with its own deterministic focus target can
 * transfer focus; all other commands restore the palette opener.
 */
export type CommandPaletteFocusPolicy = "restore" | "transfer";

export interface CommandPaletteProps {
  open: boolean;
  title: string;
  searchLabel: string;
  searchPlaceholder: string;
  emptyQueryLabel: string;
  emptyState: string;
  noResults: string;
  closeHint: string;
  dismissLabel: string;
  commands: readonly CommandPaletteCommand[];
  /** Explicit opener captured by the consumer before it changes `open`. */
  returnFocusTo?: HTMLElement | null;
  /** Called for Escape, backdrop dismissal, and the optional close affordance. */
  onDismiss: () => void;
  /**
   * Called only for an enabled command. The application decides whether to
   * close first, then returns whether the command owns a new focus target.
   */
  onCommand: (command: CommandPaletteCommand) => CommandPaletteFocusPolicy | void;
}

interface AttributeSnapshot {
  element: HTMLElement;
  ariaHidden: string | null;
  inert: string | null;
}

const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "a[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function normalize(value: string) {
  return value.trim().toLocaleLowerCase();
}

function focusableElements(root: HTMLElement | null) {
  if (!root) return [];
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
    .filter((element) => !element.hasAttribute("hidden") && element.tabIndex >= 0);
}

function restoreAttribute(
  element: HTMLElement,
  name: "aria-hidden" | "inert",
  value: string | null,
) {
  if (value === null) element.removeAttribute(name);
  else element.setAttribute(name, value);
}

/**
 * An accessible, controlled command palette primitive. It intentionally owns
 * no product actions: callers provide the exact commands their current state
 * permits, while the primitive provides filtering, keyboard navigation,
 * modal focus trapping, and Escape focus restoration.
 */
export function CommandPalette({
  open,
  title,
  searchLabel,
  searchPlaceholder,
  emptyQueryLabel,
  emptyState,
  noResults,
  closeHint,
  dismissLabel,
  commands,
  returnFocusTo,
  onDismiss,
  onCommand,
}: CommandPaletteProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const commandRefs = useRef(new Map<string, HTMLButtonElement>());
  const previouslyFocused = useRef<HTMLElement | null>(null);
  const wasOpen = useRef(false);
  const restoreFocusOnClose = useRef(true);
  const backdropPointerRef = useRef<number | null>(null);
  const backdropClickArmedRef = useRef(false);
  const listId = useId();
  const titleId = useId();
  const [query, setQuery] = useState("");
  const [activeCommandId, setActiveCommandId] = useState<string>();

  const normalizedQuery = normalize(query);
  const filteredCommands = useMemo(() => commands.filter((command) => {
    if (!normalizedQuery) return true;
    return normalize([
      command.label,
      command.description ?? "",
      command.shortcut ?? "",
      ...(command.keywords ?? []),
    ].join(" ")).includes(normalizedQuery);
  }), [commands, normalizedQuery]);
  const firstEnabledCommand = filteredCommands.find((command) => !command.disabled);
  const activeCommand = filteredCommands.find((command) => command.id === activeCommandId
    && !command.disabled) ?? firstEnabledCommand;

  useLayoutEffect(() => {
    if (!open) return;
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
    const reclaimFocus = (event: FocusEvent) => {
      const dialog = dialogRef.current;
      if (event.target instanceof Node && dialog?.contains(event.target)) return;
      inputRef.current?.focus();
    };
    document.addEventListener("focusin", reclaimFocus, true);
    return () => {
      document.removeEventListener("focusin", reclaimFocus, true);
      for (const { element, ariaHidden, inert } of backgrounds) {
        restoreAttribute(element, "aria-hidden", ariaHidden);
        restoreAttribute(element, "inert", inert);
      }
    };
  }, [open]);

  useEffect(() => {
    if (open) {
      if (!wasOpen.current) {
        previouslyFocused.current = returnFocusTo ?? (document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null);
        setQuery("");
        window.requestAnimationFrame(() => inputRef.current?.focus());
      }
      wasOpen.current = true;
      return;
    }

    if (wasOpen.current && restoreFocusOnClose.current) {
      const returnTarget = previouslyFocused.current;
      window.requestAnimationFrame(() => returnTarget?.focus());
    }
    previouslyFocused.current = null;
    restoreFocusOnClose.current = true;
    wasOpen.current = false;
  }, [open, returnFocusTo]);

  useEffect(() => {
    if (!open) return;
    setActiveCommandId((current) => filteredCommands.some((command) => command.id === current
      && !command.disabled)
      ? current
      : firstEnabledCommand?.id);
  }, [filteredCommands, firstEnabledCommand?.id, open]);

  const dismiss = () => {
    restoreFocusOnClose.current = true;
    onDismiss();
  };

  const execute = (command: CommandPaletteCommand | undefined) => {
    if (!command || command.disabled) return;
    const focusPolicy = onCommand(command);
    restoreFocusOnClose.current = focusPolicy !== "transfer";
  };

  const moveActiveCommand = (direction: -1 | 1, moveFocus: boolean) => {
    const enabled = filteredCommands.filter((command) => !command.disabled);
    if (!enabled.length) return;
    const currentIndex = activeCommand
      ? enabled.findIndex((command) => command.id === activeCommand.id)
      : -1;
    const nextIndex = currentIndex < 0
      ? (direction === 1 ? 0 : enabled.length - 1)
      : (currentIndex + direction + enabled.length) % enabled.length;
    const next = enabled[nextIndex];
    if (!next) return;
    setActiveCommandId(next.id);
    if (moveFocus) {
      window.requestAnimationFrame(() => commandRefs.current.get(next.id)?.focus());
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      dismiss();
      return;
    }

    if (event.key === "Tab") {
      const focusable = focusableElements(dialogRef.current);
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable.at(-1);
      if (!first || !last) return;
      const focused = document.activeElement;
      if (event.shiftKey && (focused === first || !dialogRef.current?.contains(focused))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (focused === last || !dialogRef.current?.contains(focused))) {
        event.preventDefault();
        first.focus();
      }
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActiveCommand(1, event.target !== inputRef.current);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActiveCommand(-1, event.target !== inputRef.current);
      return;
    }
    if (event.key === "Enter" && event.target === inputRef.current) {
      event.preventDefault();
      execute(activeCommand);
    }
  };

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

  const dismissFromBackdrop = (event: MouseEvent<HTMLDivElement>) => {
    const directCompletedPress = event.target === event.currentTarget
      && backdropClickArmedRef.current;
    backdropClickArmedRef.current = false;
    if (directCompletedPress) dismiss();
  };

  if (!open) return null;

  return createPortal(
    <div
      className="command-palette-layer"
      onClick={dismissFromBackdrop}
      onPointerCancel={cancelBackdropPress}
      onPointerDown={beginBackdropPress}
      onPointerUp={finishBackdropPress}
    >
      <div
        ref={dialogRef}
        aria-labelledby={titleId}
        aria-modal="true"
        className="command-palette"
        onKeyDown={handleKeyDown}
        role="dialog"
      >
        <div className="command-palette__header">
          <div>
            <p className="command-palette__eyebrow">{emptyQueryLabel}</p>
            <h2 id={titleId}>{title}</h2>
          </div>
          <div className="command-palette__dismiss">
            <span className="command-palette__close-hint" aria-hidden="true">
              <kbd>Esc</kbd>
              {closeHint}
            </span>
            <button
              aria-label={dismissLabel}
              className="command-palette__close"
              title={dismissLabel}
              type="button"
              onClick={dismiss}
            >
              <Icon aria-hidden name="close" size={18} />
            </button>
          </div>
        </div>
        <label className="command-palette__search">
          <Icon aria-hidden name="search" size={18} />
          <span className="sr-only">{searchLabel}</span>
          <input
            ref={inputRef}
            aria-activedescendant={activeCommand ? `${listId}-${activeCommand.id}` : undefined}
            aria-controls={listId}
            aria-expanded="true"
            autoComplete="off"
            placeholder={searchPlaceholder}
            role="combobox"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        {filteredCommands.length ? (
          <div
            id={listId}
            aria-label={searchLabel}
            className="command-palette__results"
            role="listbox"
          >
            {filteredCommands.map((command) => {
            const isActive = activeCommand?.id === command.id;
            const optionId = `${listId}-${command.id}`;
            const descriptionId = command.disabledReason
              ? `${optionId}-reason`
              : command.description ? `${optionId}-description` : undefined;
            return (
              <button
                key={command.id}
                ref={(node) => {
                  if (node) commandRefs.current.set(command.id, node);
                  else commandRefs.current.delete(command.id);
                }}
                aria-describedby={descriptionId}
                aria-label={command.label}
                aria-selected={isActive}
                className="command-palette__command"
                disabled={command.disabled}
                id={optionId}
                role="option"
                tabIndex={isActive ? 0 : -1}
                onClick={() => execute(command)}
                onFocus={() => !command.disabled && setActiveCommandId(command.id)}
              >
                <Icon aria-hidden name={command.icon} size={19} />
                <span className="command-palette__command-copy">
                  <strong>{command.label}</strong>
                  {command.disabledReason ? (
                    <small id={`${optionId}-reason`}>{command.disabledReason}</small>
                  ) : command.description ? (
                    <small id={`${optionId}-description`}>{command.description}</small>
                  ) : null}
                </span>
                {command.shortcut ? <kbd className="command-palette__shortcut">{command.shortcut}</kbd> : null}
              </button>
            );
            })}
          </div>
        ) : (
          <p className="command-palette__state" role="status">
            {commands.length ? noResults : emptyState}
          </p>
        )}
      </div>
    </div>,
    document.body,
  );
}
