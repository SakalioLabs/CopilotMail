import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import {
  E2E_AGENT_DISPLAY_NAME,
  E2E_AGENT_ID,
  E2E_AGENT_OWNER_DISPLAY_NAME,
  E2E_APPROVAL_ID,
  E2E_FIXED_NOW,
  E2E_ORGANIZATION_ID,
  E2E_RETENTION_POLICY_ID,
  E2E_RETENTION_VERSION_ID,
  E2E_ROLE_ASSIGNMENTS,
  expect,
  selectAdminRoute,
  test,
} from "./fixtures";

const WCAG_AA_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] as const;

type ScenarioLocale = "en" | "zh-CN";

interface ScenarioCopy {
  readonly roleNavigation: string;
  readonly rolesTitle: string;
  readonly rolesContext: string;
  readonly roleCatalogEvidence: string;
  readonly roleAssignmentEvidence: string;
  readonly principalId: string;
  readonly approvalId: string;
  readonly reviewGrant: string;
  readonly human: string;
  readonly agent: string;
  readonly noExpiry: string;
  readonly retentionNavigation: string;
  readonly retentionTitle: string;
  readonly retentionPolicyId: string;
  readonly loadPolicy: string;
  readonly retentionVersionFacts: string;
  readonly activeVersion: string;
  readonly latestVersion: string;
  readonly retentionEvidence: string;
  readonly reviewRetentionActivation: string;
}

const COPY: Record<ScenarioLocale, ScenarioCopy> = {
  en: {
    roleNavigation: "Role access",
    rolesTitle: "Roles and assignments",
    rolesContext: "Current role-assignment context",
    roleCatalogEvidence: "Role-catalog evidence",
    roleAssignmentEvidence: "Assignment evidence",
    principalId: "Target principal ID",
    approvalId: "Approval request ID",
    reviewGrant: "Review time-bound grant",
    human: "Human",
    agent: "Agent",
    noExpiry: "No expiry",
    retentionNavigation: "Retention",
    retentionTitle: "Production retention",
    retentionPolicyId: "Retention policy ID",
    loadPolicy: "Load policy",
    retentionVersionFacts: "Version facts",
    activeVersion: "Active version",
    latestVersion: "Latest draft",
    retentionEvidence: "Policy and version evidence",
    reviewRetentionActivation: "Review latest-version activation",
  },
  "zh-CN": {
    roleNavigation: "角色授权",
    rolesTitle: "角色与授权分配",
    rolesContext: "当前角色授权上下文",
    roleCatalogEvidence: "角色目录证据",
    roleAssignmentEvidence: "授权记录证据",
    principalId: "目标主体 ID",
    approvalId: "审批请求 ID",
    reviewGrant: "核对限时授权",
    human: "人类",
    agent: "Agent",
    noExpiry: "无到期时间",
    retentionNavigation: "保留策略",
    retentionTitle: "生产保留策略",
    retentionPolicyId: "保留策略 ID",
    loadPolicy: "读取策略",
    retentionVersionFacts: "版本事实",
    activeVersion: "当前生效版本",
    latestVersion: "最新草稿版本",
    retentionEvidence: "策略与版本证据",
    reviewRetentionActivation: "核对最新版本激活",
  },
};

const SCENARIOS = [
  { name: "1440-en", width: 1440, height: 1000, locale: "en" },
  { name: "390-zh-CN", width: 390, height: 844, locale: "zh-CN" },
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

async function switchScenarioLocale(page: Page, locale: ScenarioLocale) {
  if (locale === "en") return;
  // Narrow shells deliberately move session controls into the explicit
  // navigation drawer. Open that real control surface instead of asserting
  // against a visually hidden duplicate in the persistent rail.
  await page.getByRole("button", { name: "Open navigation", exact: true }).click();
  const drawer = page.getByRole("dialog", { name: "Admin navigation", exact: true });
  await expect(drawer).toBeVisible();
  const language = drawer.locator(".locale-switch select");
  await expect(language).toBeVisible();
  await language.selectOption("zh-CN");
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  await page.getByRole("button", { name: "关闭导航", exact: true }).click();
  await expect(drawer).toBeHidden();
}

async function selectScenarioRoute(
  page: Page,
  route: string,
  locale: ScenarioLocale,
) {
  await selectAdminRoute(
    page,
    route,
    locale === "zh-CN"
      ? { drawerName: "企业管理导航", triggerName: "打开导航" }
      : undefined,
  );
}

function roleAssignmentWriteCount(
  requestLog: ReadonlyArray<{ readonly method: string; readonly path: string }>,
) {
  return requestLog.filter(({ method, path }) => (
    method === "POST" && path.endsWith("/role-assignments")
  )).length;
}

function retentionActivationWriteCount(
  requestLog: ReadonlyArray<{ readonly method: string; readonly path: string }>,
) {
  return requestLog.filter(({ method, path }) => (
    method === "POST" && path.endsWith("/activation")
  )).length;
}

for (const scenario of SCENARIOS) {
  test(`Roles preserves real Human and Agent assignment evidence at ${scenario.name}`, async ({
    adminApp,
    page,
  }) => {
    const copy = COPY[scenario.locale];
    await page.setViewportSize({ width: scenario.width, height: scenario.height });
    await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
    await adminApp.goto();
    await switchScenarioLocale(page, scenario.locale);
    await selectScenarioRoute(page, copy.roleNavigation, scenario.locale);

    await expect(page.getByRole("heading", {
      level: 1,
      name: copy.rolesTitle,
      exact: true,
    })).toBeVisible();
    await expect(page.locator(".role-card").getByText("Mail administrator", {
      exact: true,
    })).toBeVisible();

    const context = page.getByRole("region", {
      name: copy.rolesContext,
      exact: true,
    });
    await expect(context).toContainText(E2E_ORGANIZATION_ID);
    await expect(context).toContainText("2");

    const catalogEvidence = page.locator("details.role-catalog-evidence");
    const assignmentEvidence = page.locator("details.role-assignment-evidence");
    await expect(catalogEvidence).not.toHaveAttribute("open");
    await expect(catalogEvidence).toContainText(copy.roleCatalogEvidence);
    await expect(assignmentEvidence).toHaveCount(E2E_ROLE_ASSIGNMENTS.length);
    await expect(assignmentEvidence.first()).not.toHaveAttribute("open");
    await expect(assignmentEvidence.first()).toContainText(copy.roleAssignmentEvidence);

    const humanRow = page.locator(".role-assignment-row").filter({
      hasText: E2E_AGENT_OWNER_DISPLAY_NAME,
    });
    const agentRow = page.locator(".role-assignment-row").filter({
      hasText: E2E_AGENT_DISPLAY_NAME,
    });
    await expect(humanRow).toContainText(copy.noExpiry);
    await expect(humanRow.locator('[data-identity-kind="human"]')).toHaveText(copy.human);
    await expect(agentRow.locator('[data-identity-kind="agent"]')).toHaveText(copy.agent);
    await expect(agentRow).toContainText(E2E_AGENT_ID.slice(0, 8));

    await expectNoHorizontalOverflow(page);
    await expectNoAxeViolations(page, ".workspace");
    await expect(page).toHaveScreenshot(`admin-roles-${scenario.name}.png`, {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });

    const writesBeforeReview = roleAssignmentWriteCount(adminApp.requestLog);
    await page.getByRole("textbox", { name: copy.principalId, exact: true })
      .fill(E2E_AGENT_ID);
    await page.getByRole("textbox", { name: copy.approvalId, exact: true })
      .fill(E2E_APPROVAL_ID);
    await page.getByRole("button", { name: copy.reviewGrant, exact: true }).click();

    const review = page.getByRole("dialog", { name: copy.reviewGrant, exact: true });
    await expect(review).toBeVisible();
    await expect(review).toContainText(E2E_AGENT_ID);
    await expect(review).toContainText("Mail administrator");
    await expect(review).toContainText(E2E_APPROVAL_ID);
    expect(roleAssignmentWriteCount(adminApp.requestLog)).toBe(writesBeforeReview);
  });

  test(`Retention keeps exact policy/version facts and requires review at ${scenario.name}`, async ({
    adminApp,
    page,
  }) => {
    const copy = COPY[scenario.locale];
    await page.setViewportSize({ width: scenario.width, height: scenario.height });
    await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
    await adminApp.goto();
    await switchScenarioLocale(page, scenario.locale);
    await selectScenarioRoute(page, copy.retentionNavigation, scenario.locale);

    await expect(page.getByRole("heading", {
      level: 1,
      name: copy.retentionTitle,
      exact: true,
    })).toBeVisible();
    await page.getByRole("textbox", { name: copy.retentionPolicyId, exact: true })
      .fill(E2E_RETENTION_POLICY_ID);
    await page.getByRole("button", { name: copy.loadPolicy, exact: true }).click();

    const context = page.getByRole("region", {
      name: "Production audit evidence",
      exact: true,
    });
    await expect(context).toContainText(E2E_RETENTION_POLICY_ID);
    await expect(context).toContainText(E2E_ORGANIZATION_ID);

    const versionFacts = page.getByRole("region", {
      name: copy.retentionVersionFacts,
      exact: true,
    });
    const active = versionFacts.locator(".retention-version").filter({
      hasText: copy.activeVersion,
    });
    const latest = versionFacts.locator(".retention-version").filter({
      hasText: copy.latestVersion,
    });
    await expect(active).toContainText(E2E_RETENTION_VERSION_ID);
    await expect(latest).toContainText(E2E_RETENTION_VERSION_ID);
    await expect(page.locator("details.retention-evidence")).not.toHaveAttribute("open");
    await expect(page.locator("details.retention-evidence")).toContainText(copy.retentionEvidence);

    if (scenario.width === 390) {
      const [appendDecision, activationDecision] = await Promise.all([
        page.locator(".retention-decision-section").first().boundingBox(),
        page.locator(".retention-decision-section").nth(1).boundingBox(),
      ]);
      expect(appendDecision).not.toBeNull();
      expect(activationDecision).not.toBeNull();
      expect(activationDecision!.y).toBeGreaterThan(appendDecision!.y);
    }

    await expectNoHorizontalOverflow(page);
    await expectNoAxeViolations(page, ".workspace");
    await expect(page).toHaveScreenshot(`admin-retention-${scenario.name}.png`, {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });

    const writesBeforeReview = retentionActivationWriteCount(adminApp.requestLog);
    await page.getByRole("textbox", { name: copy.approvalId, exact: true })
      .fill(E2E_APPROVAL_ID);
    await page.getByRole("button", {
      name: copy.reviewRetentionActivation,
      exact: true,
    }).click();
    const review = page.getByRole("dialog", {
      name: copy.reviewRetentionActivation,
      exact: true,
    });
    await expect(review).toBeVisible();
    await expect(review).toContainText(E2E_RETENTION_POLICY_ID);
    await expect(review).toContainText(E2E_RETENTION_VERSION_ID);
    await expect(review).toContainText(E2E_APPROVAL_ID);
    expect(retentionActivationWriteCount(adminApp.requestLog)).toBe(writesBeforeReview);
  });
}
