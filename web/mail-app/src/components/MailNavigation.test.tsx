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
  it("keeps the selected Human identity and server sendability visible on desktop even while compose is busy", () => {
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

    const identity = screen.getByRole("region", { name: "Current mailbox identity" });
    expect(identity).toHaveTextContent("Personal");
    expect(within(identity).getByText("reader@example.test")).toBeInTheDocument();
    expect(within(identity).getByText("Server status")).toBeInTheDocument();
    expect(within(identity).getByText("Can send email")).toBeInTheDocument();
    expect(identity.querySelector("[data-sendable='true']")).toHaveClass(
      "status-indicator--positive",
    );
    expect(screen.getByRole("combobox", { name: "Switch mailbox" })).toHaveValue(
      mailbox.mailbox_id,
    );
    expect(screen.getByRole("button", { name: "Compose" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Compose" })).toHaveClass("compose-fab");
    const card = screen.getByRole("button", { name: /reader@example\.test/ });
    expect(card).toHaveClass("identity-card");
    expect(card).toHaveAttribute("aria-haspopup", "menu");
    fireEvent.click(card);
    const menu = screen.getByRole("menu", { name: "Account" });
    expect(within(menu).getByRole("menuitem", { name: "Settings" })).toBeEnabled();
    expect(within(menu).getByRole("menuitem", { name: "Sign out" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Inbox" }).querySelector("[data-icon='inbox']"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sent" }).querySelector("[data-icon='sent']"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Drafts" }).querySelector("[data-icon='draft']"))
      .toBeInTheDocument();
  });

  it("keeps factual mobile scope visible while low-frequency controls are closed by default", () => {
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
    const scope = drawer.querySelector<HTMLDetailsElement>(".mobile-mailbox-scope");
    const summary = scope?.querySelector<HTMLElement>("summary");
    expect(scope).not.toBeNull();
    expect(scope).not.toHaveAttribute("open");
    expect(summary).toHaveTextContent("triage-agent@example.test");
    expect(summary).toHaveTextContent("Agent");
    expect(summary).toHaveTextContent("This mailbox is read-only");
    expect(summary?.querySelector("[data-sendable='false']")).toHaveClass(
      "status-indicator--warning",
    );

    fireEvent.click(summary!);
    expect(scope).toHaveAttribute("open");

    const identity = scope?.querySelector<HTMLElement>(".mobile-mailbox-identity");
    expect(identity).toHaveTextContent("Current mailbox identity");
    expect(identity).toHaveTextContent("Agent");
    expect(identity).toHaveTextContent("triage-agent@example.test");
    expect(identity).toHaveTextContent("Server status");
    expect(identity?.querySelector("[data-sendable='false']")).toHaveClass(
      "status-indicator--warning",
    );
    expect(within(scope!).getByRole("combobox", { name: "Switch mailbox" })).toHaveValue(
      mailbox.mailbox_id,
    );
    expect(within(scope!).queryByRole("combobox", { name: "Language" })).not.toBeInTheDocument();
    expect(within(scope!).queryByRole("button", { name: "Sign out" })).not.toBeInTheDocument();
    const card = within(drawer).getByRole("button", { name: /triage-agent@example\.test/ });
    expect(card).toHaveClass("identity-card");
    fireEvent.click(card);
    expect(within(drawer).getByRole("menu", { name: "Account" })).toBeInTheDocument();
    expect(within(drawer).queryByRole("button", { name: "Compose" })).not.toBeInTheDocument();
    expect(document.querySelector<HTMLButtonElement>(".compose-fab")).toBeDisabled();
    expect(within(drawer).getByRole("button", { name: "Inbox" }).querySelector("[data-icon='inbox']"))
      .toBeInTheDocument();
    expect(within(drawer).getByRole("button", { name: "Sent" }).querySelector("[data-icon='sent']"))
      .toBeInTheDocument();
    expect(within(drawer).getByRole("button", { name: "Drafts" }).querySelector("[data-icon='draft']"))
      .toBeInTheDocument();
  });

  it("locks only the mailbox switch in an expanded mobile scope", () => {
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
    const scope = drawer.querySelector<HTMLDetailsElement>(".mobile-mailbox-scope");
    fireEvent.click(scope?.querySelector("summary")!);

    expect(scope).toHaveAttribute("open");
    expect(within(scope!).getByRole("combobox", { name: "Switch mailbox" })).toBeDisabled();
    expect(within(drawer).getByRole("button", { name: /writer@example\.test/ })).toBeDisabled();
  });
});
