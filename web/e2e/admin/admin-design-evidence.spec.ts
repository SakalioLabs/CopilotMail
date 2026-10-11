import AxeBuilder from "@axe-core/playwright";
import type { Locator, Page } from "@playwright/test";
import {
  E2E_APPENDED_RETENTION_VERSION_ID,
  E2E_APPROVAL_ID,
  E2E_COMPLIANCE_CASE,
  E2E_FIXED_NOW,
  E2E_ORGANIZATION_ID,
  E2E_RETENTION_POLICY_ID,
  expect,
  selectAdminRoute,
  test,
} from "./fixtures";

const WCAG_AA_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] as const;
const OVERVIEW_VIEWPORTS = [
  { name: "1440", width: 1440, height: 1000 },
  { name: "1280", width: 1280, height: 960 },
  { name: "1024", width: 1024, height: 960 },
  { name: "768", width: 768, height: 960 },
  { name: "390", width: 390, height: 844 },
] as const;
const RESPONSIVE_VIEWPORTS = [
  { name: "1440", width: 1440, height: 1000 },
  { name: "390", width: 390, height: 844 },
] as const;

async function expectNoHorizontalOverflow(page: Page) {
  await expect.poll(() => page.evaluate(() => ({
    body: document.body.scrollWidth <= document.body.clientWidth,
    root: document.documentElement.scrollWidth <= document.documentElement.clientWidth,
  }))).toEqual({ body: true, root: true });
}

async function expectNoAxeViolations(page: Page, include: string) {
  const results = await new AxeBuilder({ page })
    .include(include)
    .withTags([...WCAG_AA_TAGS])
    .analyze();
  expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
}

async function capture(page: Page, name: string) {
  await expect(page).toHaveScreenshot(name, {
    animations: "disabled",
    caret: "hide",
    fullPage: true,
    scale: "css",
  });
}

async function openCases(page: Page) {
  await selectAdminRoute(page, "Compliance cases");
  await expect(page.getByRole("heading", {
    level: 1,
    name: "Compliance cases",
    exact: true,
  })).toBeVisible();
  await expect(page.getByRole("button", {
    name: "Mailbox domain activation review",
    exact: true,
  })).toBeVisible();
}

async function openCaseDialog(page: Page): Promise<{
  dialog: Locator;
  trigger: Locator;
}> {
  const trigger = page.getByRole("button", { name: "New case", exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "New case", exact: true });
  await expect(dialog).toBeVisible();
  return { dialog, trigger };
}

for (const viewport of OVERVIEW_VIEWPORTS) {
  test(`Overview stays factual and accessible at ${viewport.name}px`, async ({
    adminApp,
    page,
  }) => {
    await page.setViewportSize(viewport);
    await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
    await adminApp.goto();

    const openCases = page.getByRole("region", { name: "Open cases", exact: true });
    const activeHolds = page.getByRole("region", { name: "Active legal holds", exact: true });
    await expect(openCases).toHaveAttribute("data-overview-state", "ready");
    await expect(activeHolds).toHaveAttribute("data-overview-state", "ready");
    await expect(openCases).toContainText("0");
    await expect(activeHolds).toContainText("0");

    await expectNoHorizontalOverflow(page);
    await expectNoAxeViolations(page, ".app-shell");
    await capture(page, `admin-overview-${viewport.name}.png`);
  });

  test(`Case Create Dialog owns focus and dismissal at ${viewport.name}px`, async ({
    adminApp,
    page,
  }) => {
    await page.setViewportSize(viewport);
    await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
    await adminApp.goto();
    await openCases(page);

    const firstOpen = await openCaseDialog(page);
    const close = firstOpen.dialog.getByRole("button", {
      name: "Close dialog",
      exact: true,
    });
    const externalReference = firstOpen.dialog.getByRole("textbox", {
      name: "External reference",
      exact: true,
    });
    await expect(externalReference).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(close).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(externalReference).toBeFocused();

    await externalReference.fill("SEC-2026-0917");
    await firstOpen.dialog.getByRole("textbox", {
      name: "Title",
      exact: true,
    }).fill("Cross-region mailbox access investigation");
    await firstOpen.dialog.getByRole("textbox", {
      name: "Purpose",
      exact: true,
    }).fill("Preserve the immutable mail and authorization trail while Human and Agent access is reviewed.");

    await expectNoHorizontalOverflow(page);
    await expect.poll(() => firstOpen.dialog.evaluate((element) => (
      element.scrollWidth <= element.clientWidth
    ))).toBe(true);
    await expectNoAxeViolations(page, ".dialog");
    await capture(page, `admin-case-create-dialog-${viewport.name}.png`);

    await page.keyboard.press("Escape");
    await expect(firstOpen.dialog).toBeHidden();
    await expect(firstOpen.trigger).toBeFocused();

    const secondOpen = await openCaseDialog(page);
    await page.locator(".dialog-backdrop").click({ position: { x: 4, y: 4 } });
    await expect(secondOpen.dialog).toBeHidden();
    await expect(secondOpen.trigger).toBeFocused();
  });
}

test("Overview decision navigation preserves its bounded reads and moves focus to the workspace", async ({
  adminApp,
  page,
}) => {
  let overviewCaseReads = 0;
  await page.route("**/compliance-cases**", async (route) => {
    const url = new URL(route.request().url());
    if (
      url.pathname === `/api/v2/organizations/${E2E_ORGANIZATION_ID}/compliance-cases`
      && url.searchParams.get("status") === "open"
      && url.searchParams.get("limit") === "100"
    ) {
      overviewCaseReads += 1;
      await route.fulfill({
        status: 200,
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({
          items: [E2E_COMPLIANCE_CASE],
          next_cursor: null,
          has_more: false,
        }),
      });
      return;
    }
    await route.fallback();
  });

  await page.setViewportSize({ width: 1440, height: 1000 });
  await adminApp.goto();
  const action = page.getByRole("button", { name: "View open cases", exact: true });
  await expect(action).toBeVisible();
  await action.click();

  await expect(page.getByRole("heading", {
    level: 1,
    name: "Compliance cases",
    exact: true,
  })).toBeVisible();
  await expect(page.locator("main.workspace")).toBeFocused();
  expect(overviewCaseReads).toBe(1);
  expect(new URL(page.url()).pathname).toBe("/");
});

for (const viewport of RESPONSIVE_VIEWPORTS) {
  test(`Approval actionable detail is accessible at ${viewport.name}px`, async ({
    adminApp,
    page,
  }) => {
    await page.setViewportSize(viewport);
    await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
    await adminApp.goto();
    await selectAdminRoute(page, "Approvals");

    const row = page.getByRole("button", { name: "audit.export", exact: true });
    await expect(row).toBeVisible();
    await row.click();
    const detail = page.locator(".approval-detail");
    const title = detail.locator("h2.admin-list-detail__title");
    await expect(title).toHaveText(
      `${E2E_APPROVAL_ID.slice(0, 8)}…${E2E_APPROVAL_ID.slice(-6)}`,
    );
    await expect(detail.getByText(
      "Export the signed archive for the domain-activation review.",
      { exact: true },
    )).toBeVisible();
    const context = page.getByRole("region", {
      name: "Current approval context",
      exact: true,
    });
    await expect(context).toContainText("Needs my decision");
    await expect(context).toContainText("Pending");
    await expect(context.getByRole("button", {
      name: "Request approval",
      exact: true,
    })).toBeVisible();
    await expect(detail.getByRole("heading", {
      name: "What this decision changes",
      exact: true,
    })).toBeVisible();
    await expect(detail.getByRole("heading", {
      name: "Resource and decision boundary",
      exact: true,
    })).toBeVisible();
    await expect(detail.locator("details.approval-evidence-inspector"))
      .not.toHaveAttribute("open");
    if (viewport.width <= 1080) await expect(title).toBeFocused();

    await expectNoHorizontalOverflow(page);
    await expectNoAxeViolations(page, ".workspace");
    if (viewport.width === 1440) {
      await capture(page, "admin-approval-actionable-1440.png");
    }
  });
}

test("Admin navigation changes only at the 1023/1024 and 1279/1280 shell boundaries", async ({
  adminApp,
  page,
}) => {
  const rail = page.locator(".admin-navigation--rail");
  const trigger = page.getByRole("button", { name: "Open navigation", exact: true });
  const currentPage = page.locator(".mobile-navigation-context__page");

  for (const viewport of [
    { width: 1023, height: 960, mode: "drawer" },
    { width: 1024, height: 960, mode: "compact" },
    { width: 1279, height: 960, mode: "compact" },
  ] as const) {
    await page.setViewportSize(viewport);
    await adminApp.goto();

    await expect(rail).toBeHidden();
    await expect(trigger).toBeVisible();
    await expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");

    if (viewport.mode === "drawer") {
      await expect(currentPage).toBeVisible();
      await expect(currentPage).toContainText("Overview");
    } else {
      // The compact rail stays an explicit named control, rather than a
      // hover-only icon strip; the current-page label belongs in its drawer.
      await expect(currentPage).toBeHidden();
      await trigger.focus();
      await page.keyboard.press("Enter");
      const drawer = page.getByRole("dialog", {
        name: "Admin navigation",
        exact: true,
      });
      await expect(drawer).toBeVisible();
      await expect(drawer).toContainText("Overview");
      await drawer.getByRole("button", {
        name: "Close navigation",
        exact: true,
      }).click();
      await expect(drawer).toBeHidden();
      await expect(trigger).toBeFocused();
    }
  }

  await page.setViewportSize({ width: 1280, height: 960 });
  await adminApp.goto();
  await expect(rail).toBeVisible();
  await expect(trigger).toBeHidden();
  await expect(currentPage).toBeHidden();
  const casesAt1280 = rail.getByRole("button", {
    name: "Compliance cases",
    exact: true,
  });
  await expect(casesAt1280).toBeVisible();
  await casesAt1280.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", {
    level: 1,
    name: "Compliance cases",
    exact: true,
  })).toBeVisible();
});

test("Admin drawer retains keyboard focus and current-page context at 1023px", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1023, height: 960 });
  await adminApp.goto();

  const rail = page.locator(".admin-navigation--rail");
  const trigger = page.getByRole("button", { name: "Open navigation", exact: true });
  await expect(rail).toBeHidden();
  await expect(page.locator(".mobile-navigation-context")).toContainText("Overview");
  await trigger.focus();
  await page.keyboard.press("Enter");

  const drawer = page.getByRole("dialog", { name: "Admin navigation", exact: true });
  const close = drawer.getByRole("button", { name: "Close navigation", exact: true });
  await expect(drawer).toBeVisible();
  await expect(close).toBeFocused();
  await expect(drawer.getByRole("heading", { name: "Command center", exact: true }))
    .toBeVisible();
  await expect(drawer.getByRole("heading", { name: "Operations & review", exact: true }))
    .toBeVisible();
  await expect(drawer.getByRole("heading", { name: "Evidence & retention", exact: true }))
    .toBeVisible();
  await expect(drawer.getByRole("heading", { name: "Identity & automation", exact: true }))
    .toBeVisible();
  await expectNoAxeViolations(page, ".adaptive-nav-drawer");

  await close.click();
  await expect(drawer).toBeHidden();
  await expect(trigger).toBeFocused();
  await trigger.click();
  await expect(drawer).toBeVisible();

  const casesInDrawer = drawer.getByRole("button", { name: "Compliance cases", exact: true });
  await casesInDrawer.focus();
  await page.keyboard.press("Enter");
  await expect(drawer).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect(page.locator(".mobile-navigation-context")).toContainText("Compliance cases");
  await expect(page.getByRole("heading", {
    level: 1,
    name: "Compliance cases",
    exact: true,
  })).toBeVisible();
});

test("Admin desktop rail keeps all four named operator-intent groups at 1440px", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();

  const rail = page.locator(".admin-navigation--rail");
  await expect(rail).toBeVisible();
  for (const group of [
    "Command center",
    "Operations & review",
    "Evidence & retention",
    "Identity & automation",
  ]) {
    await expect(rail.getByRole("heading", { name: group, exact: true })).toBeVisible();
  }
  await expect(rail.getByRole("button", { name: "Mail domains", exact: true })).toBeVisible();
  await expect(rail.getByRole("button", { name: "Audit search", exact: true })).toBeVisible();
  await expect(rail.getByRole("button", { name: "Agent inspector", exact: true })).toBeVisible();
  await expectNoAxeViolations(page, ".admin-navigation--rail");
  await expect(rail).toHaveScreenshot("admin-navigation-rail-1440.png", {
    animations: "disabled",
    caret: "hide",
    scale: "css",
  });
});

test("Retention appends one immutable version and reconciles the authoritative policy", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();
  await selectAdminRoute(page, "Retention");
  await expect(page.getByRole("heading", {
    level: 1,
    name: "Production retention",
    exact: true,
  })).toBeVisible();

  await page.getByRole("textbox", { name: "Retention policy ID", exact: true })
    .fill(E2E_RETENTION_POLICY_ID);
  await page.getByRole("button", { name: "Load policy", exact: true }).click();
  await expect(page.getByRole("heading", {
    level: 2,
    name: "Production audit evidence",
    exact: true,
  })).toBeVisible();

  const editor = page.getByRole("form", { name: "Append draft", exact: true });
  await editor.getByRole("spinbutton", { name: "Minimum retention days", exact: true })
    .fill("3000");
  await editor.getByRole("spinbutton", { name: "Archive after", exact: true })
    .fill("45");
  await editor.getByRole("textbox", { name: "Scope rule (JSON)", exact: true })
    .fill('{"environment":"production","region":"cn-east-1"}');

  const appendRequest = page.waitForRequest((request) => (
    request.method() === "POST"
      && new URL(request.url()).pathname
        === `/api/v2/retention-policies/${E2E_RETENTION_POLICY_ID}/versions`
  ));
  await editor.getByRole("button", { name: "Append draft", exact: true }).click();
  expect((await appendRequest).postDataJSON()).toEqual({
    minimum_retention_days: 3000,
    archive_after_days: 45,
    delete_after_days: null,
    rule: { environment: "production", region: "cn-east-1" },
  });

  const latest = page.locator(".retention-version").filter({ hasText: "Latest draft" });
  await expect(latest).toContainText("v2 · 3000 days");
  await expect(latest).toContainText(E2E_APPENDED_RETENTION_VERSION_ID);
  await expect(editor.getByRole("spinbutton", {
    name: "Minimum retention days",
    exact: true,
  })).toHaveValue("3000");
  await expectNoHorizontalOverflow(page);
  await expectNoAxeViolations(page, ".workspace");
});
