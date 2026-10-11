import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import {
  type AdminAppFixture,
  type AdminInspectorFixtureResponse,
  type AdminMailSubmissionEvidenceFixtureResponse,
  E2E_AGENT_INSPECTOR_DTO,
  E2E_AGENT_INSPECTOR_PRODUCTION_DTO,
  E2E_AGENT_DISPLAY_NAME,
  E2E_AGENT_ID,
  E2E_AGENT_MAIL_SUBMISSION_ID,
  expect,
  selectAdminRoute,
  test,
} from "./fixtures";

/**
 * Browser-contract tests for the deliberately bounded Agent Inspector.
 *
 * The configured runner serves the built Admin app as `baseURL`; fixtures
 * answer all declared API traffic locally and reject every non-loopback
 * request. Committed baselines change only through the explicit update task.
 */

const INTERNAL_HOST = "db-prod.internal.invalid";
const SENSITIVE_TRACE = "trace-sensitive-control-plane-root";
const SENSITIVE_KEY = "-----BEGIN PRIVATE KEY-----";
const ADMIN_LOCALE_STORAGE_KEY = "copilotmail.admin.locale.v1";

function inspectorErrorResponse({
  status,
  retryable,
  requestId,
  code,
  internalReason,
}: {
  status: 403 | 404 | 503;
  retryable: boolean;
  requestId: string;
  code: string;
  internalReason: string;
}): AdminInspectorFixtureResponse {
  return {
    status,
    body: {
      code,
      message: `${internalReason}; upstream=${INTERNAL_HOST}`,
      request_id: requestId,
      trace_id: SENSITIVE_TRACE,
      retryable,
      internal_debug: `credential_material=${SENSITIVE_KEY}`,
    },
  };
}

function successfulInspectorResponse(): AdminInspectorFixtureResponse {
  return { status: 200, body: E2E_AGENT_INSPECTOR_DTO };
}

function mailSubmissionEvidenceErrorResponse({
  requestId,
  code,
  internalReason,
}: {
  requestId: string;
  code: string;
  internalReason: string;
}): AdminMailSubmissionEvidenceFixtureResponse {
  return {
    status: 404,
    body: {
      code,
      message: `${internalReason}; upstream=${INTERNAL_HOST}`,
      request_id: requestId,
      trace_id: SENSITIVE_TRACE,
      retryable: false,
      internal_debug: `credential_material=${SENSITIVE_KEY}`,
    },
  };
}

function inspectorRequestLog(adminApp: AdminAppFixture) {
  return adminApp.requestLog.filter(({ path }) => path.endsWith("/inspector"));
}

function mailSubmissionEvidenceRequestLog(adminApp: AdminAppFixture) {
  return adminApp.requestLog.filter(({ path }) =>
    path.includes("/mail-submissions/"),
  );
}

async function submitAgentId(page: Page, agentId: string) {
  const field = page.getByRole("textbox", { name: "Agent ID" });
  await field.fill(agentId);
  await field.press("Enter");
  return field;
}

async function openProductionAgentForSubmissionEvidence(
  adminApp: AdminAppFixture,
  page: Page,
) {
  adminApp.setInspectorResponses([
    { status: 200, body: E2E_AGENT_INSPECTOR_PRODUCTION_DTO },
  ]);
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();
  await submitAgentId(page, E2E_AGENT_ID);
  await expect(
    page.getByRole("textbox", { name: "Submission ID" }),
  ).toBeVisible();
}

async function expectSensitiveErrorDetailsHidden(page: Page, code: string) {
  const body = page.locator("body");
  await expect(body).not.toContainText(INTERNAL_HOST);
  await expect(body).not.toContainText(SENSITIVE_TRACE);
  await expect(body).not.toContainText(SENSITIVE_KEY);
  await expect(body).not.toContainText(code);
  await expect(body).not.toContainText("credential_material");
}

async function switchToChinese(page: Page) {
  const rail = page.locator(".admin-navigation--rail");
  if (await rail.isVisible()) {
    await page.getByRole("combobox", { name: "Language" }).selectOption("zh-CN");
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
    await expect(
      page.getByRole("navigation", { name: "企业管理" }),
    ).toBeVisible();
    await expect(rail.getByRole("button", { name: "Agent 检视" })).toHaveText(
      "Agent 检视",
    );
    return;
  }

  // Compact and mobile shells expose session controls only from the named
  // navigation drawer. Select the locale through that visible control rather
  // than targeting the intentionally hidden desktop footer.
  const trigger = page.getByRole("button", { name: "Open navigation", exact: true });
  await expect(trigger).toBeVisible();
  await trigger.click();
  const englishDrawer = page.getByRole("dialog", {
    name: "Admin navigation",
    exact: true,
  });
  await expect(englishDrawer).toBeVisible();
  await englishDrawer.getByRole("combobox", { name: "Language" }).selectOption("zh-CN");
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");

  const drawer = page.getByRole("dialog", {
    name: "企业管理导航",
    exact: true,
  });
  await expect(
    drawer.getByRole("button", { name: "Agent 检视", exact: true }),
  ).toHaveText("Agent 检视");
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
}

async function openChineseAgentInspectorWithKeyboard(page: Page) {
  await selectAdminRoute(page, "Agent 检视", {
    drawerName: "企业管理导航",
    keyboard: true,
    triggerName: "打开导航",
  });
  await expect(
    page.getByRole("heading", { name: "Agent 控制面检视" }),
  ).toBeVisible();
}

async function prepareAgentIdForScreenshot(page: Page) {
  const field = page.getByRole("textbox", { name: "Agent ID" });
  const selection = await field.evaluate((element) => {
    const input = element as HTMLInputElement;
    input.blur();
    input.setSelectionRange(0, 0);
    input.scrollLeft = 0;
    return {
      end: input.selectionEnd,
      focused: document.activeElement === input,
      scrollLeft: input.scrollLeft,
      start: input.selectionStart,
    };
  });
  expect(selection).toEqual({
    end: 0,
    focused: false,
    scrollLeft: 0,
    start: 0,
  });
}

/**
 * The compact snapshot context holds long UUIDs, an owner, and an observation
 * timestamp. Assert the actual responsive grid rather than treating a
 * screenshot alone as proof that those facts were not compressed together.
 */
async function expectReadableInspectorContext(
  page: Page,
  expectedColumnCount: number,
) {
  const facts = page
    .getByRole("region", { name: E2E_AGENT_DISPLAY_NAME })
    .locator(".context-bar__facts");
  await expect(facts).toBeVisible();

  const layout = await facts.evaluate((node) => {
    const facts = Array.from(
      node.querySelectorAll<HTMLElement>(".context-bar__fact"),
    );
    const columns = getComputedStyle(node)
      .gridTemplateColumns.trim()
      .split(/\s+/u)
      .filter(Boolean).length;
    return {
      columns,
      documentOverflows:
        document.documentElement.scrollWidth >
        document.documentElement.clientWidth,
      valuesOverflow: facts.some((fact) => {
        const value = fact.querySelector<HTMLElement>(".context-bar__value");
        return value !== null && value.scrollWidth > value.clientWidth;
      }),
    };
  });

  expect(layout).toEqual({
    columns: expectedColumnCount,
    documentOverflows: false,
    valuesOverflow: false,
  });
}

test("Agent Inspector keyboard lookup renders one exact bounded DTO", async ({
  adminApp,
  page,
}) => {
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();

  const field = page.getByRole("textbox", { name: "Agent ID" });
  await field.focus();
  await page.keyboard.type(E2E_AGENT_ID);
  await page.keyboard.press("Enter");

  await expect(
    page.getByRole("heading", { name: E2E_AGENT_DISPLAY_NAME }),
  ).toBeVisible();
  await expect(field).toHaveValue(E2E_AGENT_ID);
  const context = page.getByRole("region", { name: E2E_AGENT_DISPLAY_NAME });
  await expect(context).toContainText(E2E_AGENT_ID);
  await expect(context).toContainText("E2E Human owner");
  await expect(context).toContainText("Testing");
  await expect(context).toContainText("Snapshot observed");
  await expect(page.getByText("Testing", { exact: true })).toHaveCount(1);
  await expect(page.getByText("Browser-contract verification")).toBeVisible();
  await expect(page.getByText("E2E Human owner")).toBeVisible();
  await expect(page.getByText("organization.read")).toBeVisible();
  await expect(page.getByText("agent.operate")).toBeVisible();
  await expect(
    page.getByRole("status").filter({ hasText: "Agent snapshot loaded." }),
  ).toBeVisible();
  expect(adminApp.inspectorReads).toEqual([E2E_AGENT_ID]);
});

test("Agent mail-submission evidence remains dormant until the Human explicitly enters one pair", async ({
  adminApp,
  page,
}) => {
  adminApp.setInspectorResponses([
    { status: 200, body: E2E_AGENT_INSPECTOR_PRODUCTION_DTO },
  ]);
  await adminApp.goto();

  // The initial workspace never infers an Agent/submission pair.
  expect(adminApp.mailSubmissionEvidenceReads).toEqual([]);
  expect(mailSubmissionEvidenceRequestLog(adminApp)).toEqual([]);

  await adminApp.openAgentInspectorWithKeyboard();
  await submitAgentId(page, E2E_AGENT_ID);
  await expect(
    page.getByRole("textbox", { name: "Submission ID" }),
  ).toBeVisible();

  // A verified Inspector snapshot is still not an authorization to prefetch
  // mail-submission evidence. The second UUID and an explicit submit remain
  // required browser actions.
  expect(adminApp.inspectorReads).toEqual([E2E_AGENT_ID]);
  expect(adminApp.mailSubmissionEvidenceReads).toEqual([]);
  expect(mailSubmissionEvidenceRequestLog(adminApp)).toEqual([]);
});

test("Agent mail-submission evidence reads exactly one submitted pair and starts with immutable references collapsed", async ({
  adminApp,
  page,
}) => {
  await openProductionAgentForSubmissionEvidence(adminApp, page);

  const submissionId = page.getByRole("textbox", { name: "Submission ID" });
  await submissionId.fill(E2E_AGENT_MAIL_SUBMISSION_ID);
  await page.getByRole("button", { name: "Read submission evidence" }).click();

  await expect(
    page.locator("section.context-bar").filter({
      hasText: E2E_AGENT_MAIL_SUBMISSION_ID,
    }),
  ).toBeVisible();
  await expect(
    page.getByText(
      "This response did not return a delivery observation. It does not mean the submission was not delivered or failed.",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(page.getByText("Not delivered", { exact: true })).toHaveCount(0);

  const auditSummary = page.getByText("Immutable audit references", {
    exact: true,
  });
  const auditDetails = auditSummary.locator("xpath=ancestor::details");
  await expect(auditDetails).not.toHaveAttribute("open");
  await expect(
    page.getByText("mail.submission.accepted", { exact: true }),
  ).not.toBeVisible();

  expect(adminApp.mailSubmissionEvidenceReads).toEqual([
    {
      agentId: E2E_AGENT_ID,
      submissionId: E2E_AGENT_MAIL_SUBMISSION_ID,
    },
  ]);
  expect(mailSubmissionEvidenceRequestLog(adminApp)).toEqual([
    {
      method: "POST",
      path: `/api/v2/agents/${E2E_AGENT_ID}/mail-submissions/${E2E_AGENT_MAIL_SUBMISSION_ID}/evidence`,
    },
  ]);
});

test("Agent mail-submission evidence rejects invalid submission UUIDs locally with zero evidence requests", async ({
  adminApp,
  page,
}) => {
  await openProductionAgentForSubmissionEvidence(adminApp, page);
  const submissionId = page.getByRole("textbox", { name: "Submission ID" });
  const submit = page.getByRole("button", { name: "Read submission evidence" });

  for (const invalidId of [
    "not-a-uuid",
    "00000000-0000-0000-0000-000000000000",
    E2E_AGENT_MAIL_SUBMISSION_ID.toUpperCase(),
  ]) {
    await submissionId.fill(invalidId);
    await submit.click();
    await expect(page.getByRole("alert")).toHaveText(
      "Enter a canonical mail-submission UUID.",
    );
  }

  expect(adminApp.mailSubmissionEvidenceReads).toEqual([]);
  expect(mailSubmissionEvidenceRequestLog(adminApp)).toEqual([]);
});

test("Agent mail-submission evidence renders distinct 404 causes as one hidden-detail target", async ({
  adminApp,
  page,
}) => {
  const code = "mail_submission_evidence_target_private";
  const firstReason = "submission row is absent from the selected tenant";
  const secondReason =
    "submission belongs to another tenant with an incomplete approval graph";
  adminApp.setMailSubmissionEvidenceResponses([
    mailSubmissionEvidenceErrorResponse({
      requestId: "support-mail-submission-404-a",
      code,
      internalReason: firstReason,
    }),
    mailSubmissionEvidenceErrorResponse({
      requestId: "support-mail-submission-404-b",
      code,
      internalReason: secondReason,
    }),
  ]);
  await openProductionAgentForSubmissionEvidence(adminApp, page);

  const submissionId = page.getByRole("textbox", { name: "Submission ID" });
  await submissionId.fill(E2E_AGENT_MAIL_SUBMISSION_ID);
  await page.getByRole("button", { name: "Read submission evidence" }).click();

  const alert = page.getByRole("alert");
  await expect(alert).toContainText("This submission cannot be shown");
  await expect(alert).toContainText("support-mail-submission-404-a");
  await expect(page.getByRole("button", { name: "Retry" })).toHaveCount(0);
  await expectSensitiveErrorDetailsHidden(page, code);
  await expect(page.locator("body")).not.toContainText(firstReason);

  await submissionId.press("Enter");
  await expect(alert).toContainText("This submission cannot be shown");
  await expect(alert).toContainText("support-mail-submission-404-b");
  await expect(alert).not.toContainText("support-mail-submission-404-a");
  await expectSensitiveErrorDetailsHidden(page, code);
  await expect(page.locator("body")).not.toContainText(secondReason);
  await expect(page.locator("body")).not.toContainText("another tenant");
  await expect(page.locator("body")).not.toContainText(
    "incomplete approval graph",
  );

  expect(adminApp.mailSubmissionEvidenceReads).toEqual([
    { agentId: E2E_AGENT_ID, submissionId: E2E_AGENT_MAIL_SUBMISSION_ID },
    { agentId: E2E_AGENT_ID, submissionId: E2E_AGENT_MAIL_SUBMISSION_ID },
  ]);
});

for (const { viewport, columns } of [
  { viewport: { width: 1024, height: 900 }, columns: 2 },
  { viewport: { width: 768, height: 900 }, columns: 1 },
] as const) {
  test(`Agent Inspector keeps context facts readable at ${viewport.width}px`, async ({
    adminApp,
    page,
  }) => {
    await page.setViewportSize(viewport);
    await adminApp.goto();
    await adminApp.openAgentInspectorWithKeyboard();
    await adminApp.lookupAgentWithKeyboard();

    await expectReadableInspectorContext(page, columns);
  });
}

test("Agent Inspector keyboard refresh preserves the explicit result", async ({
  adminApp,
  page,
}) => {
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();
  await adminApp.lookupAgentWithKeyboard();

  const refresh = page.getByRole("button", { name: "Refresh snapshot" });
  await refresh.focus();
  await page.keyboard.press("Enter");

  await expect
    .poll(() => adminApp.inspectorReads)
    .toEqual([E2E_AGENT_ID, E2E_AGENT_ID]);
  await expect(
    page.getByRole("heading", { name: E2E_AGENT_DISPLAY_NAME }),
  ).toBeVisible();
  await expect(refresh).toBeFocused();
  await expect(
    page.getByRole("status").filter({ hasText: "Agent snapshot refreshed." }),
  ).toBeVisible();
});

test("Agent Inspector rejects malformed and reserved UUIDs locally with zero Inspector requests", async ({
  adminApp,
  page,
}) => {
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();

  const field = await submitAgentId(page, "not-a-uuid");
  await expect(field).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByRole("alert")).toHaveText(
    "Enter a canonical Agent UUID.",
  );

  await submitAgentId(page, "00000000-0000-0000-0000-000000000000");
  await expect(page.getByRole("alert")).toHaveText(
    "Enter a canonical Agent UUID.",
  );

  await submitAgentId(page, "019B0000-0000-7000-8000-000000000101");
  await expect(page.getByRole("alert")).toHaveText(
    "Enter a canonical Agent UUID.",
  );
  expect(adminApp.inspectorReads).toEqual([]);
  expect(inspectorRequestLog(adminApp)).toEqual([]);
});

test("Agent Inspector keeps a 403 refusal non-retryable and hides response internals", async ({
  adminApp,
  page,
}) => {
  const code = "permission_denied_internal_policy_branch";
  adminApp.setInspectorResponses([
    inspectorErrorResponse({
      status: 403,
      retryable: false,
      requestId: "support-request-403",
      code,
      internalReason: "operator entitlement lookup failed in shard 7",
    }),
  ]);
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();
  await submitAgentId(page, E2E_AGENT_ID);

  const alert = page.getByRole("alert");
  await expect(alert).toContainText("Inspector access is not available");
  await expect(alert).toContainText("support-request-403");
  await expect(page.getByRole("button", { name: "Retry" })).toHaveCount(0);
  await expectSensitiveErrorDetailsHidden(page, code);
  expect(adminApp.inspectorReads).toEqual([E2E_AGENT_ID]);
  expect(inspectorRequestLog(adminApp)).toEqual([
    {
      method: "GET",
      path: `/api/v2/agents/${E2E_AGENT_ID}/inspector`,
    },
  ]);
});

test("Agent Inspector renders deliberately different 404 causes as one unavailable target", async ({
  adminApp,
  page,
}) => {
  const code = "agent_inspector_target_unavailable_internal";
  adminApp.setInspectorResponses([
    inspectorErrorResponse({
      status: 404,
      retryable: false,
      requestId: "support-request-404-a",
      code,
      internalReason: "target row does not exist",
    }),
    inspectorErrorResponse({
      status: 404,
      retryable: false,
      requestId: "support-request-404-b",
      code,
      internalReason:
        "target belongs to another tenant and has an incomplete profile",
    }),
  ]);
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();

  await submitAgentId(page, E2E_AGENT_ID);
  const alert = page.getByRole("alert");
  await expect(alert).toContainText("This target cannot be shown");
  await expect(alert).toContainText("support-request-404-a");
  await expect(page.getByRole("button", { name: "Retry" })).toHaveCount(0);

  await page.getByRole("textbox", { name: "Agent ID" }).press("Enter");
  await expect(alert).toContainText("This target cannot be shown");
  await expect(alert).toContainText("support-request-404-b");
  await expect(alert).not.toContainText("support-request-404-a");
  await expectSensitiveErrorDetailsHidden(page, code);
  await expect(page.locator("body")).not.toContainText("another tenant");
  await expect(page.locator("body")).not.toContainText("incomplete profile");
  expect(adminApp.inspectorReads).toEqual([E2E_AGENT_ID, E2E_AGENT_ID]);
});

for (const retryable of [false, true] as const) {
  test(`Agent Inspector honors retryable=${retryable} on a sanitized 503`, async ({
    adminApp,
    page,
  }) => {
    const code = `inspector_dependency_unavailable_${retryable}`;
    const responses: AdminInspectorFixtureResponse[] = [
      inspectorErrorResponse({
        status: 503,
        retryable,
        requestId: `support-request-503-${retryable}`,
        code,
        internalReason: "control-plane database pool exhausted",
      }),
    ];
    if (retryable) responses.push(successfulInspectorResponse());
    adminApp.setInspectorResponses(responses);
    await adminApp.goto();
    await adminApp.openAgentInspectorWithKeyboard();
    await submitAgentId(page, E2E_AGENT_ID);

    const alert = page.getByRole("alert");
    await expect(alert).toContainText("Snapshot is temporarily unavailable");
    await expect(alert).toContainText(`support-request-503-${retryable}`);
    await expectSensitiveErrorDetailsHidden(page, code);

    const retry = page.getByRole("button", { name: "Retry" });
    if (retryable) {
      await expect(retry).toBeVisible();
      await retry.click();
      await expect(
        page.getByRole("heading", { name: E2E_AGENT_DISPLAY_NAME }),
      ).toBeVisible();
      expect(adminApp.inspectorReads).toEqual([E2E_AGENT_ID, E2E_AGENT_ID]);
    } else {
      await expect(retry).toHaveCount(0);
      expect(adminApp.inspectorReads).toEqual([E2E_AGENT_ID]);
    }
  });
}

test("Agent Inspector keeps its last verified snapshot and refresh focus after a failed refresh", async ({
  adminApp,
  page,
}) => {
  const code = "refresh_snapshot_dependency_unavailable";
  adminApp.setInspectorResponses([
    successfulInspectorResponse(),
    inspectorErrorResponse({
      status: 503,
      retryable: false,
      requestId: "support-request-refresh-503",
      code,
      internalReason: "refresh projection read failed after initial snapshot",
    }),
  ]);
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();
  await adminApp.lookupAgentWithKeyboard();

  const refresh = page.getByRole("button", { name: "Refresh snapshot" });
  await refresh.focus();
  await page.keyboard.press("Enter");

  await expect(page.getByRole("alert")).toContainText(
    "Snapshot is temporarily unavailable",
  );
  await expect(page.getByRole("alert")).toContainText(
    "support-request-refresh-503",
  );
  await expect(page.getByRole("alert")).toContainText(
    "The last confirmed snapshot remains visible (observed",
  );
  await expect(page.getByRole("alert")).toContainText("It is not live status.");
  await expect(
    page.getByRole("heading", { name: E2E_AGENT_DISPLAY_NAME }),
  ).toBeVisible();
  await expect(page.getByText("Browser-contract verification")).toBeVisible();
  await expect(refresh).toBeFocused();
  await expectSensitiveErrorDetailsHidden(page, code);
  expect(adminApp.inspectorReads).toEqual([E2E_AGENT_ID, E2E_AGENT_ID]);
});

test("Admin language selection switches the live Agent Inspector contract to zh-CN", async ({
  adminApp,
  page,
}) => {
  await adminApp.goto();
  await switchToChinese(page);
  await expect
    .poll(() =>
      page.evaluate(
        (key) => window.localStorage.getItem(key),
        ADMIN_LOCALE_STORAGE_KEY,
      ),
    )
    .toBe("zh-CN");

  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  await expect(page.getByRole("combobox", { name: "语言" })).toHaveValue(
    "zh-CN",
  );
  await expect(
    page.getByRole("navigation", { name: "企业管理" }),
  ).toBeVisible();
  await openChineseAgentInspectorWithKeyboard(page);
  await adminApp.lookupAgentWithKeyboard();

  const context = page.getByRole("region", { name: E2E_AGENT_DISPLAY_NAME });
  await expect(context).toContainText(E2E_AGENT_ID);
  await expect(context).toContainText("E2E Human owner");
  await expect(context).toContainText("测试");
  await expect(context).toContainText("快照时间");
  await expect(page.getByRole("heading", { name: "边界与复核" })).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "令牌访问交集" }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "邮件控制面" })).toBeVisible();
  await expect(context.getByText("测试", { exact: true })).toBeVisible();
  await expect(page.getByText("低", { exact: true })).toBeVisible();
  await expect(page.getByText("未到期", { exact: true })).toBeVisible();
  await expect(page.getByText("有效", { exact: true })).toBeVisible();
  await expect(page.getByText("无需邮件准入", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("status").filter({ hasText: "Agent 快照已读取。" }),
  ).toBeVisible();
});

test("Agent Inspector satisfies the selected Axe WCAG tags", async ({
  adminApp,
  page,
}) => {
  await adminApp.goto();
  await adminApp.openAgentInspectorWithKeyboard();
  await adminApp.lookupAgentWithKeyboard();

  const results = await new AxeBuilder({ page })
    .include(".workspace")
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();

  expect(results.violations).toEqual([]);
});

const visualViewports = [
  { width: 1440, height: 1000 },
  { width: 1280, height: 960 },
  { width: 1024, height: 900 },
  { width: 768, height: 900 },
  { width: 390, height: 844 },
] as const;

for (const viewport of visualViewports) {
  test(`Agent Inspector screenshot — ${viewport.width}px`, async ({
    adminApp,
    page,
  }) => {
    await page.setViewportSize(viewport);
    await adminApp.goto();
    await adminApp.openAgentInspectorWithKeyboard();
    await adminApp.lookupAgentWithKeyboard();
    await prepareAgentIdForScreenshot(page);

    await expect(page).toHaveScreenshot(
      `agent-inspector-${viewport.width}.png`,
      {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      },
    );
  });

  test(`Agent Inspector zh-CN screenshot — ${viewport.width}px`, async ({
    adminApp,
    page,
  }) => {
    await page.setViewportSize(viewport);
    await adminApp.goto();
    await switchToChinese(page);
    await openChineseAgentInspectorWithKeyboard(page);
    await adminApp.lookupAgentWithKeyboard();
    await expect(page.getByText("无需邮件准入", { exact: true })).toBeVisible();
    await prepareAgentIdForScreenshot(page);

    await expect(page).toHaveScreenshot(
      `agent-inspector-zh-${viewport.width}.png`,
      {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        scale: "css",
      },
    );
  });
}
