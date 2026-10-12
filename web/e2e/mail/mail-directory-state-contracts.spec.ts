import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import type { MailDelivery, MailSubmission } from "../../mail-app/src/types";
import {
  expect,
  FIXTURE_DRAFT_V4,
  FIXTURE_SENT_STATE_PAGE,
  LONG_SUBJECT,
  MAIL_FIXTURE_IDS,
  test,
} from "./mail-fixtures";

const DESKTOP = { width: 1440, height: 1024 } as const;
const TABLET = { width: 1008, height: 900 } as const;
const MOBILE = { width: 390, height: 844 } as const;
const DIRECTORY_VIEWPORTS = [
  { name: "1440", ...DESKTOP },
  { name: "1008", ...TABLET },
  { name: "390", ...MOBILE },
] as const;
const WCAG_22_AA_TAGS = [
  "wcag2a",
  "wcag2aa",
  "wcag21a",
  "wcag21aa",
  "wcag22aa",
] as const;
const FIRST_RECIPIENT_REF =
  "4db054bef65528cf7d9df038af553c8211362e2e6d9ba7e7f0689535958081a8";
const SECOND_RECIPIENT_REF =
  "9ce445b59023aa524745035f4d14d92baf55e4beb7c50548ca26abdcf2404e11";

const SUBMISSION_BASE = {
  submission_id: MAIL_FIXTURE_IDS.submission,
  mailbox_id: MAIL_FIXTURE_IDS.mailbox,
  draft_id: MAIL_FIXTURE_IDS.draft,
  draft_version: 1,
  created_at: "2026-01-18T12:10:00Z",
} as const;

const QUEUED_DELIVERY: MailDelivery = {
  observed_at: "2026-01-18T12:10:02Z",
  terminal: false,
  recipient_count: 2,
  delivered_count: 0,
  failed_count: 0,
  queued_count: 2,
  unknown_count: 0,
  recipients: [{
    recipient_ref: FIRST_RECIPIENT_REF,
    status: "queued",
    smtp_reply: "queued for recipient one",
  }, {
    recipient_ref: SECOND_RECIPIENT_REF,
    status: "queued",
    smtp_reply: "queued for recipient two",
  }],
};

const PARTIAL_DELIVERY: MailDelivery = {
  observed_at: "2026-01-18T12:10:03Z",
  terminal: true,
  recipient_count: 2,
  delivered_count: 1,
  failed_count: 1,
  queued_count: 0,
  unknown_count: 0,
  recipients: [{
    recipient_ref: FIRST_RECIPIENT_REF,
    status: "delivered",
    smtp_reply: "250 2.0.0 accepted by recipient server",
  }, {
    recipient_ref: SECOND_RECIPIENT_REF,
    status: "permanent_failure",
    smtp_reply: "550 5.1.1 recipient rejected",
  }],
};

const ACCEPTED_TO_PARTIAL_TIMELINE: readonly MailSubmission[] = [{
  ...SUBMISSION_BASE,
  status: "accepted",
  state_version: 1,
  updated_at: "2026-01-18T12:10:01Z",
  delivery: null,
}, {
  ...SUBMISSION_BASE,
  status: "accepted",
  state_version: 1,
  updated_at: "2026-01-18T12:10:01Z",
  delivery: QUEUED_DELIVERY,
}, {
  ...SUBMISSION_BASE,
  status: "accepted",
  state_version: 1,
  updated_at: "2026-01-18T12:10:01Z",
  delivery: PARTIAL_DELIVERY,
}];

const PENDING_TO_MANUAL_REVIEW_TIMELINE: readonly MailSubmission[] = [{
  ...SUBMISSION_BASE,
  status: "pending",
  state_version: 1,
  updated_at: "2026-01-18T12:10:01Z",
  delivery: null,
}, {
  ...SUBMISSION_BASE,
  status: "manual_review",
  state_version: 2,
  updated_at: "2026-01-18T12:10:02Z",
  delivery: null,
}];

const RECONCILING_TO_REJECTED_TIMELINE: readonly MailSubmission[] = [{
  ...SUBMISSION_BASE,
  status: "reconciling",
  state_version: 1,
  updated_at: "2026-01-18T12:10:01Z",
  delivery: null,
}, {
  ...SUBMISSION_BASE,
  status: "rejected",
  state_version: 2,
  updated_at: "2026-01-18T12:10:02Z",
  delivery: null,
}];

async function captureAt(
  page: Page,
  viewport: Readonly<{ width: number; height: number }>,
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

async function expectNoHorizontalOverflow(page: Page) {
  await expect.poll(() => page.evaluate(() => (
    document.documentElement.scrollWidth <= document.documentElement.clientWidth
  ))).toBe(true);
}

function expectDirectoryToStayMetadataOnly(
  fixture: { state: { detailRequestCount: number; messageStateRequestCount: number } },
) {
  expect(fixture.state.detailRequestCount).toBe(0);
  expect(fixture.state.messageStateRequestCount).toBe(0);
}

async function openComposerAndSend(page: Page, subject: string) {
  await page.locator(".sidebar .compose").click();
  const composer = page.getByRole("dialog", { name: "New message", exact: true });
  await composer.getByRole("textbox", { name: "To", exact: true })
    .fill("one@recipient.example; two@recipient.example");
  await composer.getByRole("textbox", { name: "Subject", exact: true }).fill(subject);
  await composer.getByRole("textbox", { name: "Message body", exact: true })
    .fill("This deterministic browser fixture exercises the server submission lifecycle.");
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  return composer;
}

test.describe("Mail draft directory evidence", () => {
  test.use({
    mailFixtureOptions: {
      draftPages: [{
        items: [FIXTURE_DRAFT_V4],
        next_cursor: null,
        has_more: false,
      }],
    },
  });

  test("shows only real server metadata for a restorable draft", async ({ mailApp }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();
    await page.locator(".sidebar").getByRole("button", { name: "Drafts", exact: true }).click();

    const draft = page.locator(".draft-item");
    await expect(draft).toHaveCount(1);
    await expect(draft.getByText("New message draft", { exact: true })).toBeVisible();
    await expect(draft.getByText("3 recipients", { exact: true })).toBeVisible();
    await expect(draft.getByText("Resume editing", { exact: true })).toBeVisible();
    await expect(page.getByText("Recovered retention review", { exact: true })).toHaveCount(0);
    await expect(page.getByText("review-owner@copilotmail.test", { exact: true })).toHaveCount(0);
    await expect(page.locator(".mail-shell > .detail")).toHaveCount(0);
    expectDirectoryToStayMetadataOnly(fixture);

    for (const viewport of DIRECTORY_VIEWPORTS) {
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await expect(draft).toHaveCount(1);
      await expectNoHorizontalOverflow(page);
      expectDirectoryToStayMetadataOnly(fixture);
    }

    await captureAt(page, DESKTOP, "mail-drafts-normal-1440.png");
    await captureAt(page, TABLET, "mail-drafts-normal-1008.png");
    await captureAt(page, MOBILE, "mail-drafts-normal-390.png");
  });
});

test("renders truthful empty Drafts and Sent states", async ({ mailApp }) => {
  const { page } = mailApp;
  await page.setViewportSize(DESKTOP);
  await mailApp.goto();

  await page.locator(".sidebar").getByRole("button", { name: "Drafts", exact: true }).click();
  await expect(page.getByText(
    "This mailbox has no draft that can be restored",
    { exact: true },
  )).toBeVisible();
  await expect(page.locator(".empty-action").getByText("Refresh drafts", { exact: true }))
    .toBeVisible();

  await page.locator(".sidebar").getByRole("button", { name: "Sent", exact: true }).click();
  await expect(page.getByText(
    "This mailbox has no accepted sent messages yet",
    { exact: true },
  )).toBeVisible();
  await expect(page.locator(".empty-action").getByText("Refresh sent mail", { exact: true }))
    .toBeVisible();
});

test.describe("Mail directory recovery evidence", () => {
  test.use({
    mailFixtureOptions: {
      draftFailuresBeforeSuccess: 1,
      draftPages: [{
        items: [FIXTURE_DRAFT_V4],
        next_cursor: null,
        has_more: false,
      }],
      sentFailuresBeforeSuccess: 1,
      sentPages: [FIXTURE_SENT_STATE_PAGE],
    },
  });

  test("recovers Drafts and Sent independently through their visible retry actions", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    await page.locator(".sidebar").getByRole("button", { name: "Drafts", exact: true }).click();
    await expect(page.getByRole("alert"))
      .toHaveText("Drafts are temporarily unavailableRetry");
    await page.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(page.locator(".draft-item")).toHaveCount(1);
    expect(fixture.state.draftListRequestCount).toBe(2);

    await page.locator(".sidebar").getByRole("button", { name: "Sent", exact: true }).click();
    await expect(page.getByRole("alert"))
      .toHaveText("Sent mail is temporarily unavailableRetry");
    await page.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(page.locator(".sent-item")).toHaveCount(4);
    expect(fixture.state.sentListRequestCount).toBe(2);
  });
});

test.describe("Mail sent delivery-state evidence", () => {
  test.use({ mailFixtureOptions: { sentPages: [FIXTURE_SENT_STATE_PAGE] } });

  test("renders accepted, queued, delivered, and partial outcomes without row affordance", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();
    await page.locator(".sidebar").getByRole("button", { name: "Sent", exact: true }).click();

    const sentItems = page.locator(".sent-item");
    await expect(sentItems).toHaveCount(4);
    await expect(sentItems.getByText("Accepted for delivery", { exact: true })).toHaveCount(4);
    await expect(page.getByText(
      "Accepted for delivery; waiting for a delivery status update",
      { exact: true },
    )).toBeVisible();
    await expect(page.getByText(
      "Delivery update: 0/2 delivered, 0 failed, 2 queued, 0 unknown",
      { exact: true },
    )).toBeVisible();
    await expect(page.getByText(
      "Delivered to all 2 recipient servers",
      { exact: true },
    )).toBeVisible();
    await expect(page.getByText(
      "Delivery complete: 1 succeeded, 1 failed",
      { exact: true },
    )).toBeVisible();
    await expect(sentItems.getByRole("button")).toHaveCount(0);
    await expect(sentItems.first()).toHaveCSS("cursor", "auto");
    const initialBackground = await sentItems.first().evaluate(
      (element) => getComputedStyle(element).backgroundColor,
    );
    await sentItems.first().hover();
    expect(await sentItems.first().evaluate(
      (element) => getComputedStyle(element).backgroundColor,
    )).toBe(initialBackground);
    await expect(page.locator(".mail-shell > .detail")).toHaveCount(0);
    expectDirectoryToStayMetadataOnly(fixture);

    for (const viewport of DIRECTORY_VIEWPORTS) {
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await expect(sentItems).toHaveCount(4);
      await expectNoHorizontalOverflow(page);
      expectDirectoryToStayMetadataOnly(fixture);
    }

    await captureAt(page, DESKTOP, "mail-sent-states-1440.png");
    await captureAt(page, TABLET, "mail-sent-states-1008.png");
    await captureAt(page, MOBILE, "mail-sent-states-390.png");
  });
});

test.describe("Mail D4C metadata-only directory boundaries", () => {
  test.use({
    mailFixtureOptions: {
      draftPages: [{
        items: [FIXTURE_DRAFT_V4],
        next_cursor: null,
        has_more: false,
      }],
      sentPages: [FIXTURE_SENT_STATE_PAGE],
    },
  });

  test("switches Drafts and Sent without reading a message or writing message state", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;

    for (const viewport of DIRECTORY_VIEWPORTS) {
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await mailApp.goto();
      const openNavigation = async () => {
        if (viewport.width > 1007) return page.locator(".sidebar");
        await page.locator(".mail-topbar__menu").click();
        return page.getByRole("dialog", { name: "Mail navigation" });
      };

      await (await openNavigation())
        .getByRole("button", { name: "Drafts", exact: true }).click();
      await expect(page.getByRole("heading", { name: "Drafts", exact: true })).toBeVisible();
      await expect(page.locator(".draft-item")).toHaveCount(1);
      await expect(page.locator(".mail-shell > .detail")).toHaveCount(0);
      await expectNoHorizontalOverflow(page);
      expectDirectoryToStayMetadataOnly(fixture);

      await (await openNavigation())
        .getByRole("button", { name: "Sent", exact: true }).click();
      await expect(page.getByRole("heading", { name: "Sent", exact: true })).toBeVisible();
      await expect(page.locator(".sent-item")).toHaveCount(4);
      await expect(page.locator(".mail-shell > .detail")).toHaveCount(0);
      await expectNoHorizontalOverflow(page);
      expectDirectoryToStayMetadataOnly(fixture);
    }
  });

  test("has no detectable WCAG A or AA violations in Drafts or Sent", async ({ mailApp }) => {
    const { page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();

    await page.locator(".sidebar").getByRole("button", { name: "Drafts", exact: true }).click();
    await expect(page.locator(".drafts-pane")).toBeVisible();
    const draftsAccessibility = await new AxeBuilder({ page })
      .include(".drafts-pane")
      .withTags([...WCAG_22_AA_TAGS])
      .analyze();
    expect(draftsAccessibility.violations, JSON.stringify(draftsAccessibility.violations, null, 2))
      .toEqual([]);

    await page.locator(".sidebar").getByRole("button", { name: "Sent", exact: true }).click();
    await expect(page.locator(".sent-pane")).toBeVisible();
    const sentAccessibility = await new AxeBuilder({ page })
      .include(".sent-pane")
      .withTags([...WCAG_22_AA_TAGS])
      .analyze();
    expect(sentAccessibility.violations, JSON.stringify(sentAccessibility.violations, null, 2))
      .toEqual([]);
  });
});

test.describe("Mail accepted submission lifecycle", () => {
  test.use({
    mailFixtureOptions: {
      submissionTimeline: ACCEPTED_TO_PARTIAL_TIMELINE,
    },
  });

  test("moves from accepted to queued delivery evidence and a terminal partial outcome", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();
    const composer = await openComposerAndSend(page, "Partial delivery contract");

    await expect(composer.getByText(
      "Accepted by the mail server; retrieving final delivery status…",
      { exact: true },
    )).toBeVisible();
    await expect(composer.getByText(
      "Accepted by the mail server; delivered 0/2",
      { exact: true },
    )).toBeVisible();
    await expect(composer.getByText(
      "Delivery complete: 1 succeeded, 1 failed",
      { exact: true },
    )).toBeVisible();
    await expect(composer.getByRole("button", { name: "Sending locked", exact: true }))
      .toBeDisabled();
    expect(fixture.state.submissionRequestCount).toBe(1);
  });
});

test.describe("Mail manual-review submission lifecycle", () => {
  test.use({
    mailFixtureOptions: {
      submissionTimeline: PENDING_TO_MANUAL_REVIEW_TIMELINE,
    },
  });

  test("locks a queued submission when the server requires manual review", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();
    const composer = await openComposerAndSend(page, "Manual review contract");

    await expect(composer.getByText("Queued for sending…", { exact: true })).toBeVisible();
    await expect(composer.getByText(
      "The send status needs manual review; do not send it again",
      { exact: true },
    )).toBeVisible();
    const evidence = composer.locator(".composer-evidence");
    await expect(evidence)
      .toBeVisible();
    await expect(evidence).not.toHaveAttribute("open", "");
    await expect(evidence.locator("summary"))
      .toHaveText("Submission and delivery evidence");
    await expect(evidence.getByText("Submission ID", { exact: true })).toBeHidden();

    await evidence.locator("summary").click();
    await expect(evidence).toHaveAttribute("open", "");
    await expect(evidence.getByRole("img", {
      name: "A mail-processing route paused at a review gate",
    })).toBeVisible();
    await expect(evidence.getByText("Submission ID", { exact: true })).toBeVisible();
    await expect(evidence.getByText(SUBMISSION_BASE.submission_id, { exact: true }))
      .toBeVisible();
    await expect(evidence.getByText(
      "Copy the submission ID and contact an administrator; do not send this message again. Reopen this evidence with Compose after closing it.",
      { exact: true },
    )).toBeVisible();
    await page.setViewportSize(DESKTOP);
    await captureAt(page, DESKTOP, "mail-manual-review-evidence-1440.png");
    await evidence.locator("summary").click();
    await expect(evidence).not.toHaveAttribute("open", "");
    await expect(composer.getByRole("button", { name: "Sending locked", exact: true }))
      .toBeDisabled();
    expect(fixture.state.submissionRequestCount).toBe(1);

    await composer.getByRole("button", { name: "Close composer", exact: true }).click();
    await expect(composer).not.toBeVisible();
    await page.keyboard.press("c");

    const reopened = page.getByRole("dialog", { name: "New message", exact: true });
    await expect(reopened).toBeVisible();
    await expect(reopened.getByRole("textbox", { name: "To", exact: true }))
      .toHaveValue("one@recipient.example; two@recipient.example");
    await expect(reopened.getByRole("textbox", { name: "Subject", exact: true }))
      .toHaveValue("Manual review contract");
    await expect(reopened.getByRole("textbox", { name: "Message body", exact: true }))
      .toHaveValue(
        "This deterministic browser fixture exercises the server submission lifecycle.",
      );
    const reopenedEvidence = reopened.locator(".composer-evidence");
    await expect(reopenedEvidence).toBeVisible();
    await expect(reopenedEvidence).not.toHaveAttribute("open", "");
    await expect(reopenedEvidence.getByText(SUBMISSION_BASE.submission_id, { exact: true }))
      .toBeHidden();
    await expect(reopened.getByRole("button", { name: "Sending locked", exact: true }))
      .toBeDisabled();
    expect(fixture.state.draftSaveRequestCount).toBe(1);
    expect(fixture.state.submissionRequestCount).toBe(1);

    const accessibility = await new AxeBuilder({ page })
      .include(".composer")
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();
    expect(accessibility.violations).toEqual([]);

    await page.setViewportSize(DESKTOP);
    await expect(reopened.locator("footer")).toHaveScreenshot(
      "mail-manual-review-footer-1440.png",
      {
        animations: "disabled",
        caret: "hide",
        maxDiffPixels: 0,
        scale: "css",
      },
    );
    await captureAt(page, DESKTOP, "mail-manual-review-1440.png");

    await page.setViewportSize(TABLET);
    await captureAt(page, TABLET, "mail-manual-review-1008.png");

    await page.setViewportSize(MOBILE);
    await expect(reopened.locator("footer")).toHaveScreenshot(
      "mail-manual-review-footer-390.png",
      {
        animations: "disabled",
        caret: "hide",
        maxDiffPixels: 0,
        scale: "css",
      },
    );
    await captureAt(page, MOBILE, "mail-manual-review-390.png");
  });
});

test.describe("Mail rejected submission lifecycle", () => {
  test.use({
    mailFixtureOptions: {
      submissionTimeline: RECONCILING_TO_REJECTED_TIMELINE,
    },
  });

  test("locks a reconciling submission when the server rejects it", async ({ mailApp }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(DESKTOP);
    await mailApp.goto();
    const composer = await openComposerAndSend(page, "Rejected delivery contract");

    await expect(composer.getByText("Confirming the send result…", { exact: true }))
      .toBeVisible();
    await expect(composer.getByText(
      "The mail server rejected this message; contact an administrator",
      { exact: true },
    )).toBeVisible();
    await expect(composer.getByRole("button", { name: "Sending locked", exact: true }))
      .toBeDisabled();
    expect(fixture.state.submissionRequestCount).toBe(1);
  });
});

test("keeps the 390px inbox and detail mutually exclusive with deterministic focus", async ({
  mailApp,
}) => {
  const { page } = mailApp;
  await page.setViewportSize(MOBILE);
  await mailApp.goto();

  const list = page.locator(".mail-shell > .inbox");
  const detail = page.locator(".mail-shell > .detail");
  const message = page.getByRole("button", { name: LONG_SUBJECT });
  await expect(list).toBeVisible();
  await expect(detail).toBeHidden();

  await message.click();
  const heading = page.getByRole("heading", { name: LONG_SUBJECT, level: 2 });
  await expect(heading).toBeVisible();
  await expect(heading).toBeFocused();
  await expect(list).toBeHidden();
  await expect(detail).toBeVisible();
  expect(await page.evaluate(() =>
    document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);

  await page.getByRole("button", { name: "Back to inbox", exact: true }).click();
  await expect(list).toBeVisible();
  await expect(detail).toBeHidden();
  await expect(message).toBeFocused();

  await page.keyboard.press("Enter");
  await expect(heading).toBeVisible();
  await expect(heading).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(list).toBeVisible();
  await expect(detail).toBeHidden();
  await expect(message).toBeFocused();
  expect(await page.evaluate(() =>
    document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
});
