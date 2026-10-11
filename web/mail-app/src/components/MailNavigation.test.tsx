// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import type { MailboxDirectoryItem } from "../types";
import { DesktopNavigation, MobileToolbar } from "./MailNavigation";

afterEach(cleanup);

function navigationActions() {
  return {
    onCompose: vi.fn(),
    onLocaleChange: vi.fn(),
    onMailboxChange: vi.fn(),
    onMailboxLoadMore: vi.fn(),
    onOpenCommandPalette: vi.fn(),
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
      <DesktopNavigation
        {...commonProps(mailbox)}
        canCompose
        composeBusy
      />,
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
    expect(screen.getByRole("button", { name: "Compose" })).toHaveClass("button", "compose");
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

    const { container } = render(
      <MobileToolbar
        {...commonProps(mailbox)}
        canCompose={false}
      />,
    );

    const scope = container.querySelector<HTMLDetailsElement>(".mobile-mailbox-scope");
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
    expect(within(scope!).getByRole("combobox", { name: "Language" })).toBeEnabled();
    expect(within(scope!).getByRole("button", { name: "Sign out" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Compose" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Inbox" }).querySelector("[data-icon='inbox']"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sent" }).querySelector("[data-icon='sent']"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Drafts" }).querySelector("[data-icon='draft']"))
      .toBeInTheDocument();
  });

  it("locks only the mailbox switch in an expanded mobile scope", () => {
    const mailbox: MailboxDirectoryItem = {
      mailbox_id: "019b0000-0000-7000-8000-000000000022",
      kind: "human",
      primary_address: "writer@example.test",
      can_send: true,
    };

    const { container } = render(
      <MobileToolbar
        {...commonProps(mailbox)}
        canCompose
        sessionLocked
      />,
    );

    const scope = container.querySelector<HTMLDetailsElement>(".mobile-mailbox-scope");
    fireEvent.click(scope?.querySelector("summary")!);

    expect(scope).toHaveAttribute("open");
    expect(within(scope!).getByRole("combobox", { name: "Switch mailbox" })).toBeDisabled();
    expect(within(scope!).getByRole("combobox", { name: "Language" })).toBeEnabled();
    expect(within(scope!).getByRole("button", { name: "Sign out" })).toBeEnabled();
  });
});
