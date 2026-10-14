// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import { IdentityCardMenu } from "./IdentityCardMenu";

const mailbox: MailboxDirectoryItem = {
  mailbox_id: "019b0000-0000-7000-8000-000000000020",
  kind: "human",
  primary_address: "alex.chen@copilotmail.test",
  can_send: true,
};

function renderMenu(overrides: Partial<React.ComponentProps<typeof IdentityCardMenu>> = {}) {
  const onOpenSettings = vi.fn();
  const onSignOut = vi.fn();
  render(
    <IdentityCardMenu
      activeMailbox={mailbox}
      t={createTranslator("en")}
      onOpenSettings={onOpenSettings}
      onSignOut={onSignOut}
      {...overrides}
    />,
  );
  return { onOpenSettings, onSignOut };
}

afterEach(cleanup);

describe("identity card account menu", () => {
  it("shows the display name and full address, and opens a named menu", () => {
    renderMenu();
    const card = screen.getByRole("button", { name: /alex\.chen alex\.chen@copilotmail\.test/ });
    expect(card).toHaveAttribute("aria-haspopup", "menu");
    expect(card).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(card);
    expect(card).toHaveAttribute("aria-expanded", "true");
    const menu = screen.getByRole("menu", { name: "Account" });
    expect(card).toHaveAttribute("aria-controls", menu.id);
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent))
      .toEqual(["Settings", "Sign out"]);
    // The first item receives focus when the menu opens.
    expect(screen.getByRole("menuitem", { name: "Settings" })).toHaveFocus();
  });

  it("moves focus with arrows, wraps around, and Escape restores the card", () => {
    renderMenu();
    const card = screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ });
    fireEvent.keyDown(card, { key: "ArrowDown" });
    const settings = screen.getByRole("menuitem", { name: "Settings" });
    const signOut = screen.getByRole("menuitem", { name: "Sign out" });
    expect(settings).toHaveFocus();

    fireEvent.keyDown(settings, { key: "ArrowDown" });
    expect(signOut).toHaveFocus();
    fireEvent.keyDown(signOut, { key: "ArrowDown" });
    expect(settings).toHaveFocus();
    fireEvent.keyDown(settings, { key: "ArrowUp" });
    expect(signOut).toHaveFocus();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(card).toHaveFocus();
    expect(card).toHaveAttribute("aria-expanded", "false");
  });

  it("dismisses on an outside press and reopens", () => {
    renderMenu();
    const card = screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ });
    fireEvent.click(card);
    expect(screen.getByRole("menu")).toBeInTheDocument();

    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    fireEvent.click(card);
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("invokes the scoped action with the card as the return-focus element", () => {
    const { onOpenSettings, onSignOut } = renderMenu();
    const card = screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ });

    fireEvent.click(card);
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(onOpenSettings).toHaveBeenCalledWith(card);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    fireEvent.click(card);
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    expect(onSignOut).toHaveBeenCalledWith(card);
  });

  it("locks the card while the session is being revalidated", () => {
    renderMenu({ sessionLocked: true });
    expect(screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ })).toBeDisabled();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
