import AxeBuilder from "@axe-core/playwright";
import {
  expect,
  FIXTURE_MESSAGE,
  LONG_SUBJECT,
  MAIL_VISUAL_VIEWPORTS,
  SECOND_FIXTURE_MESSAGE,
  test,
  type MailApp,
} from "./mail-fixtures";

const WCAG_22_AA_TAGS = [
  "wcag2a",
  "wcag2aa",
  "wcag21a",
  "wcag21aa",
  "wcag22aa",
] as const;

async function openFixtureMessage(mailApp: MailApp) {
  const { page } = mailApp;
  const detailResponse = page.waitForResponse((response) =>
    response.request().method() === "GET"
      && new URL(response.url()).pathname.endsWith(`/messages/${FIXTURE_MESSAGE.message_id}`));
  const readStateResponse = page.waitForResponse((response) =>
    response.request().method() === "PATCH"
      && new URL(response.url()).pathname.endsWith(`/messages/${FIXTURE_MESSAGE.message_id}`));
  await page.getByRole("button", { name: LONG_SUBJECT }).click();
  await detailResponse;
  await readStateResponse;
  await expect(page.getByRole("heading", { name: LONG_SUBJECT, level: 2 })).toBeVisible();
}

test.describe("Mail built-artifact browser contracts", () => {
  for (const viewport of MAIL_VISUAL_VIEWPORTS) {
    test(`captures the authorized human mailbox — ${viewport.name}px`, async ({ mailApp }) => {
      const { page } = mailApp;
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await mailApp.goto();
      await openFixtureMessage(mailApp);

      const metadata = page.locator(".message-metadata");
      await expect(metadata.locator("dt")).toHaveText(["From", "Received", "To", "Cc"]);
      await expect(metadata.locator("dd").nth(0))
        .toHaveText("Jordan Li <jordan.li@partner.example>");
      await expect(metadata.locator("time"))
        .toHaveAttribute("datetime", FIXTURE_MESSAGE.received_at);
      await expect(metadata.locator("dd").nth(2)).toHaveText([
        "Priya Shah <priya.shah@copilotmail.test>",
        "Mateo Ortiz <mateo.ortiz@copilotmail.test>",
        "Nia Brooks <nia.brooks@copilotmail.test>",
      ].join(", "));
      await expect(metadata.locator("dd").nth(3))
        .toHaveText("Operations Review <ops-review@copilotmail.test>");
      const readingActions = page.locator(".reading-actions");
      await expect(readingActions).toHaveAttribute("role", "group");
      await expect(readingActions.getByRole("button", { name: "Reply", exact: true }))
        .toBeVisible();
      expect(await page.locator(".reading").evaluate((reading) => {
        const metadata = reading.querySelector(".message-metadata");
        const actions = reading.querySelector(".reading-actions");
        const body = reading.querySelector(".detail-body");
        if (!metadata || !actions || !body) return false;
        return Boolean(
          metadata.compareDocumentPosition(actions) & Node.DOCUMENT_POSITION_FOLLOWING,
        ) && Boolean(
          actions.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING,
        );
      })).toBe(true);
      await expect(page.getByText("routing-evidence-q1.pdf", { exact: true })).toBeVisible();
      // On the explicit mobile list-detail stack the queue is correctly
      // hidden after selection; desktop still proves the list preview.
      if (viewport.width > 1007) {
        await expect(page.getByText(FIXTURE_MESSAGE.preview, { exact: true })).toBeVisible();
        // A snapshot can accidentally bless an unstyled native button. Keep
        // the shared primary-control contract independently measurable.
        const compose = page.locator(".sidebar .compose");
        await expect(compose).toHaveClass(/\bbutton\b/);
        expect(await compose.evaluate((button) => button.getBoundingClientRect().height))
          .toBeGreaterThanOrEqual(40);
        await expect(page.locator(".detail-back")).toBeHidden();
      } else {
        await expect(page.locator(".detail-back")).toBeVisible();
      }
      expect(await page.locator("html").evaluate((html) => html.scrollWidth <= window.innerWidth))
        .toBe(true);
      await expect(page).toHaveScreenshot(`mail-inbox-${viewport.name}.png`, {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      });
    });
  }

  test("uses only readable three-column geometry and changes to the explicit stack before columns crowd", async ({
    mailApp,
  }) => {
    const { page } = mailApp;

    for (const expectation of [
      { width: 1440, rail: 264 },
      { width: 1084, rail: 216 },
      { width: 1008, rail: 216 },
    ]) {
      await page.setViewportSize({ width: expectation.width, height: 900 });
      await mailApp.goto();
      await expect(page.locator(".sidebar")).toBeVisible();
      await expect(page.locator(".mail-topbar__menu")).toBeHidden();
      await expect(page.locator(".compose-fab")).toBeHidden();

      const geometry = await page.locator(".mail-shell").evaluate((shell) => {
        const rail = shell.querySelector<HTMLElement>(".sidebar");
        const queue = shell.querySelector<HTMLElement>(".inbox");
        const reading = shell.querySelector<HTMLElement>(".detail");
        if (!rail || !queue || !reading) throw new Error("mail_shell_geometry_missing");
        return {
          documentWidth: document.documentElement.scrollWidth,
          queue: queue.getBoundingClientRect().width,
          rail: rail.getBoundingClientRect().width,
          reading: reading.getBoundingClientRect().width,
          viewportWidth: window.innerWidth,
        };
      });

      expect(geometry.rail).toBeCloseTo(expectation.rail, 0);
      expect(geometry.queue).toBeGreaterThanOrEqual(299.5);
      expect(geometry.reading).toBeGreaterThanOrEqual(459.5);
      expect(geometry.documentWidth).toBe(geometry.viewportWidth);
      expect(mailApp.fixture.state.detailRequestCount).toBe(0);
    }

    await page.setViewportSize({ width: 1007, height: 900 });
    await mailApp.goto();
    await expect(page.locator(".sidebar")).toBeHidden();
    await expect(page.locator(".mail-topbar__menu")).toBeVisible();
    await expect(page.locator(".compose-fab")).toBeVisible();
    await expect(page.locator(".mail-shell > .detail")).toBeHidden();
    await expect(page.getByText(FIXTURE_MESSAGE.preview, { exact: true })).toBeVisible();
    expect(mailApp.fixture.state.detailRequestCount).toBe(0);
    expect(await page.locator("html").evaluate((html) => html.scrollWidth <= window.innerWidth))
      .toBe(true);
  });

  test.describe("Queue scan hierarchy", () => {
    test.use({
      mailFixtureOptions: {
        messages: [FIXTURE_MESSAGE, SECOND_FIXTURE_MESSAGE],
      },
    });

    for (const viewport of [
      { name: "1440", width: 1440, height: 1024 },
      { name: "1008", width: 1008, height: 900 },
      { name: "390", width: 390, height: 844 },
    ]) {
      test(`keeps scanning local until explicit open — ${viewport.name}px`, async ({ mailApp }) => {
        const { fixture, page } = mailApp;
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
        await mailApp.goto();

        const first = page.getByRole("button", { name: LONG_SUBJECT });
        const second = page.getByRole("button", { name: SECOND_FIXTURE_MESSAGE.subject });
        await page.keyboard.press("j");
        await expect(first).toHaveAttribute("aria-current", "true");
        await page.keyboard.press("j");
        await expect(second).toHaveAttribute("aria-current", "true");
        await expect(page.locator(".message.selected")).toHaveCount(1);
        await expect(page.locator(".message.unread")).toHaveCount(1);
        expect(fixture.state.detailRequestCount).toBe(0);
        expect(fixture.state.messageStateRequestCount).toBe(0);
        expect(await page.locator("html").evaluate((html) => html.scrollWidth <= window.innerWidth))
          .toBe(true);

        await expect(page).toHaveScreenshot(`mail-queue-scan-${viewport.name}.png`, {
          animations: "disabled",
          caret: "hide",
          fullPage: true,
          scale: "css",
        });
      });
    }
  });

  test("keeps the mobile scope factual when closed and reveals low-frequency controls only on demand", async ({
    mailApp,
  }) => {
    const { page } = mailApp;
    await page.setViewportSize({ width: 390, height: 844 });
    await mailApp.goto();

    await page.locator(".mail-topbar__menu").click();
    const scope = page.getByRole("dialog", { name: "Mail navigation" })
      .locator(".mobile-mailbox-scope");
    const summary = scope.locator("summary");
    await expect(scope).not.toHaveAttribute("open", "");
    await expect(summary).toContainText("alex.chen@copilotmail.test");
    await expect(summary).toContainText("Personal");
    await expect(summary).toContainText("Can send email");
    await expect(summary.locator("[data-icon='chevron-down']")).toBeVisible();
    expect(mailApp.fixture.state.detailRequestCount).toBe(0);

    await summary.click();

    await expect(scope).toHaveAttribute("open", "");
    await expect(scope.getByRole("combobox", { name: "Switch mailbox" })).toBeVisible();
    await expect(scope.getByRole("combobox", { name: "Language" })).toBeVisible();
    await expect(scope.getByRole("button", { name: "Sign out" })).toBeVisible();
    await expect(scope.getByText("Sign out", { exact: true })).toBeVisible();
    expect(mailApp.fixture.state.detailRequestCount).toBe(0);
    await expect(page).toHaveScreenshot("mail-mobile-scope-390.png", {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });
  });

  test("keeps the tablet mobile scope factual at its four-column summary breakpoint", async ({
    mailApp,
  }) => {
    const { page } = mailApp;
    await page.setViewportSize({ width: 768, height: 900 });
    await mailApp.goto();

    await page.locator(".mail-topbar__menu").click();
    const scope = page.getByRole("dialog", { name: "Mail navigation" })
      .locator(".mobile-mailbox-scope");
    const summary = scope.locator("summary");
    await expect(summary).toContainText("alex.chen@copilotmail.test");
    await expect(summary).toContainText("Personal");
    await expect(summary).toContainText("Can send email");
    await summary.click();
    await expect(scope).toHaveAttribute("open", "");
    await expect(scope.getByRole("combobox", { name: "Switch mailbox" })).toBeVisible();
    await expect(scope.getByRole("combobox", { name: "Language" })).toBeVisible();
    await expect(scope.getByText("Sign out", { exact: true })).toBeVisible();
    expect(mailApp.fixture.state.detailRequestCount).toBe(0);
    expect(await page.locator("html").evaluate((html) => html.scrollWidth <= window.innerWidth))
      .toBe(true);
    await expect(page).toHaveScreenshot("mail-mobile-scope-768.png", {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });
  });

  test("completes the keyboard-first inbox, compose, and view workflow", async ({
    mailApp,
  }) => {
    const { page } = mailApp;
    await mailApp.goto();

    await page.keyboard.press("/");
    const search = page.getByRole("textbox", { name: "Search email", exact: true });
    await expect(search).toBeFocused();

    const commandTrigger = page.getByRole("button", {
      name: "Open command palette",
      exact: true,
    });
    await commandTrigger.focus();
    await page.keyboard.press("c");
    const composer = page.getByRole("dialog", { name: "New message", exact: true });
    await expect(composer).toBeVisible();
    await expect(composer.getByRole("textbox", { name: "To", exact: true })).toBeFocused();
    const composerContext = composer.getByRole("region", { name: "Compose context", exact: true });
    await expect(composerContext).toBeVisible();
    const contextFacts = composerContext.locator(".context-bar__fact");
    const sendingScope = contextFacts.filter({ hasText: "Sending scope" });
    await expect(sendingScope).toHaveCount(1);
    await expect(sendingScope.locator("dt")).toHaveText("Sending scope");
    await expect(sendingScope.locator("dd")).toHaveText("alex.chen@copilotmail.test");
    const sendingIdentity = contextFacts.filter({ hasText: "Sending identity" });
    await expect(sendingIdentity).toHaveCount(1);
    await expect(sendingIdentity.locator("dt")).toHaveText("Sending identity");
    await expect(sendingIdentity.locator("dd")).toHaveText("Personal");
    await expect(contextFacts).toHaveCount(2);
    await expect(composerContext.locator(".context-bar__facts"))
      .not.toContainText("Server status");
    const serverStatus = composerContext.locator(".context-bar__status [data-sendable='true']");
    await expect(serverStatus).toHaveCount(1);
    await expect(serverStatus).toHaveText(
      "Server status: this mailbox can send email.",
    );
    await expect(serverStatus).not.toHaveAttribute("aria-live");
    await expect(composer.locator(".composer-evidence")).toHaveCount(0);
    await expect(composer).not.toContainText(/owner|delegat|on behalf|actor/i);

    await page.keyboard.press("Escape");
    await expect(composer).toBeHidden();
    await expect(commandTrigger).toBeFocused();

    await page.keyboard.press("g");
    await page.keyboard.press("d");
    await expect(page.getByRole("heading", { name: "Drafts", exact: true })).toBeVisible();

    await page.keyboard.press("g");
    await page.keyboard.press("i");
    await expect(page.getByRole("heading", { name: "Inbox", exact: true })).toBeVisible();
  });

  test("has no detectable WCAG A or AA violations in the authenticated inbox", async ({
    mailApp,
  }) => {
    await mailApp.goto();

    const results = await new AxeBuilder({ page: mailApp.page })
      .withTags([...WCAG_22_AA_TAGS])
      .analyze();

    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
  });

  test("has no detectable WCAG A or AA violations after an explicit Reading open", async ({
    mailApp,
  }) => {
    await mailApp.goto();
    await openFixtureMessage(mailApp);

    const results = await new AxeBuilder({ page: mailApp.page })
      .withTags([...WCAG_22_AA_TAGS])
      .analyze();

    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
  });
});
