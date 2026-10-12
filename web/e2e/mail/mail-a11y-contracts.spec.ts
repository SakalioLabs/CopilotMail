import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import {
  expect,
  FIXTURE_DRAFT_V4,
  FIXTURE_MESSAGE,
  FIXTURE_SENT_STATE_PAGE,
  LONG_SUBJECT,
  SECOND_FIXTURE_MESSAGE,
  test,
} from "./mail-fixtures";

/**
 * Screenshot-free accessibility contracts for the authenticated mail views.
 * These cases prove scripted focus paints a visible ring, dialogs trap and
 * return focus in a real browser, lists announce their state through polite
 * live regions, and the queue stays reachable through the plain Tab order.
 * They are Axe/keyboard evidence only; they do not claim WCAG 2.2 AA.
 */
const WCAG_22_AA_TAGS = [
  "wcag2a",
  "wcag2aa",
  "wcag21aa",
  "wcag22aa",
] as const;

const DESKTOP = { width: 1440, height: 1024 } as const;

async function expectNoAxeViolations(page: Page, include: string) {
  const results = await new AxeBuilder({ page })
    .include(include)
    .withTags([...WCAG_22_AA_TAGS])
    .analyze();
  expect(results.violations, JSON.stringify(results.violations, null, 2))
    .toEqual([]);
}

test("the composer traps Tab in both directions and returns focus to Compose", async ({
  mailApp,
}) => {
  const { page } = mailApp;
  await page.setViewportSize(DESKTOP);
  await mailApp.goto();

  const composeTrigger = page.locator(".sidebar .compose");
  await composeTrigger.focus();
  await page.keyboard.press("Enter");
  const composer = page.getByRole("dialog", { name: "New message", exact: true });
  await expect(composer).toBeVisible();
  await expect(composer).toHaveAttribute("aria-modal", "true");

  const to = composer.getByRole("textbox", { name: "To", exact: true });
  await expect(to).toBeFocused();
  expect(await to.evaluate((element) => element.matches(":focus-visible")))
    .toBe(true);

  const close = composer.getByRole("button", {
    name: "Close composer",
    exact: true,
  });
  await to.fill("reviewer@copilotmail.test");
  const send = composer.getByRole("button", { name: "Send", exact: true });
  await expect(send).toBeEnabled();

  await send.focus();
  await page.keyboard.press("Tab");
  await expect(close).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(send).toBeFocused();

  await expectNoAxeViolations(page, ".dialog");

  await page.keyboard.press("Escape");
  await expect(composer).toBeHidden();
  await expect(composeTrigger).toBeFocused();
});

test.describe("draft restore confirmation", () => {
  test.use({
    mailFixtureOptions: {
      draftPages: [
        { items: [FIXTURE_DRAFT_V4], next_cursor: null, has_more: false },
      ],
    },
  });

  test("keeps focus inside the alertdialog and returns it to the restore row", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    // The confirmation exists only to protect unsaved composer content.
    await page.keyboard.press("c");
    const composer = page.getByRole("dialog", { name: "New message", exact: true });
    await composer.getByRole("textbox", { name: "Subject", exact: true })
      .fill("Unsaved local investigation");
    await page.keyboard.press("Escape");

    await page.keyboard.press("g");
    await page.keyboard.press("d");
    await expect(page.getByRole("heading", { name: "Drafts", exact: true }))
      .toBeVisible();
    const restore = page.getByRole("button", { name: /Resume editing$/u });
    await expect(restore).toHaveCount(1);
    await restore.focus();
    await page.keyboard.press("Enter");

    const confirmation = page.getByRole("alertdialog", {
      name: "Replace the current message?",
      exact: true,
    });
    await expect(confirmation).toHaveAttribute("aria-modal", "true");
    const keep = confirmation.getByRole("button", {
      name: "Keep current content",
      exact: true,
    });
    const confirmRestore = confirmation.getByRole("button", {
      name: "Restore draft",
      exact: true,
    });
    await expect(keep).toBeFocused();
    expect(await keep.evaluate((element) => element.matches(":focus-visible")))
      .toBe(true);

    // Both wrap directions stay inside the two-action confirmation.
    await page.keyboard.press("Shift+Tab");
    await expect(confirmRestore).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(keep).toBeFocused();

    await expectNoAxeViolations(page, ".dialog");

    await page.keyboard.press("Escape");
    await expect(confirmation).toBeHidden();
    await expect(restore).toBeFocused();
    expect(fixture.state.draftRestoreRequests).toEqual([]);
  });
});

test.describe("mail queue reachability", () => {
  test.use({
    mailFixtureOptions: {
      messages: [FIXTURE_MESSAGE, SECOND_FIXTURE_MESSAGE],
      sentPages: [FIXTURE_SENT_STATE_PAGE],
    },
  });

  test("announces queue state through polite atomic status regions", async ({
    mailApp,
  }) => {
    const { page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    const inboxStatus = page.locator(".inbox").getByRole("status");
    await expect(inboxStatus).toHaveAttribute("aria-live", "polite");
    await expect(inboxStatus).toHaveAttribute("aria-atomic", "true");
    await expect(inboxStatus).toHaveText("2 messages");

    await page.keyboard.press("g");
    await page.keyboard.press("d");
    await expect(page.getByRole("heading", { name: "Drafts", exact: true }))
      .toBeVisible();
    const draftStatus = page.locator(".drafts-pane").getByRole("status");
    await expect(draftStatus).toHaveAttribute("aria-live", "polite");
    await expect(draftStatus).toHaveAttribute("aria-atomic", "true");
    await expect(draftStatus).toHaveText("0 drafts");

    await page.keyboard.press("g");
    await page.keyboard.press("s");
    await expect(page.getByRole("heading", { name: "Sent", exact: true }))
      .toBeVisible();
    const sentStatus = page.locator(".sent-pane").getByRole("status");
    await expect(sentStatus).toHaveAttribute("aria-live", "polite");
    await expect(sentStatus).toHaveAttribute("aria-atomic", "true");
    await expect(sentStatus).toHaveText("Showing 4 sent messages");
  });

  test("keeps the queue on the plain Tab order and j/k on selection order", async ({
    mailApp,
  }) => {
    const { page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    const first = page.getByRole("button", { name: LONG_SUBJECT });
    const second = page.getByRole("button", {
      name: SECOND_FIXTURE_MESSAGE.subject,
    });

    const search = page.getByRole("textbox", { name: "Search email", exact: true });
    await search.focus();
    await page.keyboard.press("Tab");
    await expect(page.getByRole("button", { name: "Search", exact: true }))
      .toBeFocused();
    await page.keyboard.press("Tab");
    await expect(first).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(second).toBeFocused();

    // j/k move the same real focus along with the selection marker.
    await page.keyboard.press("j");
    await expect(first).toBeFocused();
    await expect(first).toHaveAttribute("aria-current", "true");
    await page.keyboard.press("j");
    await expect(second).toBeFocused();
    await expect(second).toHaveAttribute("aria-current", "true");
    await page.keyboard.press("k");
    await expect(first).toBeFocused();
    await expect(first).toHaveAttribute("aria-current", "true");
  });

  test("announces the same queue state in zh-CN", async ({ mailApp }) => {
    const { page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();
    await page.getByRole("combobox", { name: "Language", exact: true })
      .selectOption("zh-CN");
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");

    const inboxStatus = page.locator(".inbox").getByRole("status");
    await expect(inboxStatus).toHaveAttribute("aria-live", "polite");
    await expect(inboxStatus).toHaveText("2 封邮件");
    await expect(page.getByRole("textbox", { name: "搜索邮件", exact: true }))
      .toBeVisible();
    await expectNoAxeViolations(page, ".inbox");
  });
});
