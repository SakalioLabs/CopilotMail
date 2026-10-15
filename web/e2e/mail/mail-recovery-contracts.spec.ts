import type { Page } from "@playwright/test";
import {
  expect,
  FIXTURE_DRAFT_RESTORE_V5,
  FIXTURE_DRAFT_V4,
  FIXTURE_DRAFT_V5,
  LONG_SUBJECT,
  MAIL_FIXTURE_IDS,
  test,
} from "./mail-fixtures";

const DESKTOP = { width: 1440, height: 1024 } as const;
const TABLET = { width: 1008, height: 900 } as const;
const MOBILE = { width: 390, height: 844 } as const;

const GROUPED_MAILBOXES = [
  {
    mailbox_id: MAIL_FIXTURE_IDS.mailbox,
    kind: "human",
    primary_address: "alex.chen@copilotmail.test",
    can_send: true,
  },
  {
    mailbox_id: "20000000-0000-4000-8000-000000000002",
    kind: "agent",
    primary_address: "triage-agent@copilotmail.test",
    can_send: true,
  },
  {
    mailbox_id: "20000000-0000-4000-8000-000000000003",
    kind: "shared",
    primary_address: "operations-shared@copilotmail.test",
    can_send: false,
  },
  {
    mailbox_id: "20000000-0000-4000-8000-000000000004",
    kind: "group",
    primary_address: "security-group@copilotmail.test",
    can_send: false,
  },
] as const;

async function captureAt(
  page: Page,
  viewport: typeof DESKTOP | typeof TABLET | typeof MOBILE,
  name: string,
) {
  await page.setViewportSize(viewport);
  await expect(page).toHaveScreenshot(name, {
    animations: "disabled",
    caret: "hide",
    fullPage: true,
    scale: "css",
  });
}

test.describe("Mail grouped mailbox directory", () => {
  test.use({ mailFixtureOptions: { mailboxes: GROUPED_MAILBOXES } });

  test("renders all four identity groups and binds a switch to the selected mailbox", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    const card = page.locator(".sidebar .identity-card");
    await card.click();
    const menu = page.getByRole("menu", { name: "Account", exact: true });
    const section = menu.getByRole("group", { name: "Switch mailbox", exact: true });
    const groups = section.getByRole("group");
    await expect(groups).toHaveCount(4);
    await expect.poll(() => groups.evaluateAll((items) =>
      items.map((group) => group.getAttribute("aria-label"))))
      .toEqual([
        "Human mailboxes",
        "Agent mailboxes",
        "Shared mailboxes",
        "Group mailboxes",
      ]);
    await expect(section.getByRole("menuitemradio")).toHaveCount(4);

    const agentMailbox = GROUPED_MAILBOXES[1];
    const switchedInbox = page.waitForResponse((response) =>
      response.request().method() === "GET"
        && new URL(response.url()).pathname
          === `/api/v2/mailboxes/${agentMailbox.mailbox_id}/messages`);
    await section.getByRole("menuitemradio", {
      name: agentMailbox.primary_address,
      exact: true,
    }).click();
    await switchedInbox;
    await expect(card.locator(".identity-card__email"))
      .toHaveText(agentMailbox.primary_address);
    await card.click();
    const switchedSection = page.getByRole("menu", { name: "Account", exact: true })
      .getByRole("group", { name: "Switch mailbox", exact: true });
    await expect(switchedSection).toContainText("Agent");
    await expect(switchedSection).toContainText("Can send email");
    await expect.poll(() => fixture.state.inboxMailboxIds.at(-1))
      .toBe(agentMailbox.mailbox_id);

    await page.locator(".compose-fab").click();
    const composer = page.getByRole("dialog", { name: "New message", exact: true });
    const composerContext = composer.getByRole("region", { name: "Compose context", exact: true });
    const contextFacts = composerContext.locator(".context-bar__fact");
    const sendingScope = contextFacts.filter({ hasText: "Sending scope" });
    await expect(sendingScope).toHaveCount(1);
    await expect(sendingScope.locator("dt")).toHaveText("Sending scope");
    await expect(sendingScope.locator("dd")).toHaveText("triage-agent@copilotmail.test");
    const sendingIdentity = contextFacts.filter({ hasText: "Sending identity" });
    await expect(sendingIdentity).toHaveCount(1);
    await expect(sendingIdentity.locator("dt")).toHaveText("Sending identity");
    await expect(sendingIdentity.locator("dd")).toHaveText("Agent");
    await expect(contextFacts).toHaveCount(2);
    await expect(composerContext.locator(".context-bar__facts"))
      .not.toContainText("Server status");
    const serverStatus = composerContext.locator(".context-bar__status [data-sendable='true']");
    await expect(serverStatus).toHaveCount(1);
    await expect(serverStatus).toHaveText(
      "Server status: this mailbox can send email.",
    );
    await expect(serverStatus).not.toHaveAttribute("aria-live");
    await composer.getByRole("button", { name: "Close composer", exact: true }).click();
  });
});

test.describe("Mail search result boundaries", () => {
  test.use({ mailFixtureOptions: { searchMessages: [] } });

  test("renders a search-specific zero state and clears it from the keyboard", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    await page.keyboard.press("/");
    const search = page.getByRole("textbox", { name: "Search email", exact: true });
    await expect(search).toBeFocused();
    await search.fill("unmatched retention control");
    await page.keyboard.press("Enter");

    await expect(page.getByRole("heading", { name: "Search results", exact: true }))
      .toBeVisible();
    await expect(page.getByText("No matching messages", { exact: true })).toBeVisible();
    await expect(page.getByText(
      "Try a different search term or return to the inbox.",
      { exact: true },
    )).toBeVisible();
    await expect(page.locator(".empty--contextual img")).toHaveCount(0);
    expect(fixture.state.searchRequests).toEqual([{
      cursor: null,
      pageSize: 30,
      text: "unmatched retention control",
    }]);

    await captureAt(page, DESKTOP, "mail-search-no-results-1440.png");
    await captureAt(page, MOBILE, "mail-search-no-results-390.png");

    const clear = page.getByRole("button", { name: "Clear search", exact: true });
    await clear.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { name: "Inbox", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: LONG_SUBJECT })).toBeVisible();
    await expect(search).toHaveValue("");
    await expect(page.getByText("No matching messages", { exact: true })).toHaveCount(0);
  });

  test("returns from a directory to one coherent ordinary inbox state", async ({ mailApp }) => {
    const { fixture, page } = mailApp;
    await mailApp.goto();
    const search = page.getByRole("textbox", { name: "Search email", exact: true });
    await search.fill("unmatched retention control");
    await search.press("Enter");
    const searchResultsHeading = page.getByRole("heading", {
      name: "Search results",
      exact: true,
    });
    await expect(searchResultsHeading).toBeVisible();

    // The search field intentionally owns ordinary g/d keystrokes.
    await expect(search).toBeFocused();
    await page.keyboard.press("g");
    await page.keyboard.press("d");
    await expect(search).toHaveValue("unmatched retention controlgd");
    await search.fill("unmatched retention control");

    // Move focus to a non-editable application landmark before exercising the
    // same keys as a global navigation chord.
    await searchResultsHeading.focus();
    await expect(searchResultsHeading).toBeFocused();

    await page.keyboard.press("g");
    await page.keyboard.press("d");
    await expect(page.getByRole("heading", { name: "Drafts", exact: true })).toBeVisible();
    await page.keyboard.press("g");
    await page.keyboard.press("i");

    await expect(page.getByRole("heading", { name: "Inbox", exact: true })).toBeVisible();
    await expect(search).toHaveValue("");
    await expect(page.getByRole("button", { name: LONG_SUBJECT })).toBeVisible();
    await expect(page.getByText("No matching messages", { exact: true })).toHaveCount(0);
    expect(fixture.state.searchRequests).toHaveLength(1);
    await expect.poll(() => fixture.state.inboxRequestCount).toBe(2);
  });
});

test.describe("Mail detail recovery boundary", () => {
  test.use({ mailFixtureOptions: { detailFailuresBeforeSuccess: 1 } });

  test("keeps the selected-message context through a bounded 503 and retries in place", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    const message = page.getByRole("button", { name: LONG_SUBJECT });
    await message.focus();
    await page.keyboard.press("Enter");

    // The routed detail replaces the list; the URL is the selection record.
    await expect(page).toHaveURL(new RegExp(`#/message/${MAIL_FIXTURE_IDS.message}$`));
    await expect(message).toHaveCount(0);
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toBeVisible();
    await expect(page.locator(".detail-load-error").getByRole("alert"))
      .toHaveText("This message is temporarily unavailable");
    const metadata = page.locator(".message-metadata");
    await expect(metadata.locator("dt")).toHaveText(["From", "Received"]);
    await expect(metadata.locator("dd").first())
      .toHaveText("Jordan Li <jordan.li@partner.example>");
    await expect(page.getByText("fixture_message_store_unavailable", { exact: true }))
      .toHaveCount(0);
    expect(fixture.state.detailRequestCount).toBe(1);

    await captureAt(page, DESKTOP, "mail-detail-error-1440.png");
    await captureAt(page, MOBILE, "mail-detail-error-390.png");

    const retry = page.locator(".detail-load-error")
      .getByRole("button", { name: "Retry", exact: true });
    await retry.click();
    await expect(page.getByText(
      "The attached report records the approved routing evidence and the current retention checkpoint. Please add any material corrections before Friday.",
      { exact: true },
    )).toBeVisible();
    await expect(page.getByText("routing-evidence-q1.pdf", { exact: true })).toBeVisible();
    expect(fixture.state.detailRequestCount).toBe(2);

    await page.keyboard.press("Escape");
    await expect(message).toBeVisible();
    await expect(message).toBeFocused();
    await expect(message).toHaveAttribute("aria-current", "true");
  });
});

test.describe("Mail draft restore CAS", () => {
  test.use({
    mailFixtureOptions: {
      draftPages: [
        { items: [FIXTURE_DRAFT_V4], next_cursor: null, has_more: false },
        { items: [FIXTURE_DRAFT_V5], next_cursor: null, has_more: false },
      ],
      draftRestoreOutcomes: ["conflict", FIXTURE_DRAFT_RESTORE_V5],
    },
  });

  test("confirms replacement, refreshes a 409, and restores only the new exact version", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    await page.keyboard.press("c");
    const newComposer = page.getByRole("dialog", { name: "New message", exact: true });
    await newComposer.getByRole("textbox", { name: "Subject", exact: true })
      .fill("Unsaved local investigation");
    await page.keyboard.press("Escape");

    await page.keyboard.press("g");
    await page.keyboard.press("d");
    await expect(page.getByRole("heading", { name: "Drafts", exact: true })).toBeVisible();
    const restore = page.getByRole("button", { name: /Resume editing$/ });
    await expect(restore).toHaveCount(1);
    await restore.focus();
    await page.keyboard.press("Enter");

    const firstConfirmation = page.getByRole("alertdialog", {
      name: "Replace the current message?",
      exact: true,
    });
    const cancel = firstConfirmation.getByRole("button", {
      name: "Keep current content",
      exact: true,
    });
    await expect(cancel).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(firstConfirmation).toBeHidden();
    await expect(restore).toBeFocused();
    expect(fixture.state.draftRestoreRequests).toEqual([]);

    await page.keyboard.press("Enter");
    const conflictConfirmation = page.getByRole("alertdialog", {
      name: "Replace the current message?",
      exact: true,
    });
    await expect(conflictConfirmation.getByRole("button", {
      name: "Keep current content",
      exact: true,
    })).toBeFocused();
    await page.keyboard.press("Tab");
    const confirm = conflictConfirmation.getByRole("button", {
      name: "Restore draft",
      exact: true,
    });
    await expect(confirm).toBeFocused();
    await page.keyboard.press("Enter");

    const conflict = page.getByRole("alert");
    await expect(conflict).toHaveText(
      "This draft changed elsewhere; the list was refreshedRetry",
    );
    await expect.poll(() => fixture.state.draftListRequestCount).toBe(2);
    await expect(restore).toBeFocused();
    expect(fixture.state.draftRestoreRequests).toEqual([{
      draftId: MAIL_FIXTURE_IDS.draft,
      expectedVersion: 4,
    }]);
    expect(fixture.state.draftSaveRequestCount).toBe(0);

    await captureAt(page, DESKTOP, "mail-draft-cas-conflict-1440.png");
    await captureAt(page, MOBILE, "mail-draft-cas-conflict-390.png");

    await expect(restore).toBeFocused();
    await page.keyboard.press("Enter");
    const refreshedConfirmation = page.getByRole("alertdialog", {
      name: "Replace the current message?",
      exact: true,
    });
    await expect(refreshedConfirmation.getByRole("button", {
      name: "Keep current content",
      exact: true,
    })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(refreshedConfirmation.getByRole("button", {
      name: "Restore draft",
      exact: true,
    })).toBeFocused();
    await page.keyboard.press("Enter");

    const restored = page.getByRole("dialog", { name: "Edit draft", exact: true });
    await expect(restored.getByRole("textbox", { name: "To", exact: true }))
      .toHaveValue("review-owner@copilotmail.test");
    await restored.getByText("Cc / Bcc", { exact: true }).click();
    await expect(restored.getByRole("textbox", { name: "Cc", exact: true }))
      .toHaveValue("operations@copilotmail.test");
    await expect(restored.getByRole("textbox", { name: "Bcc", exact: true }))
      .toHaveValue("audit@copilotmail.test");
    await expect(restored.getByRole("textbox", { name: "Subject", exact: true }))
      .toHaveValue("Recovered retention review");
    await expect(restored.getByRole("textbox", { name: "Message body", exact: true }))
      .toHaveValue("The server-authoritative version is ready for continued editing.");
    await captureAt(page, DESKTOP, "mail-restored-draft-1440.png");
    await captureAt(page, TABLET, "mail-restored-draft-1008.png");
    await captureAt(page, MOBILE, "mail-restored-draft-390.png");
    expect(fixture.state.draftRestoreRequests).toEqual([
      { draftId: MAIL_FIXTURE_IDS.draft, expectedVersion: 4 },
      { draftId: MAIL_FIXTURE_IDS.draft, expectedVersion: 5 },
    ]);
    expect(fixture.state.draftListRequestCount).toBe(2);
    expect(fixture.state.draftSaveRequestCount).toBe(0);
  });
});
