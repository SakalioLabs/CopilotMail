// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
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
  const onMailboxChange = vi.fn();
  const onMailboxLoadMore = vi.fn();
  render(
    <IdentityCardMenu
      activeMailbox={mailbox}
      mailboxes={[mailbox]}
      mailboxHasMore={false}
      mailboxLoadMoreBusy={false}
      t={createTranslator("en")}
      onMailboxChange={onMailboxChange}
      onMailboxLoadMore={onMailboxLoadMore}
      onOpenSettings={onOpenSettings}
      onSignOut={onSignOut}
      {...overrides}
    />,
  );
  return { onOpenSettings, onSignOut, onMailboxChange, onMailboxLoadMore };
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
    expect(within(menu).getAllByRole("menuitem").map((item) => item.textContent))
      .toEqual(["Settings", "Sign out"]);
    // The first focusable row is the active mailbox radio.
    expect(within(menu).getAllByRole("menuitemradio")[0]).toHaveFocus();
    expect(within(menu).getAllByRole("menuitemradio"))
      .toHaveLength(1);
  });

  it("moves focus with arrows, wraps around, and Escape restores the card", () => {
    renderMenu();
    const card = screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ });
    fireEvent.keyDown(card, { key: "ArrowDown" });
    const menu = screen.getByRole("menu", { name: "Account" });
    const mailboxItem = within(menu).getByRole("menuitemradio", {
      name: "alex.chen@copilotmail.test",
    });
    const settings = within(menu).getByRole("menuitem", { name: "Settings" });
    const signOut = within(menu).getByRole("menuitem", { name: "Sign out" });
    expect(mailboxItem).toHaveFocus();

    fireEvent.keyDown(mailboxItem, { key: "ArrowDown" });
    expect(settings).toHaveFocus();
    fireEvent.keyDown(settings, { key: "ArrowDown" });
    expect(signOut).toHaveFocus();
    fireEvent.keyDown(signOut, { key: "ArrowDown" });
    expect(mailboxItem).toHaveFocus();
    fireEvent.keyDown(mailboxItem, { key: "ArrowUp" });
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

describe("identity card menu mailbox switching", () => {
  const directory: MailboxDirectoryItem[] = [
    {
      mailbox_id: "018f0000-0000-7000-8000-000000000003",
      kind: "shared",
      primary_address: "operations@example.test",
      can_send: true,
    },
    {
      mailbox_id: "018f0000-0000-7000-8000-000000000004",
      kind: "group",
      primary_address: "risk-review@example.test",
      can_send: false,
    },
    mailbox,
    {
      mailbox_id: "018f0000-0000-7000-8000-000000000005",
      kind: "agent",
      primary_address: "triage-agent@example.test",
      can_send: true,
    },
    {
      mailbox_id: "018f0000-0000-7000-8000-000000000002",
      kind: "shared",
      primary_address: "support@example.test",
      can_send: false,
    },
  ];

  it("groups the directory by kind, marks the active mailbox, and binds switching", () => {
    const { onMailboxChange } = renderMenu({ mailboxes: directory });
    const card = screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ });
    fireEvent.click(card);

    const menu = screen.getByRole("menu", { name: "Account" });
    const section = within(menu).getByRole("group", { name: "Switch mailbox" });
    // The active mailbox's kind and server sendability stay discoverable in
    // the menu header instead of a permanent rail section.
    expect(within(section).getByText("Personal")).toBeInTheDocument();
    expect(within(section).getByText("Can send email")).toBeInTheDocument();
    const kindGroups = within(section).getAllByRole("group");
    expect(kindGroups.map((group) => group.getAttribute("aria-label"))).toEqual([
      "Human mailboxes",
      "Agent mailboxes",
      "Shared mailboxes",
      "Group mailboxes",
    ]);
    const shared = kindGroups[2];
    expect(within(shared).getAllByRole("menuitemradio").map((item) => item.textContent))
      .toEqual(["operations@example.test", "support@example.test"]);
    expect(within(section).getByRole("menuitemradio", { name: "alex.chen@copilotmail.test" }))
      .toHaveAttribute("aria-checked", "true");
    expect(within(section).getByRole("menuitemradio", { name: "support@example.test" }))
      .toHaveAttribute("aria-checked", "false");

    fireEvent.click(within(section).getByRole("menuitemradio", { name: "support@example.test" }));
    expect(onMailboxChange).toHaveBeenCalledWith("018f0000-0000-7000-8000-000000000002");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(card).toHaveFocus();
  });

  it("keeps pagination reachable and reports its error inside the menu", () => {
    const { onMailboxLoadMore } = renderMenu({
      mailboxes: directory,
      mailboxHasMore: true,
      mailboxLoadMoreBusy: false,
      mailboxLoadMoreError: "directory page failed",
    });
    fireEvent.click(screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ }));

    const menu = screen.getByRole("menu", { name: "Account" });
    const loadMore = within(menu).getByRole("menuitem", { name: "Load more mailboxes" });
    fireEvent.click(loadMore);
    // Paging keeps the menu open so the merged page can be picked next.
    expect(onMailboxLoadMore).toHaveBeenCalledTimes(1);
    expect(menu).toBeInTheDocument();
    expect(within(menu).getByRole("alert")).toHaveTextContent("directory page failed");
  });

  it("shows the in-flight pagination state and disables it", () => {
    renderMenu({ mailboxes: directory, mailboxHasMore: true, mailboxLoadMoreBusy: true });
    fireEvent.click(screen.getByRole("button", { name: /alex\.chen@copilotmail\.test/ }));
    expect(screen.getByRole("menuitem", { name: "Loading more mailboxes…" })).toBeDisabled();
  });
});
