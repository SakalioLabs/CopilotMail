// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import { DesktopNavigation, MobileNavigation } from "./MailNavigation";

afterEach(cleanup);

function navigationActions() {
  return {
    onCompose: vi.fn(),
    onMailboxChange: vi.fn(),
    onMailboxLoadMore: vi.fn(),
    onOpenCommandPalette: vi.fn(),
    onOpenSettings: vi.fn(),
    onSignOut: vi.fn(),
    onViewChange: vi.fn(),
  };
}

function commonProps(mailbox: MailboxDirectoryItem) {
  return {
    ...navigationActions(),
    activeMailbox: mailbox,
    activeView: "inbox" as const,
    locale: "en" as const,
    mailboxHasMore: false,
    mailboxLoadMoreBusy: false,
    mailboxes: [mailbox],
    sessionLocked: false,
    t: createTranslator("en"),
  };
}

describe("mail navigation mailbox identity", () => {
  it("keeps the selected Human identity and server sendability inside the account menu on desktop", () => {
    const mailbox: MailboxDirectoryItem = {
      mailbox_id: "019b0000-0000-7000-8000-000000000020",
      kind: "human",
      primary_address: "reader@example.test",
      can_send: true,
    };

    render(
      <>
        <DesktopNavigation
          {...commonProps(mailbox)}
          canCompose
          composeBusy
        />
        <MobileNavigation
          {...commonProps(mailbox)}
          canCompose
          composeBusy
          drawerOpen={false}
          onDrawerClose={vi.fn()}
        />
      </>,
    );

    // The rail no longer carries a permanent mailbox section; the directory
    // facts moved into the identity card's account menu.
    const sidebar = document.querySelector<HTMLElement>(".sidebar")!;
    expect(sidebar.querySelector(".mailbox-identity")).not.toBeInTheDocument();
    expect(within(sidebar).queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Compose" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Compose" })).toHaveClass("compose-fab");
    const card = screen.getByRole("button", { name: /reader@example\.test/ });
    expect(card).toHaveClass("identity-card");
    expect(card).toHaveAttribute("aria-haspopup", "menu");
    fireEvent.click(card);
    const menu = screen.getByRole("menu", { name: "Account" });
    const section = within(menu).getByRole("group", { name: "Switch mailbox" });
    expect(within(section).getByText("Personal")).toBeInTheDocument();
    expect(within(section).getByText("Can send email")).toBeInTheDocument();
    expect(within(section).getByRole("menuitemradio", { name: "reader@example.test" }))
      .toHaveAttribute("aria-checked", "true");
    expect(within(menu).getByRole("menuitem", { name: "Settings" })).toBeEnabled();
    expect(within(menu).getByRole("menuitem", { name: "Sign out" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Inbox" }).querySelector("[data-icon='inbox']"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sent" }).querySelector("[data-icon='sent']"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Drafts" }).querySelector("[data-icon='draft']"))
      .toBeInTheDocument();
  });

  it("keeps the drawer factual through the identity card menu without a scope disclosure", () => {
    const mailbox: MailboxDirectoryItem = {
      mailbox_id: "019b0000-0000-7000-8000-000000000021",
      kind: "agent",
      primary_address: "triage-agent@example.test",
      can_send: false,
    };

    render(
      <MobileNavigation
        {...commonProps(mailbox)}
        canCompose={false}
        drawerOpen
        onDrawerClose={vi.fn()}
      />,
    );

    const drawer = screen.getByRole("dialog", { name: "Mail navigation" });
    expect(drawer.querySelector(".mobile-mailbox-scope")).not.toBeInTheDocument();
    expect(within(drawer).queryByRole("combobox")).not.toBeInTheDocument();
    const card = within(drawer).getByRole("button", { name: /triage-agent@example\.test/ });
    expect(card).toHaveClass("identity-card");
    fireEvent.click(card);
    const menu = within(drawer).getByRole("menu", { name: "Account" });
    const section = within(menu).getByRole("group", { name: "Switch mailbox" });
    expect(within(section).getByText("Agent")).toBeInTheDocument();
    expect(within(section).getByText("This mailbox is read-only")).toBeInTheDocument();
    expect(section.querySelector("[data-sendable='false']")).toHaveClass(
      "status-indicator--warning",
    );
    expect(within(section).getByRole("menuitemradio", { name: "triage-agent@example.test" }))
      .toHaveAttribute("aria-checked", "true");
    expect(within(drawer).queryByRole("combobox", { name: "Language" })).not.toBeInTheDocument();
    expect(within(drawer).queryByRole("button", { name: "Compose" })).not.toBeInTheDocument();
    expect(document.querySelector<HTMLButtonElement>(".compose-fab")).toBeDisabled();
    expect(within(drawer).getByRole("button", { name: "Inbox" }).querySelector("[data-icon='inbox']"))
      .toBeInTheDocument();
    expect(within(drawer).getByRole("button", { name: "Sent" }).querySelector("[data-icon='sent']"))
      .toBeInTheDocument();
    expect(within(drawer).getByRole("button", { name: "Drafts" }).querySelector("[data-icon='draft']"))
      .toBeInTheDocument();
  });

  it("locks mailbox switching behind the card while the session is revalidated", () => {
    const mailbox: MailboxDirectoryItem = {
      mailbox_id: "019b0000-0000-7000-8000-000000000022",
      kind: "human",
      primary_address: "writer@example.test",
      can_send: true,
    };

    render(
      <MobileNavigation
        {...commonProps(mailbox)}
        canCompose
        sessionLocked
        drawerOpen
        onDrawerClose={vi.fn()}
      />,
    );

    const drawer = screen.getByRole("dialog", { name: "Mail navigation" });
    expect(within(drawer).getByRole("button", { name: /writer@example\.test/ }))
      .toBeDisabled();
  });
});
