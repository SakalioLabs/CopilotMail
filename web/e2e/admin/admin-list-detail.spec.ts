import type { Locator, Page } from "@playwright/test";
import {
  type AdminAppFixture,
  E2E_APPROVAL_ID,
  E2E_ARCHIVE_ID,
  E2E_AUDIT_EVENT_ID,
  E2E_AUDIT_QUERY_ID,
  E2E_CASE_ID,
  E2E_COMPLIANCE_CASE,
  E2E_FIXED_NOW,
  E2E_HOLD_ID,
  E2E_LEGAL_HOLD,
  expect,
  selectAdminRoute,
  test,
} from "./fixtures";

/**
 * Browser evidence for the shared Admin list-detail contract. The fixture
 * answers only the real endpoints used by each screen. The one Case detail
 * read below is the contractual `LegalHold.case_id` relationship, never a
 * list-derived association.
 */

interface ListDetailScenario {
  readonly key: "cases" | "holds" | "approvals" | "audit";
  readonly navigationName: string;
  readonly pageHeading: string;
  readonly listLabel: string;
  readonly rowName: string;
  readonly detailTitle: string;
  readonly detailEvidence: string;
  readonly backLabel: string;
}

function shortOperationalId(value: string): string {
  return `${value.slice(0, 8)}…${value.slice(-6)}`;
}

function shortAuditId(value: string): string {
  return `${value.slice(0, 10)}…${value.slice(-8)}`;
}

const scenarios: readonly ListDetailScenario[] = [
  {
    key: "cases",
    navigationName: "Compliance cases",
    pageHeading: "Compliance cases",
    listLabel: "Compliance cases",
    rowName: E2E_COMPLIANCE_CASE.title,
    detailTitle: E2E_COMPLIANCE_CASE.title,
    detailEvidence: E2E_COMPLIANCE_CASE.purpose,
    backLabel: "Back to cases",
  },
  {
    key: "holds",
    navigationName: "Legal holds",
    pageHeading: "Legal holds",
    listLabel: "Legal holds",
    rowName: shortOperationalId(E2E_ARCHIVE_ID),
    detailTitle: shortOperationalId(E2E_HOLD_ID),
    detailEvidence: E2E_LEGAL_HOLD.reason,
    backLabel: "Back to legal holds",
  },
  {
    key: "approvals",
    navigationName: "Approvals",
    pageHeading: "Enterprise approvals",
    listLabel: "Enterprise approvals",
    rowName: "audit.export",
    detailTitle: shortOperationalId(E2E_APPROVAL_ID),
    detailEvidence: "Export the signed archive for the domain-activation review.",
    backLabel: "Back to approvals",
  },
  {
    key: "audit",
    navigationName: "Audit search",
    pageHeading: "Immutable audit evidence",
    listLabel: "Results queue",
    rowName: "mail_domain.activated",
    detailTitle: shortAuditId(E2E_AUDIT_EVENT_ID),
    detailEvidence: "corp.example",
    backLabel: "Back to audit events",
  },
] as const;

async function freezeAdminClock(page: Page) {
  await page.clock.setFixedTime(new Date(E2E_FIXED_NOW));
}

async function openScenario(
  adminApp: AdminAppFixture,
  page: Page,
  scenario: ListDetailScenario,
) {
  await adminApp.goto();
  await selectAdminRoute(page, scenario.navigationName);
  await expect(page.getByRole("heading", {
    level: 1,
    name: scenario.pageHeading,
    exact: true,
  })).toBeVisible();

  const shell = page.locator(".admin-list-detail");
  await expect(shell).toBeVisible();
  const list = shell.locator(".admin-list-detail__list");
  const detail = shell.locator(".admin-list-detail__detail");
  const row = list.getByRole("button", { name: scenario.rowName, exact: true });
  await expect(row).toBeVisible();
  return { shell, list, detail, row };
}

async function expectFocusedDetail(
  detail: Locator,
  scenario: ListDetailScenario,
) {
  await expect(detail).toBeVisible();
  const title = detail.locator("h2.admin-list-detail__title");
  await expect(title).toHaveText(scenario.detailTitle);
  await expect(title).toBeFocused();
  return title;
}

const narrowViewports = [
  { width: 1080, height: 900 },
  { width: 720, height: 960 },
] as const;

for (const viewport of narrowViewports) {
  for (const scenario of scenarios) {
    test(`${scenario.key} is a real narrow list-detail flow at ${viewport.width}px`, async ({
      adminApp,
      page,
    }) => {
      await page.setViewportSize(viewport);
      await freezeAdminClock(page);
      const { list, detail, row } = await openScenario(adminApp, page, scenario);

      // No empty detail panel competes with the list on the narrow screen.
      await expect(list).toBeVisible();
      await expect(detail).toBeHidden();

      // Keyboard selection moves to a single detail surface and announces its
      // stable h2, including while Approval is loading its separate DTO.
      await row.focus();
      await row.press("Enter");
      await expect(list).toBeHidden();
      await expectFocusedDetail(detail, scenario);

      const back = detail.getByRole("button", {
        name: scenario.backLabel,
        exact: true,
      });
      await back.click();
      await expect(list).toBeVisible();
      await expect(detail).toBeHidden();
      await expect(row).toBeFocused();

      // Pointer selection has the same contract; Escape is an explicit Back
      // and restores the exact row rather than the start of the list.
      await row.click();
      await expectFocusedDetail(detail, scenario);
      await page.keyboard.press("Escape");
      await expect(list).toBeVisible();
      await expect(detail).toBeHidden();
      await expect(row).toBeFocused();

      // Commit one focused detail state per screen and boundary as reviewable
      // visual evidence (four screens × two narrow breakpoints).
      await row.click();
      await expectFocusedDetail(detail, scenario);
      await expect(detail.getByText(scenario.detailEvidence, { exact: true })).toBeVisible();
      await expect(page).toHaveScreenshot(
        `admin-list-detail-${scenario.key}-${viewport.width}.png`,
        {
          animations: "disabled",
          caret: "hide",
          fullPage: true,
          scale: "css",
        },
      );
    });
  }
}

test("all four desktop screens keep two columns and ignore Escape", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await freezeAdminClock(page);
  await adminApp.goto();

  for (const scenario of scenarios) {
    await selectAdminRoute(page, scenario.navigationName);
    const shell = page.locator(".admin-list-detail");
    await expect(shell).toBeVisible();
    const list = shell.locator(".admin-list-detail__list");
    const detail = shell.locator(".admin-list-detail__detail");
    const row = list.getByRole("button", { name: scenario.rowName, exact: true });
    await row.click();
    await expect(detail.locator("h2.admin-list-detail__title")).toHaveText(
      scenario.detailTitle,
    );
    await expect(list).toBeVisible();
    await expect(detail).toBeVisible();
    await expect(detail.getByRole("button", {
      name: scenario.backLabel,
      exact: true,
    })).toBeHidden();

    const [listBox, detailBox] = await Promise.all([
      list.boundingBox(),
      detail.boundingBox(),
    ]);
    expect(listBox).not.toBeNull();
    expect(detailBox).not.toBeNull();
    expect(listBox!.x + listBox!.width).toBeLessThanOrEqual(detailBox!.x);

    await page.keyboard.press("Escape");
    await expect(shell).toHaveAttribute("data-detail-open", "true");
    await expect(detail.locator("h2.admin-list-detail__title")).toHaveText(
      scenario.detailTitle,
    );
  }
});

test("cases and legal holds keep evidence factual, collapsed, and bound to the selected resource", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await freezeAdminClock(page);
  await adminApp.goto();
  await selectAdminRoute(page, "Compliance cases");

  const caseRow = page.getByRole("button", {
    name: E2E_COMPLIANCE_CASE.title,
    exact: true,
  });
  await caseRow.click();
  const caseDetail = page.locator(".case-hold-detail");
  const caseContext = caseDetail.getByRole("region", { name: "Case context", exact: true });
  const caseEvidence = caseDetail.getByText("Case evidence", { exact: true })
    .locator("xpath=ancestor::details");

  await expect(caseContext).toContainText(E2E_COMPLIANCE_CASE.case_id);
  await expect(caseContext).toContainText(E2E_COMPLIANCE_CASE.external_reference);
  await expect(caseContext).toContainText("Open");
  await expect(caseDetail.getByRole("heading", { name: "Case facts", exact: true })).toBeVisible();
  await expect(caseDetail.getByText(E2E_COMPLIANCE_CASE.purpose, { exact: true })).toBeVisible();
  await expect(caseDetail.getByRole("heading", { name: "Current case decision", exact: true })).toBeVisible();
  await expect(caseEvidence).not.toHaveAttribute("open", "");
  await expect(caseEvidence).toContainText(E2E_COMPLIANCE_CASE.created_by);

  await caseDetail.getByRole("button", { name: "Close case", exact: true }).click();
  const closeDialog = page.getByRole("dialog", { name: "Close case", exact: true });
  await expect(closeDialog).toContainText(E2E_COMPLIANCE_CASE.case_id);
  await expect(closeDialog).toContainText(E2E_COMPLIANCE_CASE.external_reference);
  await expect(closeDialog).toContainText("After confirmation, this case will become closed.");
  await page.keyboard.press("Escape");
  await expect(closeDialog).toBeHidden();

  await caseContext.getByRole("button", { name: "View holds for this case", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Legal holds", exact: true })).toBeVisible();
  await expect(page.locator(".filter-chip")).toContainText(E2E_CASE_ID.slice(0, 8));
  await expect.poll(() => adminApp.unexpectedApiRequests).toEqual([]);

  const holdRow = page.getByRole("button", {
    name: shortOperationalId(E2E_ARCHIVE_ID),
    exact: true,
  });
  await holdRow.click();
  const holdDetail = page.locator(".case-hold-detail");
  const holdContext = holdDetail.getByRole("region", { name: "Legal hold context", exact: true });
  const holdEvidence = holdDetail.getByText("Legal-hold evidence", { exact: true })
    .locator("xpath=ancestor::details");

  await expect(holdContext).toContainText(E2E_LEGAL_HOLD.legal_hold_id);
  await expect(holdContext).toContainText(E2E_LEGAL_HOLD.archive_id);
  await expect(holdContext).toContainText(E2E_LEGAL_HOLD.case_id);
  await expect(holdDetail.getByRole("heading", { name: "Legal hold facts", exact: true })).toBeVisible();
  await expect(holdDetail.getByText(E2E_LEGAL_HOLD.reason, { exact: true })).toBeVisible();
  await expect(holdEvidence).not.toHaveAttribute("open", "");
  await expect(holdEvidence).toContainText(E2E_LEGAL_HOLD.apply_approval_request_id);

  await holdDetail.getByRole("button", { name: "Release legal hold", exact: true }).click();
  const releaseDialog = page.getByRole("dialog", { name: "Release legal hold", exact: true });
  await expect(releaseDialog).toContainText(E2E_LEGAL_HOLD.legal_hold_id);
  await expect(releaseDialog).toContainText(E2E_LEGAL_HOLD.archive_id);
  await expect(releaseDialog).toContainText(E2E_LEGAL_HOLD.apply_approval_request_id);
  await expect(releaseDialog.getByRole("textbox", { name: "Approval request ID", exact: true })).toBeVisible();
  await expect(releaseDialog).toContainText("After the server verifies the approval request you enter, this legal hold will be released.");
});

test("a legal hold opens its contract-backed case only after an exact Case read", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await freezeAdminClock(page);
  await adminApp.goto();
  await selectAdminRoute(page, "Legal holds");

  await page.getByRole("button", {
    name: shortOperationalId(E2E_ARCHIVE_ID),
    exact: true,
  }).click();
  const holdDetail = page.locator(".case-hold-detail");
  const requestLogStart = adminApp.requestLog.length;
  await holdDetail.getByRole("button", { name: "View linked case", exact: true }).click();

  await expect(page.getByRole("heading", { name: "Compliance cases", exact: true })).toBeVisible();
  const caseDetail = page.locator(".case-hold-detail");
  await expect(caseDetail.getByRole("heading", {
    name: E2E_COMPLIANCE_CASE.title,
    exact: true,
  })).toBeVisible();
  await expect(caseDetail.getByRole("region", { name: "Case context", exact: true }))
    .toContainText(E2E_CASE_ID);

  const exactCaseReads = adminApp.requestLog.slice(requestLogStart).filter((request) => (
    request.method === "GET"
    && request.path === `/api/v2/compliance-cases/${E2E_CASE_ID}`
  ));
  expect(exactCaseReads).toEqual([{
    method: "GET",
    path: `/api/v2/compliance-cases/${E2E_CASE_ID}`,
  }]);
  await expect.poll(() => adminApp.unexpectedApiRequests).toEqual([]);
});

test("audit investigation separates the query, evidence queue, and closed export security gate", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await freezeAdminClock(page);
  await adminApp.goto();
  await selectAdminRoute(page, "Audit search");

  const query = page.getByRole("region", { name: "Query context", exact: true });
  const conditions = page.getByRole("form", { name: "Query conditions", exact: true });
  const queue = page.getByRole("region", { name: "Results queue", exact: true });
  const gateSummary = page.getByText("Export authorization", { exact: true });
  const gate = gateSummary.locator("xpath=ancestor::details");

  await expect(query).toContainText(E2E_AUDIT_QUERY_ID);
  await expect(query).toContainText("No additional filters");
  await expect(query).toContainText("1");
  await expect(conditions).toBeVisible();
  await expect(queue).toContainText("mail_domain.activated");
  await expect(gate).toContainText("High-risk security gate");
  await expect(gate.locator(".audit-export-risk")).toBeVisible();
  await expect(gate).not.toHaveAttribute("open", "");
  await expect(gate.getByRole("textbox", { name: "Audit archive ID" })).toBeHidden();

  const row = queue.getByRole("button", { name: "mail_domain.activated", exact: true });
  await row.click();
  await expect(page.getByRole("region", { name: "Event facts", exact: true }))
    .toContainText(E2E_AUDIT_EVENT_ID);
  await expect(page.getByText("Hash-chain evidence", { exact: true }).locator("xpath=ancestor::details"))
    .not.toHaveAttribute("open", "");
  await expect(gate).not.toHaveAttribute("open", "");
  // The selected row can sit near the viewport fold. Resetting its transient
  // browser scroll before collecting the full-page baseline keeps visual
  // evidence about layout, rather than an incidental focus scroll.
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  // The desktop rail is fixed. A full-document page capture can stitch that
  // fixed element at an incidental scroll segment after focus restoration, so
  // capture the actual Audit workspace instead of accepting a flaky rail
  // duplicate. The semantic assertions above keep the shell contract covered.
  await expect(page.locator(".workspace")).toHaveScreenshot("audit-investigation-1440.png", {
    animations: "disabled",
    caret: "hide",
    scale: "css",
  });
});

for (const { width, detailIsExclusive } of [
  { width: 1279, detailIsExclusive: false },
  { width: 1024, detailIsExclusive: true },
] as const) {
  test(`audit investigation preserves readable selected evidence at ${width}px`, async ({
    adminApp,
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await freezeAdminClock(page);
    await adminApp.goto();
    await selectAdminRoute(page, "Audit search");

    const shell = page.locator(".audit-results");
    const queue = shell.getByRole("region", { name: "Results queue", exact: true });
    const detail = shell.locator(".audit-detail");
    const row = queue.getByRole("button", { name: "mail_domain.activated", exact: true });
    await row.click();

    await expect(detail.getByRole("region", { name: "Event facts", exact: true }))
      .toContainText(E2E_AUDIT_EVENT_ID);
    await expect.poll(() => page.evaluate(() => ({
      body: document.body.scrollWidth <= document.body.clientWidth,
      root: document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    }))).toEqual({ body: true, root: true });

    if (detailIsExclusive) {
      // <=1080 keeps the existing focused-detail navigation contract. It is a
      // full-width evidence view, not a compressed side panel.
      await expect(queue).toBeHidden();
      await expect(detail).toBeVisible();
      await expect(detail.getByRole("button", { name: "Back to audit events", exact: true }))
        .toBeVisible();
    } else {
      // Intermediate desktops retain both real surfaces but place them in
      // document reading order so the factual detail is not forced into a
      // ~330px column.
      await expect(queue).toBeVisible();
      await expect(detail).toBeVisible();
      const [queueBox, detailBox] = await Promise.all([
        queue.boundingBox(),
        detail.boundingBox(),
      ]);
      expect(queueBox).not.toBeNull();
      expect(detailBox).not.toBeNull();
      expect(queueBox!.y + queueBox!.height).toBeLessThanOrEqual(detailBox!.y);
      expect(detailBox!.width).toBeGreaterThanOrEqual(queueBox!.width * 0.98);
      await expect(detail.getByRole("button", { name: "Back to audit events", exact: true }))
        .toBeHidden();
    }

    await expect(page).toHaveScreenshot(`audit-investigation-${width}.png`, {
      animations: "disabled",
      caret: "hide",
      fullPage: true,
      scale: "css",
    });
  });
}

test("the 1081px desktop boundary never creates document overflow", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 1081, height: 900 });
  await freezeAdminClock(page);
  await adminApp.goto();

  for (const scenario of scenarios) {
    await selectAdminRoute(page, scenario.navigationName);
    const shell = page.locator(".admin-list-detail");
    const row = shell.getByRole("button", { name: scenario.rowName, exact: true });
    await expect(row).toBeVisible();
    await row.click();
    await expect(shell.locator("h2.admin-list-detail__title")).toHaveText(
      scenario.detailTitle,
    );
    await expect.poll(() => page.evaluate(() => ({
      body: document.body.scrollWidth <= document.body.clientWidth,
      root: document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    }))).toEqual({ body: true, root: true });
  }
});

test("a late Approval detail response cannot reopen a narrow selection", async ({
  adminApp,
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 960 });
  await freezeAdminClock(page);
  adminApp.delayNextApprovalDetail();
  const scenario = scenarios.find(({ key }) => key === "approvals")!;
  const { shell, list, detail, row } = await openScenario(adminApp, page, scenario);

  await row.focus();
  await row.press("Enter");
  await expect.poll(() => adminApp.approvalDetailReads).toEqual([E2E_APPROVAL_ID]);
  await expectFocusedDetail(detail, scenario);
  await expect(detail.getByText("Loading…", { exact: true })).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(list).toBeVisible();
  await expect(detail).toBeHidden();
  await expect(row).toBeFocused();

  const lateResponse = page.waitForResponse((response) => (
    response.request().method() === "GET"
      && new URL(response.url()).pathname === `/api/v2/approval-requests/${E2E_APPROVAL_ID}`
  ));
  adminApp.releaseApprovalDetail();
  await lateResponse;

  await expect(shell).toHaveAttribute("data-detail-open", "false");
  await expect(list).toBeVisible();
  await expect(detail).toBeHidden();
  await expect(row).toBeFocused();
  await expect(detail.locator("h2.admin-list-detail__title")).toHaveCount(0);
});
