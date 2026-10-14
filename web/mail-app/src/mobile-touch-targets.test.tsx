// @vitest-environment jsdom
/// <reference types="vite/client" />

import { createRef } from "react";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import detailStyles from "./detail.css?raw";
import searchStyles from "./search.css?raw";
import mailStyles from "./styles.css?raw";
import { MessageDetail } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { MailTopbar } from "./components/MailTopbar";
import { MobileNavigation } from "./components/MailNavigation";
import { createTranslator } from "./i18n";
import type { InboxPage, MailboxDirectoryItem, MessageSummary } from "./types";

const t = createTranslator("en");
const mailbox: MailboxDirectoryItem = {
  mailbox_id: "019b0000-0000-7000-8000-000000000020",
  kind: "human",
  primary_address: "reader@example.test",
  can_send: true,
};
const message: MessageSummary = {
  message_id: "019b0000-0000-7000-8000-000000000001",
  subject: "Quarterly review",
  from: [{ name: "Alice", email: "alice@example.test" }],
  received_at: "2026-07-27T12:00:00.000Z",
  preview: "A short preview",
  is_read: true,
  is_flagged: false,
  has_attachment: false,
};
const page: InboxPage = { items: [message], next_cursor: null, has_more: false };

function mediaBlock(styles: string, query: string) {
  const mediaStart = styles.indexOf(`@media ${query}`);
  if (mediaStart < 0) throw new Error(`Missing ${query} media rule`);

  const openingBrace = styles.indexOf("{", mediaStart);
  let depth = 0;
  for (let index = openingBrace; index < styles.length; index += 1) {
    if (styles[index] === "{") depth += 1;
    if (styles[index] === "}") {
      depth -= 1;
      if (depth === 0) return styles.slice(openingBrace + 1, index);
    }
  }

  throw new Error(`Unclosed ${query} media rule`);
}

function selectorRule(styles: string, selector: string) {
  const selectorStart = styles.indexOf(selector);
  if (selectorStart < 0) throw new Error(`Missing ${selector} rule`);

  const openingBrace = styles.indexOf("{", selectorStart);
  const closingBrace = styles.indexOf("}", openingBrace);
  return styles.slice(openingBrace + 1, closingBrace);
}

afterEach(cleanup);

describe("mobile touch target contract", () => {
  it("defines 44px controls inside the viewport breakpoint, independent of pointer type", () => {
    expect(mailStyles).toContain("--mail-touch: var(--cm-touch-target);");

    const mobileMailStyles = mediaBlock(mailStyles, "(max-width: 1007px)");
    expect(selectorRule(mobileMailStyles, ".mail-topbar__menu")).toContain(
      "display: inline-grid;",
    );
    // The floating compose action renders at every width, so its base rule is
    // outside the media block; the breakpoint keeps the 44px floor below.
    expect(selectorRule(mailStyles, ".compose-fab")).toContain("display: grid;");
    expect(selectorRule(mailStyles, ".compose-fab")).toContain(
      "bottom: max(var(--cm-space-5), env(safe-area-inset-bottom));",
    );
    expect(selectorRule(mobileMailStyles, ".compose-fab")).toContain(
      "min-height: var(--mail-touch);",
    );
    // Drawer scope chrome keeps its 44px floor as base styles because the
    // drawer only ever opens below the mobile breakpoint.
    expect(selectorRule(mailStyles, ".mobile-mailbox-scope__summary")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mobileMailStyles, ".mail-nav-drawer button")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mailStyles, ".mobile-mailbox-picker select")).toContain(
      "min-width: var(--mail-touch);",
    );
    expect(selectorRule(mailStyles, ".mobile-mailbox-picker select")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mailStyles, ".identity-card")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mailStyles, ".settings-dialog .language-picker select")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mobileMailStyles, ".detail-back")).toContain(
      "min-width: var(--mail-touch);",
    );
    expect(selectorRule(mobileMailStyles, ".detail-back")).toContain(
      "min-height: var(--mail-touch);",
    );
    for (const selector of [
      ".mail-topbar .icon-button",
      ".inbox-header .inbox-refresh",
      ".message__flag",
      ".composer .dialog__header .icon-button",
      ".command-palette__close",
    ]) {
      expect(selectorRule(mobileMailStyles, selector)).toContain(
        "width: var(--mail-touch);",
      );
      expect(selectorRule(mobileMailStyles, selector)).toContain(
        "height: var(--mail-touch);",
      );
    }
    for (const selector of [
      ".mail-access__action",
      ".load-more",
      ".error--actionable button",
      ".empty-action",
      ".dialog[role=\"alertdialog\"] button",
      ".composer footer button",
      ".command-palette__search input",
      ".command-palette__command",
      ".messages > .message-row > .message",
      ".messages > .draft-item",
    ]) {
      expect(selectorRule(mobileMailStyles, selector)).toContain(
        "min-height: var(--mail-touch);",
      );
    }
    expect(selectorRule(mobileMailStyles, ".composer input")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mobileMailStyles, ".composer-recipients summary")).toContain(
      "min-height: var(--mail-touch);",
    );

    const mobileSearchStyles = mediaBlock(searchStyles, "(max-width: 1007px)");
    expect(selectorRule(mobileSearchStyles, ".mail-search input")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mobileSearchStyles, ".mail-search .icon-button")).toContain(
      "width: var(--mail-touch);",
    );
    expect(selectorRule(mobileSearchStyles, ".mail-search .icon-button")).toContain(
      "height: var(--mail-touch);",
    );

    const mobileDetailStyles = mediaBlock(detailStyles, "(max-width: 1007px)");
    expect(selectorRule(mobileDetailStyles, ".reading-actions .icon-button")).toContain(
      "width: var(--mail-touch);",
    );
    expect(selectorRule(mobileDetailStyles, ".reading-actions .icon-button")).toContain(
      "height: var(--mail-touch);",
    );
    expect(selectorRule(mobileDetailStyles, ".detail-load-error button")).toContain(
      "min-height: var(--mail-touch);",
    );
    expect(selectorRule(mobileDetailStyles, ".reply-action")).toContain(
      "min-height: var(--mail-touch);",
    );
  });

  it("keeps each styled mobile action on its semantic native control", () => {
    render(
      <MobileNavigation
        activeMailbox={mailbox}
        activeView="inbox"
        canCompose
        drawerOpen
        locale="en"
        mailboxHasMore
        mailboxLoadMoreBusy={false}
        mailboxes={[mailbox]}
        sessionLocked={false}
        onCompose={vi.fn()}
        onDrawerClose={vi.fn()}
        onMailboxChange={vi.fn()}
        onMailboxLoadMore={vi.fn()}
        onOpenCommandPalette={vi.fn()}
        onOpenSettings={vi.fn()}
        onSignOut={vi.fn()}
        onViewChange={vi.fn()}
        t={t}
      />,
    );

    const drawer = screen.getByRole("dialog", { name: "Mail navigation" });
    expect(drawer.querySelector<HTMLSelectElement>(".mobile-mailbox-picker select"))
      .toHaveAccessibleName("Switch mailbox");
    expect(within(drawer).getByRole("button", { name: /reader@example\.test/ }))
      .toHaveClass("identity-card");
    expect(document.querySelector<HTMLButtonElement>(".compose-fab"))
      .toBeInstanceOf(HTMLButtonElement);
    expect(within(drawer).getByRole("button", { name: "Open command palette" }))
      .toHaveClass("mobile-command-trigger");
    expect(within(drawer).getByRole("button", { name: "Inbox" }))
      .toBeInstanceOf(HTMLButtonElement);
    expect(within(drawer).getByRole("button", { name: "Sent" }))
      .toBeInstanceOf(HTMLButtonElement);
    expect(within(drawer).getByRole("button", { name: "Drafts" }))
      .toBeInstanceOf(HTMLButtonElement);

    const { container: topbar } = render(
      <MailTopbar
        navigationOpen={false}
        searchBusy={false}
        searchText=""
        t={t}
        onClearSearch={vi.fn()}
        onOpenMenu={vi.fn()}
        onSearch={vi.fn()}
        onSearchTextChange={vi.fn()}
      />,
    );
    expect(topbar.querySelector<HTMLInputElement>(".mail-search input"))
      .toHaveAccessibleName("Search email");
    expect(topbar.querySelector<HTMLButtonElement>(".mail-search__submit"))
      .toHaveAccessibleName("Search");

    render(
      <MessageList
        locale="en"
        page={page}
        searchBusy={false}
        searchText=""
        t={t}
        messageButtonRef={() => () => undefined}
        onLoadMore={vi.fn()}
        onOpenMessage={vi.fn()}
        onRefresh={vi.fn()}
        onSearch={vi.fn()}
        onToggleFlag={vi.fn()}
      />,
    );

    const { container: detail } = render(
      <MessageDetail
        canReply
        headingRef={createRef<HTMLHeadingElement>()}
        locale="en"
        selected={message}
        t={t}
        onBack={vi.fn()}
        onReply={vi.fn()}
        onRetry={vi.fn()}
        onToggleFlag={vi.fn()}
      />,
    );
    expect(detail.querySelector<HTMLButtonElement>(".detail-back"))
      .toHaveAccessibleName("Back to inbox");
    expect(detail.querySelector<HTMLButtonElement>(".reading-actions .icon-button"))
      .toHaveAccessibleName("Add star");
  });
});
