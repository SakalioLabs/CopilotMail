import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import {
  E2E_AUDIT_EVENT,
  E2E_FIXED_NOW,
  expect,
  selectAdminRoute,
  test,
} from "./fixtures";

/**
 * Browser contracts for the audit-search filter surface. These cases stay
 * screenshot-free: they prove keyboard-only reachability, field-bound client
 * validation, a painted focus ring on script focus, and an Axe-clean filter
 * panel in both shipped locales.
 */
const WCAG_AA_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] as const;

async function expectNoAxeViolations(page: Page, include: string) {
  const results = await new AxeBuilder({ page })
    .include(include)
    .withTags([...WCAG_AA_TAGS])
    .analyze();
  expect(results.violations, JSON.stringify(results.violations, null, 2))
    .toEqual([]);
}

test("Audit search binds both client validation failures to their fields", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();
  await selectAdminRoute(page, "Audit search");
  await expect(page.getByRole("button", {
    name: E2E_AUDIT_EVENT.action,
    exact: true,
  })).toBeVisible();

  const filters = page.locator(".audit-filters");
  const runSearch = filters.getByRole("button", { name: "Run search", exact: true });
  const alert = page.getByRole("alert");
  const resourceId = filters.getByRole("textbox", {
    name: "Resource ID",
    exact: true,
  });
  const resourceType = filters.getByRole("textbox", {
    name: "Resource type",
    exact: true,
  });

  await filters.getByText("More filters", { exact: true }).click();
  await resourceId.fill("corp.example");
  await resourceId.press("Enter");

  await expect(alert).toHaveText(
    "Resource type is required when filtering by resource ID.",
  );
  const alertId = await alert.getAttribute("id");
  expect(alertId ?? "").not.toBe("");
  await expect(resourceType).toBeFocused();
  expect(await resourceType.evaluate((element) => (
    element.matches(":focus-visible")
  ))).toBe(true);
  await expect(resourceType).toHaveAttribute("aria-invalid", "true");
  expect(await resourceType.getAttribute("aria-describedby") ?? "")
    .toContain(alertId);
  // The rejected submission never reached the declared audit route.
  expect(adminApp.requestLog.filter(({ path }) => path.includes("/audit-events")))
    .toHaveLength(1);
  await expectNoAxeViolations(page, ".audit-filters");

  await filters.getByRole("button", { name: "Clear", exact: true }).click();
  await runSearch.click();
  await expect(alert).toBeHidden();
  await expect(resourceType).not.toHaveAttribute("aria-invalid");

  const from = filters.getByLabel("From", { exact: true });
  const to = filters.getByLabel("To", { exact: true });
  await from.fill("2026-08-24T12:00");
  await to.fill("2026-08-23T12:00");
  await runSearch.click();

  await expect(alert).toHaveText(
    "The time window must be valid and no longer than 31 days.",
  );
  await expect(from).toBeFocused();
  for (const boundary of [from, to]) {
    await expect(boundary).toHaveAttribute("aria-invalid", "true");
    expect(await boundary.getAttribute("aria-describedby") ?? "")
      .toContain(alertId);
  }
  await expectNoAxeViolations(page, ".audit-filters");

  await filters.getByRole("button", { name: "Last 24 hours", exact: true })
    .click();
  await runSearch.click();
  await expect(alert).toBeHidden();
  for (const boundary of [from, to]) {
    await expect(boundary).not.toHaveAttribute("aria-invalid");
  }
  await expect(page.getByRole("button", {
    name: E2E_AUDIT_EVENT.action,
    exact: true,
  })).toBeVisible();
});

test("Audit search announces the same field binding in zh-CN", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();

  await page.getByRole("combobox", { name: "Language", exact: true })
    .selectOption("zh-CN");
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  await selectAdminRoute(page, "审计检索");
  await expect(page.getByRole("button", {
    name: E2E_AUDIT_EVENT.action,
    exact: true,
  })).toBeVisible();

  const filters = page.locator(".audit-filters");
  const resourceId = filters.getByRole("textbox", {
    name: "资源 ID",
    exact: true,
  });
  const resourceType = filters.getByRole("textbox", {
    name: "资源类型",
    exact: true,
  });
  await filters.getByText("更多筛选", { exact: true }).click();
  await resourceId.fill("corp.example");
  await resourceId.press("Enter");

  const alert = page.getByRole("alert");
  await expect(alert).toHaveText("填写资源 ID 时必须同时填写资源类型。");
  await expect(resourceType).toBeFocused();
  await expect(resourceType).toHaveAttribute("aria-invalid", "true");
  const alertId = await alert.getAttribute("id");
  expect(await resourceType.getAttribute("aria-describedby") ?? "")
    .toContain(alertId);
  await expectNoAxeViolations(page, ".audit-filters");
});
