import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { Icon } from "@copilotmail/design-system";
import type { Translator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";

interface IdentityCardMenuProps {
  activeMailbox: MailboxDirectoryItem;
  sessionLocked?: boolean;
  t: Translator;
  onOpenSettings: (trigger: HTMLElement | null) => void;
  onSignOut: (trigger: HTMLElement | null) => void;
}

function menuItems(menu: HTMLElement | null) {
  return Array.from(
    menu?.querySelectorAll<HTMLElement>("[role='menuitem']") ?? [],
  );
}

/**
 * Account identity card at the bottom of the navigation rail with the
 * Apple-style popover menu. The mailbox directory remains the only identity
 * authority: the display name is the primary address local part and the
 * address itself is rendered verbatim. The popover is a real `menu` pattern —
 * Enter/Space opens it, arrows cycle items, Escape closes and returns focus,
 * and an outside press dismisses it.
 */
export function IdentityCardMenu({
  activeMailbox,
  sessionLocked = false,
  t,
  onOpenSettings,
  onSignOut,
}: IdentityCardMenuProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const initialFocus = useRef<"first" | "last">("first");
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const address = activeMailbox.primary_address;
  const displayName = address.split("@", 1)[0] || address;

  const closeMenu = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    const items = menuItems(menuRef.current);
    const target = initialFocus.current === "last" ? items.at(-1) : items[0];
    target?.focus();

    const onPointerDown = (event: PointerEvent) => {
      if (
        event.target instanceof Node
        && !rootRef.current?.contains(event.target)
      ) {
        setOpen(false);
      }
    };
    const onEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeMenu(true);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onEscape, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onEscape, true);
    };
  }, [open]);

  const onTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      initialFocus.current = "first";
      setOpen(true);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      initialFocus.current = "last";
      setOpen(true);
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Tab") {
      setOpen(false);
      return;
    }
    const items = menuItems(menuRef.current);
    if (items.length === 0) return;
    const index = items.indexOf(document.activeElement as HTMLElement);
    let next: number | undefined;
    if (event.key === "ArrowDown") {
      next = index < 0 ? 0 : (index + 1) % items.length;
    } else if (event.key === "ArrowUp") {
      next = index < 0 ? items.length - 1 : (index - 1 + items.length) % items.length;
    } else if (event.key === "Home") {
      next = 0;
    } else if (event.key === "End") {
      next = items.length - 1;
    }
    if (next === undefined) return;
    event.preventDefault();
    items[next].focus();
  };

  const activate = (action: (trigger: HTMLElement | null) => void) => {
    const trigger = triggerRef.current;
    setOpen(false);
    action(trigger);
  };

  return (
    <div ref={rootRef} className="identity-menu-wrap">
      <button
        ref={triggerRef}
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        aria-haspopup="menu"
        className="identity-card"
        disabled={sessionLocked}
        type="button"
        onClick={() => {
          initialFocus.current = "first";
          setOpen((current) => !current);
        }}
        onKeyDown={onTriggerKeyDown}
      >
        <span aria-hidden="true" className="identity-card__avatar">
          {displayName.slice(0, 1).toUpperCase()}
        </span>
        <span className="identity-card__text">
          <b className="identity-card__name">{displayName}</b>
          <span className="identity-card__email" title={address}>{address}</span>
        </span>
        <Icon
          aria-hidden
          className="identity-card__chevron"
          name="chevron-down"
          size={16}
        />
      </button>
      {open ? (
        <div
          ref={menuRef}
          aria-label={t("identity.menuLabel")}
          aria-orientation="vertical"
          className="identity-menu"
          id={menuId}
          role="menu"
          onKeyDown={onMenuKeyDown}
        >
          <button
            className="identity-menu__item"
            role="menuitem"
            type="button"
            onClick={() => activate(onOpenSettings)}
          >
            {t("identity.settings")}
          </button>
          <button
            className="identity-menu__item"
            role="menuitem"
            type="button"
            onClick={() => activate(onSignOut)}
          >
            {t("nav.signOut")}
          </button>
        </div>
      ) : null}
    </div>
  );
}
