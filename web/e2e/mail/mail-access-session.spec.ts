import AxeBuilder from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./mail-fixtures";

const ACCESS_VIEWPORTS = [
  { name: "1440", width: 1440, height: 1024 },
  { name: "901", width: 901, height: 900 },
  { name: "390", width: 390, height: 844 },
] as const;

const WCAG_22_AA_TAGS = [
  "wcag2a",
  "wcag2aa",
  "wcag21a",
  "wcag21aa",
  "wcag22aa",
] as const;

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

async function expectAccessAxe(page: Page, selector: string) {
  const results = await new AxeBuilder({ page })
    .include(selector)
    .withTags([...WCAG_22_AA_TAGS])
    .analyze();
  expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
}

async function expectMinimumTouchTarget(locator: Locator, label: string) {
  await expect(locator, `${label} must be visible before measuring`).toBeVisible();
  const box = await locator.boundingBox();
  expect(box, `${label} must have a rendered bounding box`).not.toBeNull();
  expect(box!.width, `${label} width`).toBeGreaterThanOrEqual(44);
  expect(box!.height, `${label} height`).toBeGreaterThanOrEqual(44);
}

test.describe("Mail access entry", () => {
  test.use({ mailFixtureOptions: { sessionResponses: [{ kind: "signed-out" }] } });

  test("keeps signed-out access as a single intentional entry surface", async ({ mailApp }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(ACCESS_VIEWPORTS[0]);
    await mailApp.open();

    const access = page.locator("[data-mail-access='login']");
    const signIn = access.getByRole("button", {
      name: "Sign in with your organization account",
      exact: true,
    });
    await expect(access).toBeVisible();
    await expect(access.getByRole("heading", { name: "IMail", exact: true })).toBeVisible();
    await expect(access.locator(".mail-access__visual img")).toHaveAttribute("alt", "");
    await expect(page.locator("[data-mail-access='directory']")).toHaveCount(0);
    await expect(page.locator(".mail-shell")).toHaveCount(0);
    expect(fixture.state.sessionRequestCount).toBe(1);
    expect(fixture.state.mailboxDirectoryRequestCount).toBe(0);
    expect(fixture.state.inboxRequestCount).toBe(0);
    await expectAccessAxe(page, "[data-mail-access='login']");

    for (const viewport of ACCESS_VIEWPORTS) {
      await page.setViewportSize(viewport);
      await expect(access).toBeVisible();
      if (viewport.width <= 900) {
        await expect(access.locator(".mail-access__visual")).toBeHidden();
      } else {
        await expect(access.locator(".mail-access__visual")).toBeVisible();
      }
      await expectNoHorizontalOverflow(page);
      await captureAt(page, viewport, `mail-access-signed-out-${viewport.name}.png`);
    }

    await expectMinimumTouchTarget(signIn, "sign-in action");
  });
});

test.describe("Mail localized access entry", () => {
  test.use({
    mailFixtureOptions: {
      locale: "zh-CN",
      sessionResponses: [{ kind: "signed-out" }],
    },
  });

  test("keeps the entry fact and action readable in Simplified Chinese", async ({ mailApp }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(ACCESS_VIEWPORTS[0]);
    await mailApp.open();

    const access = page.locator("[data-mail-access='login']");
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
    await expect(access.getByText("你的工作邮件，清晰而安静。", { exact: true })).toBeVisible();
    await expect(access.getByRole("button", { name: "使用企业账号登录", exact: true }))
      .toBeVisible();
    expect(fixture.state.mailboxDirectoryRequestCount).toBe(0);
    expect(fixture.state.inboxRequestCount).toBe(0);
    await expectAccessAxe(page, "[data-mail-access='login']");

    for (const viewport of ACCESS_VIEWPORTS) {
      await page.setViewportSize(viewport);
      await expectNoHorizontalOverflow(page);
      await captureAt(page, viewport, `mail-access-signed-out-zh-CN-${viewport.name}.png`);
    }
  });
});

test.describe("Mail authorized-directory access", () => {
  test.describe("confirmed empty directory", () => {
    test.use({
      mailFixtureOptions: {
        mailboxDirectoryResponse: {
          kind: "success",
          page: { items: [], next_cursor: null, has_more: false },
        },
      },
    });

    test("does not turn an empty authorized directory into a login screen", async ({ mailApp }) => {
      const { fixture, page } = mailApp;
      await page.setViewportSize(ACCESS_VIEWPORTS[0]);
      await mailApp.open();

      const access = page.locator("[data-mail-access='directory']");
      await expect(access).toBeVisible();
      await expect(access.getByRole("heading", {
        name: "Your organization mailboxes",
        exact: true,
      })).toBeVisible();
      await expect(access.getByRole("alert")).toHaveText(
        "Your account has no accessible mailbox; contact an administrator",
      );
      await expect(access.locator(".mail-access__visual")).toHaveCount(0);
      await expect(page.locator("[data-mail-access='login']")).toHaveCount(0);
      await expect(page.locator(".mail-shell")).toHaveCount(0);
      expect(fixture.state.sessionRequestCount).toBe(1);
      expect(fixture.state.mailboxDirectoryRequestCount).toBe(1);
      expect(fixture.state.inboxRequestCount).toBe(0);
      await expectAccessAxe(page, "[data-mail-access='directory']");

      for (const viewport of ACCESS_VIEWPORTS) {
        await captureAt(page, viewport, `mail-access-directory-empty-${viewport.name}.png`);
        await expectNoHorizontalOverflow(page);
      }

      await expectMinimumTouchTarget(
        access.getByRole("button", { name: "Sign out", exact: true }),
        "directory sign-out action",
      );
    });
  });

  test.describe("temporarily unavailable directory", () => {
    test.use({
      mailFixtureOptions: { mailboxDirectoryResponse: { kind: "unavailable" } },
    });

    test("offers only the existing directory retry and sign-out actions", async ({ mailApp }) => {
      const { fixture, page } = mailApp;
      await page.setViewportSize(ACCESS_VIEWPORTS[0]);
      await mailApp.open();

      const access = page.locator("[data-mail-access='directory']");
      await expect(access.getByRole("alert")).toHaveText(
        "Unable to verify the mailboxes you can access",
      );
      await expect(access.getByRole("button", { name: "Retry", exact: true })).toBeVisible();
      await expect(access.getByRole("button", { name: "Sign out", exact: true })).toBeVisible();
      await expect(access.locator(".mail-access__visual")).toHaveCount(0);
      await expect(page.locator(".mail-shell")).toHaveCount(0);
      expect(fixture.state.sessionRequestCount).toBe(1);
      expect(fixture.state.mailboxDirectoryRequestCount).toBe(1);
      expect(fixture.state.inboxRequestCount).toBe(0);
      await expectAccessAxe(page, "[data-mail-access='directory']");

      for (const viewport of ACCESS_VIEWPORTS) {
        await captureAt(page, viewport, `mail-access-directory-unavailable-${viewport.name}.png`);
        await expectNoHorizontalOverflow(page);
      }

      await expectMinimumTouchTarget(
        access.getByRole("button", { name: "Retry", exact: true }),
        "directory retry action",
      );
      await expectMinimumTouchTarget(
        access.getByRole("button", { name: "Sign out", exact: true }),
        "directory sign-out action",
      );
    });
  });

});

test.describe("Mail session trust gate", () => {
  test.use({
    mailFixtureOptions: {
      sessionResponses: [{ kind: "authenticated" }, { kind: "unavailable" }],
    },
  });

  test("locks the existing workspace behind a focused, non-modal verification failure", async ({
    mailApp,
  }) => {
    const { fixture, page } = mailApp;
    await page.setViewportSize(ACCESS_VIEWPORTS[0]);
    await mailApp.goto();

    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    const notice = page.locator("[data-session-verification='failed']");
    await expect(notice).toBeVisible();
    await expect(notice).toHaveAttribute("role", "alert");
    await expect(notice).toBeFocused();
    await expect(notice).toContainText("Unable to verify your organization session; try again");
    await expect(notice).not.toHaveAttribute("aria-modal");
    await expect(page.locator(".mail-shell")).toHaveAttribute("inert", "");
    await expect(page.locator(".mail-shell")).toHaveAttribute("aria-busy", "true");
    await expect(page.locator("[data-session-verification='checking']")).toHaveCount(0);
    await expect(page.locator("[data-mail-access]")).toHaveCount(0);
    expect(fixture.state.sessionRequestCount).toBe(2);
    expect(fixture.state.mailboxDirectoryRequestCount).toBe(1);
    expect(fixture.state.inboxRequestCount).toBe(1);
    await expectAccessAxe(page, "[data-session-verification='failed']");

    for (const viewport of ACCESS_VIEWPORTS) {
      await captureAt(page, viewport, `mail-session-verification-failed-${viewport.name}.png`);
      await expectNoHorizontalOverflow(page);
    }

    await expectMinimumTouchTarget(
      notice.getByRole("button", { name: "Retry", exact: true }),
      "session retry action",
    );
    await expectMinimumTouchTarget(
      notice.getByRole("button", { name: "Sign out", exact: true }),
      "session sign-out action",
    );
  });
});
