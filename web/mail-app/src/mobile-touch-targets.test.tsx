// @vitest-environment jsdom
/// <reference types="vite/client" />

import { createRef } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import detailStyles from "./detail.css?raw";
import searchStyles from "./search.css?raw";
import mailStyles from "./styles.css?raw";
import { MessageDetail } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { MobileToolbar } from "./components/MailNavigation";
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
    expect(mailStyles).toContain("--mail-touch-target-size: var(--cm-touch-target);");

    const mobileMailStyles = mediaBlock(mailStyles, "(max-width: 1007px)");
    expect(selectorRule(mobileMailStyles, ".mobile-actions")).toContain("flex: 0 0 auto;");
    expect(selectorRule(mobileMailStyles, ".mobile-mailbox-scope__summary")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".mobile-toolbar button")).toContain(
      "min-width: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".mobile-toolbar button")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".mobile-mailbox-picker select")).toContain(
      "min-width: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".mobile-mailbox-picker select")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(
      mobileMailStyles,
      ".mobile-mailbox-scope__session-controls .language-picker select",
    )).toContain(
      "min-width: var(--mail-touch-target-size);",
    );
    expect(selectorRule(
      mobileMailStyles,
      ".mobile-mailbox-scope__session-controls .language-picker select",
    )).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".mobile-view-tabs button")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".detail-back")).toContain(
      "min-width: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".detail-back")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    for (const selector of [
      ".inbox-header > .icon-button",
      ".composer .dialog__header .icon-button",
      ".command-palette__close",
    ]) {
      expect(selectorRule(mobileMailStyles, selector)).toContain(
        "width: var(--mail-touch-target-size);",
      );
      expect(selectorRule(mobileMailStyles, selector)).toContain(
        "height: var(--mail-touch-target-size);",
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
      ".messages > .message",
      ".messages > .draft-item",
    ]) {
      expect(selectorRule(mobileMailStyles, selector)).toContain(
        "min-height: var(--mail-touch-target-size);",
      );
    }
    expect(selectorRule(mobileMailStyles, ".composer input")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileMailStyles, ".composer-recipients summary")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );

    const mobileSearchStyles = mediaBlock(searchStyles, "(max-width: 1007px)");
    expect(selectorRule(mobileSearchStyles, ".mail-search input")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileSearchStyles, ".mail-search .icon-button")).toContain(
      "width: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileSearchStyles, ".mail-search .icon-button")).toContain(
      "height: var(--mail-touch-target-size);",
    );

    const mobileDetailStyles = mediaBlock(detailStyles, "(max-width: 1007px)");
    expect(selectorRule(mobileDetailStyles, ".reading-actions .icon-button")).toContain(
      "width: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileDetailStyles, ".reading-actions .icon-button")).toContain(
      "height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileDetailStyles, ".detail-load-error button")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
    expect(selectorRule(mobileDetailStyles, ".reply-action")).toContain(
      "min-height: var(--mail-touch-target-size);",
    );
  });

  it("keeps each styled mobile action on its semantic native control", () => {
    const { container: toolbar } = render(
      <MobileToolbar
        activeMailbox={mailbox}
        activeView="inbox"
        canCompose
        locale="en"
        mailboxHasMore
        mailboxLoadMoreBusy={false}
        mailboxes={[mailbox]}
        sessionLocked={false}
        onCompose={vi.fn()}
        onLocaleChange={vi.fn()}
        onMailboxChange={vi.fn()}
        onMailboxLoadMore={vi.fn()}
        onOpenCommandPalette={vi.fn()}
        onSignOut={vi.fn()}
        onViewChange={vi.fn()}
        t={t}
      />,
    );

    expect(toolbar.querySelector<HTMLSelectElement>(".mobile-mailbox-picker select"))
      .toHaveAccessibleName("Switch mailbox");
    expect(toolbar.querySelector<HTMLSelectElement>(".language-picker select"))
      .toHaveAccessibleName("Language");
    expect(screen.getByRole("button", { name: "Compose" })).toHaveClass("mobile-compose");
    expect(screen.getByRole("button", { name: "Open command palette" }))
      .toHaveClass("mobile-command-trigger");
    expect(screen.getByRole("button", { name: "Inbox" }))
      .toBeInstanceOf(HTMLButtonElement);
    expect(screen.getByRole("button", { name: "Sent" }))
      .toBeInstanceOf(HTMLButtonElement);
    expect(screen.getByRole("button", { name: "Drafts" }))
      .toBeInstanceOf(HTMLButtonElement);

    const { container: list } = render(
      <MessageList
        locale="en"
        page={page}
        searchBusy={false}
        searchText=""
        t={t}
        messageButtonRef={() => () => undefined}
        onClearSearch={vi.fn()}
        onLoadMore={vi.fn()}
        onOpenMessage={vi.fn()}
        onRefresh={vi.fn()}
        onSearch={vi.fn()}
        onSearchTextChange={vi.fn()}
      />,
    );
    expect(list.querySelector<HTMLInputElement>(".mail-search input"))
      .toHaveAccessibleName("Search email");
    expect(list.querySelector<HTMLButtonElement>(".mail-search__submit"))
      .toHaveAccessibleName("Search");

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
