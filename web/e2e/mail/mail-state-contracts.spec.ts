import AxeBuilder from "@axe-core/playwright";
import {
  AUTHORIZED_HUMAN_MAILBOX,
  expect,
  FIXTURE_MESSAGE,
  LONG_SUBJECT,
  MAIL_VISUAL_VIEWPORTS,
  SECOND_FIXTURE_MESSAGE,
  test,
} from "./mail-fixtures";

async function expectMinimumTouchTarget(
  locator: import("@playwright/test").Locator,
  label: string,
) {
  await expect(locator, `${label} must be visible before measuring`).toBeVisible();
  const box = await locator.boundingBox();
  expect(box, `${label} must have a rendered bounding box`).not.toBeNull();
  expect(box!.width, `${label} width`).toBeGreaterThanOrEqual(44);
  expect(box!.height, `${label} height`).toBeGreaterThanOrEqual(44);
}

test("renders an opaque focus boundary and a static reduced-motion spinner", async ({
  mailApp,
}) => {
  const { page } = mailApp;
  await mailApp.goto();

  const compose = page.locator(".sidebar").getByRole("button", {
    name: "Compose",
    exact: true,
  });
  // `:focus-visible` is intentionally input-modality aware. Begin with no
  // focused control, then reach Compose through the real keyboard tab order.
  // A DevTools `focus()` call is deliberately avoided: Chromium is permitted
  // to retain pointer modality for that call, which makes a visual focus-ring
  // test nondeterministic even though focus itself moved.
  await page.evaluate(() => {
    const active = document.activeElement;
    if (active instanceof HTMLElement) active.blur();
  });
  for (let attempt = 0; attempt < 12; attempt += 1) {
    if (await compose.evaluate((element) => document.activeElement === element)) break;
    await page.keyboard.press("Tab");
  }
  await expect(compose).toBeFocused();
  await expect.poll(() => compose.evaluate(
    (element) => getComputedStyle(element).boxShadow,
  )).not.toBe("rgba(0, 0, 0, 0) 0px 0px 0px 0px, rgba(0, 0, 0, 0) 0px 0px 0px 0px");
  const focusShadow = await compose.evaluate((element) => getComputedStyle(element).boxShadow);
  expect(focusShadow).toContain("rgb(255, 255, 255)");
  expect(focusShadow).toContain("rgb(15, 118, 110)");
  expect(focusShadow).not.toContain("rgba(15, 118, 110, 0.");

  const spinnerState = await page.evaluate(async () => {
    const spinner = document.createElement("span");
    spinner.className = "spinner";
    document.body.append(spinner);
    const before = getComputedStyle(spinner);
    const initialTransform = before.transform;
    const animationName = before.animationName;
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(
      () => resolve(),
    )));
    const finalTransform = getComputedStyle(spinner).transform;
    spinner.remove();
    return { animationName, finalTransform, initialTransform };
  });
  expect(spinnerState.animationName).toBe("none");
  expect(spinnerState.finalTransform).toBe(spinnerState.initialTransform);
});

test.describe("Mail localized browser state", () => {
  test("switches the authenticated shell to Simplified Chinese", async ({ mailApp }) => {
    const { page } = mailApp;
    await mailApp.goto();

    await page.locator(".sidebar").getByRole("combobox", { name: "Language" })
      .selectOption("zh-CN");

    await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
    await expect(page.getByRole("heading", { name: "收件箱", exact: true })).toBeVisible();
    await expect(page.getByRole("textbox", { name: "搜索邮件", exact: true })).toBeVisible();
    await expect(page.locator(".sidebar").getByRole("button", { name: "写信", exact: true }))
      .toBeVisible();
    await page.locator(".sidebar").getByRole("button", { name: "写信", exact: true }).click();
    const composer = page.getByRole("dialog", { name: "新邮件", exact: true });
    const composerContext = composer.getByRole("region", { name: "写信上下文", exact: true });
    await expect(composerContext).toBeVisible();
    await expect(composerContext.getByText("发件范围", { exact: true })).toBeVisible();
    await expect(composerContext.getByText("个人", { exact: true })).toBeVisible();
    await expect(composerContext.locator(".context-bar__fact")).toHaveCount(2);
    await expect(composerContext.locator(".context-bar__facts"))
      .not.toContainText("服务器状态");
    await expect(composerContext.locator(".context-bar__status [data-sendable='true']"))
      .toContainText("服务器返回：此邮箱可发送邮件。");
    await expect(composerContext.locator(".context-bar__status [aria-live]")).toHaveCount(0);
    await expect(composer.locator(".composer-evidence")).toHaveCount(0);
    await composer.getByRole("button", { name: "关闭写信窗口", exact: true }).click();
    await expect.poll(() => page.evaluate(() =>
      window.localStorage.getItem("copilotmail.mail.locale.v1")))
      .toBe("zh-CN");

    for (const viewport of MAIL_VISUAL_VIEWPORTS) {
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await expect(page).toHaveScreenshot(`mail-zh-cn-${viewport.name}.png`, {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      });
    }
  });
});

test("keeps the normal Compose context strip factual and compact at D4D widths", async ({
  mailApp,
}) => {
  const { fixture, page } = mailApp;
  const viewports = [
    { name: "1440", width: 1440, height: 1024 },
    { name: "1008", width: 1008, height: 900 },
    { name: "390", width: 390, height: 844 },
  ] as const;

  for (const viewport of viewports) {
    await page.setViewportSize(viewport);
    await mailApp.goto();
    const trigger = viewport.width > 1007
      ? page.locator(".sidebar").getByRole("button", { name: "Compose", exact: true })
      : page.locator(".mobile-toolbar").getByRole("button", { name: "Compose", exact: true });
    await trigger.click();

    const composer = page.getByRole("dialog", { name: "New message", exact: true });
    const context = composer.getByRole("region", { name: "Compose context", exact: true });
    await expect(context.locator(".context-bar__fact")).toHaveCount(2);
    await expect(context.locator(".context-bar__facts"))
      .not.toContainText("Server status:");
    const sendability = context.locator(".context-bar__status [data-sendable='true']");
    await expect(sendability).toHaveText("Server status: this mailbox can send email.");
    await expect(sendability).not.toHaveAttribute("aria-live");
    await expect(composer.locator(".composer-evidence")).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => (
      document.documentElement.scrollWidth <= document.documentElement.clientWidth
    ))).toBe(true);

    if (viewport.name === "1440") {
      const accessibility = await new AxeBuilder({ page })
        .include(".composer")
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
        .analyze();
      expect(accessibility.violations).toEqual([]);
    }

    await expect(composer).toHaveScreenshot(`mail-compose-context-${viewport.name}.png`, {
      animations: "disabled",
      caret: "hide",
      maxDiffPixels: 0,
      scale: "css",
    });
    await composer.getByRole("button", { name: "Close composer", exact: true }).click();
  }

  expect(fixture.state.draftSaveRequestCount).toBe(0);
  expect(fixture.state.submissionRequestCount).toBe(0);
});

test.describe("Mail server-authoritative read-only state", () => {
  test.use({
    mailFixtureOptions: {
      mailbox: {
        ...AUTHORIZED_HUMAN_MAILBOX,
        kind: "shared",
        primary_address: "operations-shared@copilotmail.test",
        can_send: false,
      },
    },
  });

  test("keeps a Shared Mailbox readable while every send path stays closed", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize({ width: 1440, height: 1024 });
    await mailApp.goto();

    const identity = page.locator(".sidebar .mailbox-identity");
    await expect(identity.getByText("Shared", { exact: true })).toBeVisible();
    await expect(identity.getByText("operations-shared@copilotmail.test", { exact: true }))
      .toBeVisible();
    await expect(identity.getByText("This mailbox is read-only", { exact: true })).toBeVisible();
    await expect(page.locator(".sidebar .compose")).toBeDisabled();
    await expect(page.locator(".sidebar").getByRole("button", { name: "Drafts", exact: true }))
      .toBeDisabled();
    await expect(page.locator(".sidebar").getByRole("button", { name: "Sent", exact: true }))
      .toBeDisabled();

    await expect(page).toHaveScreenshot("mail-readonly-shared-1440.png", {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page).toHaveScreenshot("mail-readonly-shared-390.png", {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });

    await page.getByRole("button", { name: LONG_SUBJECT }).click();
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toBeVisible();
    await expect(page.getByRole("button", { name: "Reply", exact: true })).toBeDisabled();

    await page.keyboard.press("c");
    await page.keyboard.press("r");
    await page.keyboard.press("g");
    await page.keyboard.press("s");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toBeVisible();
    await expect(page.locator(".mobile-view-tabs").getByRole("button", {
      name: "Inbox",
      exact: true,
    })).toHaveClass(/active/);
    expect(fixture.state.draftListRequestCount).toBe(0);
    expect(fixture.state.sentListRequestCount).toBe(0);
    expect(fixture.state.submissionRequestCount).toBe(0);
  });
});

test.describe("Mail bounded empty and failure states", () => {
  test.describe("empty inbox", () => {
    test.use({ mailFixtureOptions: { messages: [] } });

    test("renders a real contextual zero state", async ({ mailApp }) => {
      const { page } = mailApp;
      await page.setViewportSize({ width: 1440, height: 1024 });
      await mailApp.goto();

      await expect(page.getByText("Your inbox is empty", { exact: true })).toBeVisible();
      await expect(page.getByText("There are no messages to process right now.", { exact: true }))
        .toBeVisible();
      await expect(page.getByRole("img", { name: "An empty mail tray", exact: true }))
        .toBeVisible();
      await expect(page.locator(".messages .message")).toHaveCount(0);
      await expect(page).toHaveScreenshot("mail-empty-inbox-1440.png", {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      });
      await page.setViewportSize({ width: 390, height: 844 });
      await expect(page).toHaveScreenshot("mail-empty-inbox-390.png", {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      });
    });
  });

  test.describe("recoverable inbox error", () => {
    test.use({ mailFixtureOptions: { inboxFailuresBeforeSuccess: 1 } });

    test("shows a bounded error and recovers through the visible retry action", async ({
      mailApp,
    }) => {
      const { fixture, page } = mailApp;
      await page.setViewportSize({ width: 1440, height: 1024 });
      await mailApp.goto();

      await expect(page.getByRole("alert")).toHaveText("The inbox is temporarily unavailableRetry");
      await expect(page.getByText("fixture_inbox_upstream_failure", { exact: true }))
        .toHaveCount(0);
      await expect(page).toHaveScreenshot("mail-inbox-error-1440.png", {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      });
      await page.setViewportSize({ width: 390, height: 844 });
      await expect(page).toHaveScreenshot("mail-inbox-error-390.png", {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      });
      await page.getByRole("button", { name: "Retry", exact: true }).click();
      await expect(page.getByRole("button", { name: LONG_SUBJECT })).toBeVisible();
      expect(fixture.state.inboxRequestCount).toBe(2);
    });
  });
});

test.describe("Mail unknown-submission freeze", () => {
  test.use({ mailFixtureOptions: { submissionOutcome: "network-unknown" } });

  test("freezes the exact draft after an ambiguous submission response", async ({ mailApp }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize({ width: 1440, height: 1024 });
    await mailApp.goto();

    await page.locator(".sidebar .compose").click();
    const composer = page.getByRole("dialog", { name: "New message", exact: true });
    await composer.getByRole("textbox", { name: "To", exact: true })
      .fill("reviewer@copilotmail.test");
    await composer.getByRole("textbox", { name: "Subject", exact: true })
      .fill("Frozen delivery evidence");
    await composer.getByRole("textbox", { name: "Message body", exact: true })
      .fill("This content must remain immutable until reconciliation completes.");
    await composer.getByRole("button", { name: "Send", exact: true }).click();

    const unknownSubmissionStatus =
      "The send request status is unknown; confirm the same submission again—no duplicate email will be created";
    await expect(composer.getByText(unknownSubmissionStatus, { exact: true })).toBeVisible();
    await expect(composer.getByRole("textbox", { name: "To", exact: true })).toBeDisabled();
    await expect(composer.getByRole("textbox", { name: "Subject", exact: true }))
      .toBeDisabled();
    await expect(composer.getByRole("textbox", { name: "Message body", exact: true }))
      .toBeDisabled();
    await expect(composer.getByRole("button", {
      name: "Confirm same submission (no duplicate email)",
      exact: true,
    }))
      .toBeEnabled();
    await expect(page.locator(".sidebar .mailbox-picker select"))
      .toBeDisabled();
    expect(fixture.state.submissionRequestCount).toBe(1);

    const unknownStatusLabel = composer.locator(
      ".composer-status .status-indicator__label",
    );
    for (const viewport of [
      { width: 1440, height: 1024 },
      { width: 1008, height: 900 },
      { width: 390, height: 844 },
    ]) {
      await page.setViewportSize(viewport);
      await expect(unknownStatusLabel).toHaveText(unknownSubmissionStatus);
      const layout = await unknownStatusLabel.evaluate((label) => {
        const footer = label.closest("footer");
        if (!footer) throw new Error("unknown_submission_status_footer_missing");
        const style = getComputedStyle(label);
        return {
          footerClientWidth: footer.clientWidth,
          footerScrollWidth: footer.scrollWidth,
          labelClientWidth: label.clientWidth,
          labelScrollWidth: label.scrollWidth,
          overflowWrap: style.overflowWrap,
          whiteSpace: style.whiteSpace,
        };
      });
      expect(layout.labelClientWidth).toBeGreaterThan(0);
      expect(layout.labelScrollWidth).toBeLessThanOrEqual(layout.labelClientWidth);
      expect(layout.footerScrollWidth).toBeLessThanOrEqual(layout.footerClientWidth);
      expect(layout.whiteSpace).toBe("normal");
      expect(layout.overflowWrap).toBe("anywhere");
      await expect.poll(() => page.evaluate(() => (
        document.documentElement.scrollWidth <= document.documentElement.clientWidth
      ))).toBe(true);
    }

    await page.setViewportSize({ width: 1440, height: 1024 });
    await expect(page).toHaveScreenshot("mail-unknown-submission-1440.png", {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });
    await page.setViewportSize({ width: 1008, height: 900 });
    await expect(page).toHaveScreenshot("mail-unknown-submission-1008.png", {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page).toHaveScreenshot("mail-unknown-submission-390.png", {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });
    await page.setViewportSize({ width: 1440, height: 1024 });

    await composer.getByRole("button", { name: "Close composer", exact: true }).click();
    await page.keyboard.press("c");
    const reopened = page.getByRole("dialog", { name: "New message", exact: true });
    await expect(reopened.getByRole("textbox", { name: "Subject", exact: true }))
      .toHaveValue("Frozen delivery evidence");
    await expect(reopened.getByRole("textbox", { name: "To", exact: true }))
      .toHaveValue("reviewer@copilotmail.test");
    await expect(reopened.getByRole("textbox", { name: "Message body", exact: true }))
      .toHaveValue("This content must remain immutable until reconciliation completes.");
    await expect(reopened.getByRole("textbox", { name: "Subject", exact: true }))
      .toBeDisabled();

    await reopened.getByRole("button", {
      name: "Confirm same submission (no duplicate email)",
      exact: true,
    }).click();
    await expect.poll(() => fixture.state.submissionRequestCount).toBe(2);
    expect(fixture.state.draftSaveRequestCount).toBe(1);
    expect(fixture.state.submissionRequests).toHaveLength(2);
    expect(fixture.state.submissionRequests[1]).toEqual(
      fixture.state.submissionRequests[0],
    );
  });
});

test.describe("Mail complete inbox keyboard path", () => {
  test.use({
    mailFixtureOptions: {
      messages: [FIXTURE_MESSAGE, SECOND_FIXTURE_MESSAGE],
    },
  });

  test("supports j, k, Enter, r, s, and g s without a pointer", async ({ mailApp }) => {
    const { page } = mailApp;
    await mailApp.goto();
    const first = page.getByRole("button", { name: LONG_SUBJECT });
    const second = page.getByRole("button", { name: SECOND_FIXTURE_MESSAGE.subject });

    await page.keyboard.press("j");
    await expect(first).toHaveAttribute("aria-current", "true");
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toHaveCount(0);

    await page.keyboard.press("j");
    await expect(second).toHaveAttribute("aria-current", "true");
    await expect(page.getByRole("heading", {
      name: SECOND_FIXTURE_MESSAGE.subject,
      level: 2,
    })).toHaveCount(0);

    await page.keyboard.press("k");
    await expect(first).toHaveAttribute("aria-current", "true");
    expect(mailApp.fixture.state.detailRequestCount).toBe(0);
    expect(mailApp.fixture.state.messageStateRequestCount).toBe(0);

    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toBeVisible();
    await expect.poll(() => mailApp.fixture.state.detailRequestCount).toBe(1);
    await expect.poll(() => mailApp.fixture.state.messageStateRequestCount).toBe(1);

    await page.keyboard.press("r");
    const reply = page.getByRole("dialog", { name: "Reply", exact: true });
    await expect(reply.getByRole("textbox", { name: "To", exact: true }))
      .toHaveValue("jordan.li@partner.example");
    await expect(reply.getByRole("textbox", { name: "Subject", exact: true }))
      .toHaveValue(`Re: ${LONG_SUBJECT}`);
    await reply.getByRole("button", { name: "Close composer", exact: true }).click();

    await page.keyboard.press("s");
    await expect(page.getByRole("button", { name: "Remove star", exact: true })).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(first).toHaveAttribute("aria-current", "true");
    await expect(first).toBeFocused();
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toHaveCount(0);
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toBeVisible();

    await page.keyboard.press("g");
    await page.keyboard.press("s");
    await expect(page.getByRole("heading", { name: "Sent", exact: true })).toBeVisible();
  });

  test("keeps keyboard cursor movement in the narrow queue until Enter", async ({ mailApp }) => {
    const { page } = mailApp;
    await page.setViewportSize({ width: 390, height: 844 });
    await mailApp.goto();
    const first = page.getByRole("button", { name: LONG_SUBJECT });

    await page.keyboard.press("j");
    await expect(first).toHaveAttribute("aria-current", "true");
    await expect(page.locator(".inbox")).toBeVisible();
    await expect(page.locator(".detail")).toBeHidden();
    expect(mailApp.fixture.state.detailRequestCount).toBe(0);
    expect(mailApp.fixture.state.messageStateRequestCount).toBe(0);

    await page.keyboard.press("Enter");
    await expect(page.locator(".inbox")).toBeHidden();
    await expect(page.locator(".detail")).toBeVisible();
    await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toBeVisible();
  });
});

test.describe("Mail narrow-screen interaction cost", () => {
  test("keeps implemented controls at least 44px with the default fine pointer", async ({
    mailApp,
  }) => {
    const { page } = mailApp;
    await page.setViewportSize({ width: 390, height: 844 });
    await mailApp.goto();

    const toolbar = page.locator(".mobile-toolbar");
    for (const [label, locator] of [
      ["compose", toolbar.getByRole("button", { name: "Compose" })],
      ["command palette", toolbar.getByRole("button", { name: "Open command palette" })],
      ["Inbox tab", toolbar.getByRole("button", { name: "Inbox" })],
      ["Sent tab", toolbar.getByRole("button", { name: "Sent" })],
      ["Drafts tab", toolbar.getByRole("button", { name: "Drafts" })],
      ["search field", page.getByRole("textbox", { name: "Search email" })],
      ["search submit", page.getByRole("button", { name: "Search", exact: true })],
      ["inbox refresh", page.getByRole("button", { name: "Refresh", exact: true })],
    ] as const) {
      await expectMinimumTouchTarget(locator, label);
    }

    const mailboxScope = toolbar.locator(".mobile-mailbox-scope");
    const mailboxScopeSummary = mailboxScope.locator("summary");
    await expectMinimumTouchTarget(mailboxScopeSummary, "mailbox scope disclosure");
    await mailboxScopeSummary.click();
    await expect(mailboxScope).toHaveAttribute("open", "");
    for (const [label, locator] of [
      ["mailbox selector", toolbar.getByRole("combobox", { name: "Switch mailbox" })],
      ["language selector", toolbar.getByRole("combobox", { name: "Language" })],
      ["sign out", toolbar.getByRole("button", { name: "Sign out" })],
    ] as const) {
      await expectMinimumTouchTarget(locator, label);
    }

    await toolbar.getByRole("button", { name: "Open command palette" }).click();
    const commandDialog = page.getByRole("dialog", { name: "Command palette" });
    const commandSearch = page.getByRole("combobox", { name: "Search commands" });
    await expect(commandSearch).toBeFocused();
    await expect(page.locator(".mail-shell")).toHaveAttribute("inert", "");
    await expect(page.locator(".mail-shell")).toHaveAttribute("aria-hidden", "true");
    expect(await commandDialog.evaluate((element) => (
      element.parentElement?.parentElement === document.body
      && element.closest("[inert], [aria-hidden='true']") === null
    ))).toBe(true);
    await expectMinimumTouchTarget(
      page.getByRole("button", { name: "Close command palette" }),
      "command palette close",
    );
    await expectMinimumTouchTarget(
      commandSearch,
      "command palette search",
    );
    await expectMinimumTouchTarget(
      page.getByRole("option", { name: "Compose" }),
      "command palette option",
    );
    await page.keyboard.press("Escape");
    await expect(page.locator(".mail-shell")).not.toHaveAttribute("inert", "");
    await expect(page.locator(".mail-shell")).not.toHaveAttribute("aria-hidden", "true");

    await page.getByRole("button", { name: LONG_SUBJECT }).click();
    await expectMinimumTouchTarget(
      page.getByRole("button", { name: "Back to inbox" }),
      "detail back",
    );
    await expectMinimumTouchTarget(
      page.getByRole("button", { name: "Add star" }),
      "detail star",
    );
    await expectMinimumTouchTarget(
      page.getByRole("button", { name: "Reply" }),
      "detail reply",
    );

    await page.getByRole("button", { name: "Reply" }).click();
    const composer = page.getByRole("dialog", { name: "Reply" });
    await expect(composer.getByRole("region", { name: "Compose context" })).toBeVisible();
    await expect(composer.locator(".composer-evidence")).toHaveCount(0);
    await expect.poll(() => page.evaluate(() =>
      document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    )).toBe(true);
    await expectMinimumTouchTarget(
      composer.getByRole("button", { name: "Close composer" }),
      "composer close",
    );
    await expectMinimumTouchTarget(
      composer.getByRole("button", { name: "Send" }),
      "composer send",
    );
  });
});
