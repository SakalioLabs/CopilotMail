import type { Locator, Page } from "@playwright/test";
import {
  E2E_INITIAL_MAIL_DOMAIN_TOKEN,
  E2E_MAIL_DOMAIN_ID,
  E2E_MAIL_DOMAIN_NAME,
  E2E_ORGANIZATION_ID,
  E2E_ROTATED_MAIL_DOMAIN_TOKEN,
  E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID,
  E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME,
  expect,
  selectAdminRoute,
  test,
  type AdminAppFixture,
} from "./fixtures";

/**
 * Browser contracts for the enterprise mail-domain workbench. All responses
 * come from the in-memory fixture, including the deliberate 503-after-commit
 * path used to prove that a potentially committed route write is never retried.
 */

function mailDomainPath(mailDomainId: string): string {
  return `/api/v2/mail-domains/${mailDomainId}`;
}

function domainRequests(adminApp: AdminAppFixture) {
  return adminApp.requestLog.filter(({ path }) => path.includes("/mail-domains"));
}

async function openMailDomains(adminApp: AdminAppFixture, page: Page) {
  await adminApp.goto();
  await selectAdminRoute(page, "Mail domains");
  await expect(page.getByRole("heading", {
    level: 1,
    name: "Mail domain onboarding",
    exact: true,
  })).toBeVisible();
  await expect(page.getByRole("button", {
    name: E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME,
    exact: true,
  })).toBeVisible();
}

async function selectDomain(page: Page, name: string): Promise<Locator> {
  await page.getByRole("button", { name, exact: true }).click();
  const detail = page.locator(".admin-list-detail__detail");
  await expect(detail.locator("h2.admin-list-detail__title")).toHaveText(name);
  return detail;
}

async function openDomainEvidence(detail: Locator) {
  const evidence = detail.locator("details.mail-domain-evidence");
  await expect(evidence).not.toHaveAttribute("open", "");
  await evidence.locator("summary").click();
  await expect(evidence).toHaveAttribute("open", "");
  return evidence;
}

async function closeTokenDialog(page: Page, token: string) {
  const dialog = page.getByRole("dialog", {
    name: "One-time DNS verification token",
    exact: true,
  });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel("TXT record name", { exact: true }))
    .toHaveValue(`_copilotmail-verification.${E2E_MAIL_DOMAIN_NAME}`);
  await expect(dialog.getByLabel("TXT record value", { exact: true })).toHaveValue(token);
  await dialog.getByRole("button", { name: "Close dialog", exact: true }).last().click();
  await expect(dialog).toBeHidden();
  await expect(page.locator(`input[value="${token}"]`)).toHaveCount(0);
  expect(page.url()).not.toContain(token);
  expect(await page.evaluate((secret) => {
    const stores = [window.localStorage, window.sessionStorage];
    return stores.some((storage) => Array.from({ length: storage.length }, (_, index) => {
      const key = storage.key(index) ?? "";
      return `${key}\n${storage.getItem(key) ?? ""}`.includes(secret);
    }).some(Boolean));
  }, token)).toBe(false);
}

test("Mail domain onboarding uses the real list/detail lifecycle and keeps ownership tokens one-time", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.clock.setFixedTime(new Date("2026-08-24T12:00:00.000Z"));
  await openMailDomains(adminApp, page);

  // The list has a distinct endpoint and its selected row is then read from
  // the canonical detail endpoint rather than trusting page-summary data.
  await selectDomain(page, E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME);
  expect(domainRequests(adminApp)).toEqual(expect.arrayContaining([
    {
      method: "GET",
      path: `/api/v2/organizations/${E2E_ORGANIZATION_ID}/mail-domains`,
    },
    { method: "GET", path: mailDomainPath(E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID) },
  ]));

  await page.getByRole("button", { name: "Register domain", exact: true }).click();
  const registration = page.getByRole("dialog", {
    name: "Register mail domain",
    exact: true,
  });
  await registration.getByRole("textbox", { name: "Domain name", exact: true })
    .fill(E2E_MAIL_DOMAIN_NAME);
  await registration.getByRole("combobox", { name: "Outbound route mode", exact: true })
    .selectOption("smart_host");

  const registrationRequest = page.waitForRequest((request) => (
    request.method() === "POST"
      && new URL(request.url()).pathname
        === `/api/v2/organizations/${E2E_ORGANIZATION_ID}/mail-domains`
  ));
  await registration.getByRole("button", {
    name: "Register and get TXT token",
    exact: true,
  }).click();
  expect((await registrationRequest).postDataJSON()).toEqual({
    name: E2E_MAIL_DOMAIN_NAME,
    route_mode: "smart_host",
  });
  await closeTokenDialog(page, E2E_INITIAL_MAIL_DOMAIN_TOKEN);

  const detail = await selectDomain(page, E2E_MAIL_DOMAIN_NAME);
  await expect(detail.getByRole("region", { name: "Lifecycle facts", exact: true }))
    .toContainText(E2E_MAIL_DOMAIN_NAME);
  const lifecycle = detail.locator(".mail-domain-lifecycle");
  await expect(lifecycle).toContainText("Registered");
  await expect(lifecycle).toContainText("Latest DNS evidence");
  await expect(lifecycle).toContainText("Control-plane activation");
  await expect(lifecycle).toContainText("Mail-engine projection");
  await expect(detail.locator("details.mail-domain-evidence")).not.toHaveAttribute("open", "");
  await expect(page.locator(".workspace")).toHaveScreenshot("mail-domain-onboarding-1440.png", {
    animations: "disabled",
    caret: "hide",
  });

  // Rotation has an explicit confirmation and returns a replacement token
  // exactly once; the old token cannot be recovered through domain reads.
  const evidence = await openDomainEvidence(detail);
  await expect(evidence.getByText("Ownership verification", { exact: true })).toBeVisible();
  await evidence.getByRole("button", { name: "Rotate verification token", exact: true }).click();
  const rotation = page.getByRole("dialog", {
    name: "Rotate verification token",
    exact: true,
  });
  const rotationRequest = page.waitForRequest((request) => (
    request.method() === "POST"
      && new URL(request.url()).pathname
        === `${mailDomainPath(E2E_MAIL_DOMAIN_ID)}/ownership-challenge`
  ));
  await rotation.getByRole("button", {
    name: "Rotate verification token",
    exact: true,
  }).click();
  expect((await rotationRequest).postDataJSON()).toEqual({ expected_version: 1 });
  await closeTokenDialog(page, E2E_ROTATED_MAIL_DOMAIN_TOKEN);

  await detail.getByLabel("TXT record value", { exact: true })
    .fill(E2E_ROTATED_MAIL_DOMAIN_TOKEN);
  await detail.getByLabel("DKIM selector", { exact: true }).fill("mail2026");
  const dnsRequest = page.waitForRequest((request) => (
    request.method() === "POST"
      && new URL(request.url()).pathname
        === `${mailDomainPath(E2E_MAIL_DOMAIN_ID)}/dns-verifications`
  ));
  await detail.getByRole("button", { name: "Verify DNS", exact: true }).click();
  expect((await dnsRequest).postDataJSON()).toEqual({
    domain: E2E_MAIL_DOMAIN_NAME,
    ownership_token: E2E_ROTATED_MAIL_DOMAIN_TOKEN,
    dkim_selector: "mail2026",
  });
  await expect(detail.locator(".mail-domain-decision .notice"))
    .toContainText("DNS verification completed");
  await expect(detail.getByRole("button", {
    name: "Explicitly activate domain",
    exact: true,
  })).toBeVisible();

  await detail.getByRole("button", { name: "Explicitly activate domain", exact: true }).click();
  const activation = page.getByRole("dialog", {
    name: "Explicitly activate domain",
    exact: true,
  });
  const activationRequest = page.waitForRequest((request) => (
    request.method() === "POST"
      && new URL(request.url()).pathname
        === `${mailDomainPath(E2E_MAIL_DOMAIN_ID)}/activation`
  ));
  await activation.getByRole("button", {
    name: "Explicitly activate domain",
    exact: true,
  }).click();
  expect((await activationRequest).postData()).toBeNull();
  await expect(detail.getByRole("checkbox", { name: "Enable inbound", exact: true })).toBeVisible();

  await detail.getByRole("checkbox", { name: "Enable inbound", exact: true }).check();
  await detail.getByRole("checkbox", { name: "Enable outbound", exact: true }).check();
  await detail.getByLabel("Managed relay reference", { exact: true })
    .fill("relay://cn-east-1/mail");
  await detail.getByLabel("DKIM key reference", { exact: true })
    .fill("kms://mail/dkim/relay.example.test");
  const routeRequest = page.waitForRequest((request) => (
    request.method() === "PUT"
      && new URL(request.url()).pathname === `${mailDomainPath(E2E_MAIL_DOMAIN_ID)}/route`
  ));
  await detail.getByRole("button", { name: "Save route configuration", exact: true }).click();
  expect((await routeRequest).postDataJSON()).toEqual({
    expected_version: 1,
    inbound_enabled: true,
    outbound_enabled: true,
    smart_host_ref: "relay://cn-east-1/mail",
    dkim_key_ref: "kms://mail/dkim/relay.example.test",
  });
  await expect(detail.getByRole("checkbox", { name: "Enable inbound", exact: true }))
    .toBeChecked();
  await expect(detail.getByRole("checkbox", { name: "Enable outbound", exact: true }))
    .toBeChecked();

  await detail.getByRole("button", { name: "Suspend domain", exact: true }).click();
  const suspension = page.getByRole("dialog", { name: "Suspend domain", exact: true });
  await suspension.getByRole("textbox", { name: "Suspension reason code", exact: true })
    .fill("maintenance_window");
  const suspensionRequest = page.waitForRequest((request) => (
    request.method() === "POST"
      && new URL(request.url()).pathname === `${mailDomainPath(E2E_MAIL_DOMAIN_ID)}/suspension`
  ));
  await suspension.getByRole("button", { name: "Suspend domain", exact: true }).click();
  expect((await suspensionRequest).postDataJSON()).toEqual({
    reason_code: "maintenance_window",
  });
  await expect(detail.locator(".mail-domain-lifecycle").getByText("Suspended", { exact: true }))
    .toBeVisible();
  await expect(detail.getByRole("checkbox", { name: "Enable inbound", exact: true })).toHaveCount(0);
  await expect(detail.getByRole("checkbox", { name: "Enable outbound", exact: true })).toHaveCount(0);
  await expect(detail.getByRole("button", { name: "Verify DNS", exact: true })).toHaveCount(0);
});

test("a response-unknown route write freezes mutations and reconciles through GET only", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openMailDomains(adminApp, page);
  const detail = await selectDomain(page, E2E_UNKNOWN_WRITE_MAIL_DOMAIN_NAME);
  await expect(page.locator(".workspace")).toHaveScreenshot("mail-domain-onboarding-390.png", {
    animations: "disabled",
    caret: "hide",
  });

  await detail.getByRole("checkbox", { name: "Enable inbound", exact: true }).check();
  const routeRequest = page.waitForRequest((request) => (
    request.method() === "PUT"
      && new URL(request.url()).pathname
        === `${mailDomainPath(E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID)}/route`
  ));
  await detail.getByRole("button", { name: "Save route configuration", exact: true }).click();
  expect((await routeRequest).postDataJSON()).toEqual({
    expected_version: 7,
    inbound_enabled: true,
    outbound_enabled: false,
    smart_host_ref: null,
    dkim_key_ref: null,
  });

  await expect(page.getByText("The domain write outcome is not confirmed", {
    exact: true,
  })).toBeVisible();
  await expect(page.getByRole("button", { name: "Register domain", exact: true }))
    .toBeDisabled();
  await expect(detail.getByRole("button", { name: "Save route configuration", exact: true }))
    .toBeDisabled();
  const writeCount = domainRequests(adminApp).filter(({ method }) => method !== "GET").length;
  expect(writeCount).toBe(1);

  const logBeforeRefresh = adminApp.requestLog.length;
  await page.getByRole("button", { name: "Refresh state read-only", exact: true }).click();
  await expect(detail.getByRole("checkbox", { name: "Enable inbound", exact: true }))
    .toBeChecked();

  const refreshRequests = adminApp.requestLog.slice(logBeforeRefresh);
  // The uncertain route mutation has one known resource target. The freeze
  // must reconcile only that exact read, never refresh a broad domain list or
  // send a second mutation.
  expect(refreshRequests).toEqual([
    { method: "GET", path: mailDomainPath(E2E_UNKNOWN_WRITE_MAIL_DOMAIN_ID) },
  ]);
  expect(domainRequests(adminApp).filter(({ method }) => method !== "GET")).toHaveLength(1);
});
