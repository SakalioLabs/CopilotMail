import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import {
  E2E_ARCHIVE_ID,
  E2E_AUDIT_EVENT,
  E2E_COMPLIANCE_CASE,
  E2E_FIXED_NOW,
  E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME,
  expect,
  selectAdminRoute,
  test,
} from "./fixtures";

/**
 * Screenshot-free accessibility contracts for the Admin workspaces that batch
 * 11 did not cover: Approvals, Compliance cases, Legal holds, Mail domains,
 * and the full Audit workspace. They prove keyboard-only route navigation,
 * field-bound client validation, painted focus rings, and Axe-clean rendered
 * views under the shared WCAG tag set. They are automation evidence only and
 * do not claim WCAG 2.2 AA conformance.
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

test("Approvals binds create-form validation to its field and scans clean", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();
  await selectAdminRoute(page, "Approvals", { keyboard: true });
  await expect(page.getByRole("heading", {
    level: 1,
    name: "Enterprise approvals",
    exact: true,
  })).toBeVisible();
  await expectNoAxeViolations(page, ".workspace");

  const trigger = page.getByRole("button", {
    name: "Request approval",
    exact: true,
  });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const panel = page.locator(".approval-create-panel");
  await expect(panel).toBeVisible();

  const policyKey = panel.getByRole("textbox", {
    name: "Policy key",
    exact: true,
  });
  await expect(policyKey).toBeFocused();
  expect(await policyKey.evaluate((element) => (
    element.matches(":focus-visible")
  ))).toBe(true);

  // Every native constraint stays satisfied; only the custom bounded-text
  // rule fails, so the binding under test is the application's own contract.
  await policyKey.fill("audit.export");
  await panel.getByRole("combobox", { name: "High-risk action", exact: true })
    .selectOption("audit.export");
  await panel.getByRole("textbox", { name: "Resource kind", exact: true })
    .fill("audit_archive");
  const resourceId = panel.getByRole("textbox", {
    name: "Resource ID",
    exact: true,
  });
  await resourceId.fill(" 019b0000-0000-7000-8000-000000000401 ");
  await panel.getByRole("textbox", { name: "Reason", exact: true })
    .fill("Export the signed archive for review.");
  await panel.getByLabel("Expires", { exact: true }).fill("2026-08-25T12:00");
  await panel.getByRole("button", {
    name: "Submit approval request",
    exact: true,
  }).click();

  const alert = panel.getByRole("alert");
  await expect(alert).toHaveText(
    "A resource, reason, or ticket field has surrounding whitespace, control characters, or exceeds the server limit.",
  );
  const alertId = await alert.getAttribute("id");
  expect(alertId ?? "").not.toBe("");
  await expect(resourceId).toBeFocused();
  expect(await resourceId.evaluate((element) => element.matches(":focus-visible")))
    .toBe(true);
  await expect(resourceId).toHaveAttribute("aria-invalid", "true");
  expect(await resourceId.getAttribute("aria-describedby") ?? "")
    .toContain(alertId);
  // The rejected submission never reached the declared create route.
  expect(adminApp.requestLog.filter(({ method, path }) => (
    method === "POST" && path.includes("/approval-requests")
  ))).toHaveLength(0);
  await expectNoAxeViolations(page, ".approval-create-panel");

  await resourceId.fill("019b0000-0000-7000-8000-000000000401");
  await expect(alert).toBeHidden();
  await expect(resourceId).not.toHaveAttribute("aria-invalid");

  await page.keyboard.press("Escape");
  await expect(panel).toBeHidden();
  await expect(trigger).toBeFocused();
});

test("Compliance cases keep the selected workspace Axe-clean", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();
  await selectAdminRoute(page, "Compliance cases", { keyboard: true });

  const row = page.getByRole("button", {
    name: E2E_COMPLIANCE_CASE.title,
    exact: true,
  });
  await row.focus();
  await page.keyboard.press("Enter");
  const detail = page.locator(".admin-list-detail__detail");
  await expect(detail.locator("h2.admin-list-detail__title"))
    .toHaveText(E2E_COMPLIANCE_CASE.title);
  await expectNoAxeViolations(page, ".workspace");
});

test("Legal holds keep the selected workspace Axe-clean", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();
  await selectAdminRoute(page, "Legal holds", { keyboard: true });

  const row = page.getByRole("button", {
    name: `${E2E_ARCHIVE_ID.slice(0, 8)}…${E2E_ARCHIVE_ID.slice(-6)}`,
    exact: true,
  });
  await row.focus();
  await page.keyboard.press("Enter");
  const detail = page.locator(".admin-list-detail__detail");
  await expect(detail.locator("h2.admin-list-detail__title")).toBeVisible();
  await expectNoAxeViolations(page, ".workspace");
});

test("Mail domains keep the selected domain workspace Axe-clean", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();
  await selectAdminRoute(page, "Mail domains", { keyboard: true });
  await expect(page.getByRole("heading", {
    level: 1,
    name: "Mail domain onboarding",
    exact: true,
  })).toBeVisible();

  const row = page.getByRole("button", {
    name: E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME,
    exact: true,
  });
  await row.focus();
  await page.keyboard.press("Enter");
  const detail = page.locator(".admin-list-detail__detail");
  await expect(detail.locator("h2.admin-list-detail__title"))
    .toHaveText(E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME);
  await expectNoAxeViolations(page, ".workspace");
});

test("Audit search keeps the full results workspace Axe-clean", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
  await adminApp.goto();
  await selectAdminRoute(page, "Audit search", { keyboard: true });

  const row = page.getByRole("button", {
    name: E2E_AUDIT_EVENT.action,
    exact: true,
  });
  await row.focus();
  await page.keyboard.press("Enter");
  const detail = page.locator(".admin-list-detail__detail");
  await expect(detail.locator("h2.admin-list-detail__title")).toBeVisible();
  await expect(detail).toContainText(E2E_AUDIT_EVENT.resource_id);
  await expectNoAxeViolations(page, ".workspace");
});
