// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, type ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import RetentionView from "./RetentionView";
import type { RetentionPolicy, RetentionPolicyVersion } from "./types";

const en = translator("en");
const zh = translator("zh-CN");
const policyId = "019b0000-0000-7000-8000-000000000601";

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<Value>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function version(
  number: number,
  overrides: Partial<RetentionPolicyVersion> = {},
): RetentionPolicyVersion {
  return {
    retention_policy_version_id: `019b0000-0000-7000-8000-0000000006${number}`,
    version: number,
    minimum_retention_days: 731,
    archive_after_days: null,
    delete_after_days: 900,
    rule: { mailbox_kind: ["human", "agent"], legal: true },
    created_by: "019b0000-0000-7000-8000-000000000699",
    created_at: "2026-08-24T01:00:00Z",
    ...overrides,
  };
}

function policy(
  latestVersion: RetentionPolicyVersion,
  overrides: Partial<RetentionPolicy> = {},
): RetentionPolicy {
  return {
    retention_policy_id: policyId,
    organization_id: "019b0000-0000-7000-8000-000000000600",
    policy_key: "audit.authoritative",
    display_name: "Authoritative evidence",
    resource_kind: "audit_evidence",
    active_version: null,
    latest_version: latestVersion,
    created_by: "019b0000-0000-7000-8000-000000000699",
    created_at: "2026-08-24T00:00:00Z",
    ...overrides,
  };
}

function renderView(client: Partial<ControlPlaneClient>) {
  return render(
    <RetentionView
      client={client as ControlPlaneClient}
      locale="en"
      t={en}
    />,
  );
}

async function loadPolicy() {
  fireEvent.change(screen.getByRole("textbox", { name: "Retention policy ID" }), {
    target: { value: policyId },
  });
  fireEvent.click(screen.getByRole("button", { name: "Load policy" }));
  await screen.findByRole("heading", { name: "Authoritative evidence" });
}

function versionEditor() {
  return screen.getByRole("form", { name: "Append draft" });
}

function versionFactsCard(label: string): HTMLElement {
  const title = within(screen.getByRole("region", { name: "Version facts" }))
    .getByText(label, { selector: "strong" });
  const card = title.closest(".retention-version");
  if (!(card instanceof HTMLElement)) {
    throw new Error(`Missing version card for ${label}`);
  }
  return card;
}

/**
 * Retention activation is intentionally a two-step operation: the first
 * submit only captures a factual review snapshot; the dialog's primary
 * action is the privileged write. Keeping this helper in the tests prevents
 * accidental coverage of a direct, confirmation-free POST path.
 */
function reviewLatestActivation() {
  fireEvent.click(screen.getByRole("button", {
    name: "Review latest-version activation",
  }));
  return screen.getByRole("dialog", { name: "Review latest-version activation" });
}

function confirmLatestActivation() {
  const review = reviewLatestActivation();
  fireEvent.click(within(review).getByRole("button", {
    name: "Approve and activate latest",
  }));
  return review;
}

function expectDraftValues({
  minimumDays,
  archiveDays,
  deleteDays,
  rule,
}: {
  minimumDays: string;
  archiveDays: string;
  deleteDays: string;
  rule: Record<string, unknown>;
}) {
  const editor = within(versionEditor());
  expect(editor.getByRole("spinbutton", { name: "Minimum retention days" }))
    .toHaveValue(Number(minimumDays));
  expect((editor.getByRole("spinbutton", { name: en("archiveAfter") }) as HTMLInputElement).value)
    .toBe(archiveDays);
  expect((editor.getByRole("spinbutton", { name: en("deleteAfter") }) as HTMLInputElement).value)
    .toBe(deleteDays);
  expect(editor.getByRole("textbox", { name: "Scope rule (JSON)" }))
    .toHaveValue(JSON.stringify(rule, null, 2));
}

function openCreatePolicy() {
  const trigger = screen.getByRole("button", { name: "Create policy draft" });
  fireEvent.click(trigger);
  return screen.getByRole("dialog", { name: "Create policy draft" });
}

function fillCreatePolicy({
  archiveMode = "not-configured",
  archiveDays,
  deleteMode = "indefinite",
  deleteDays,
  rule = "",
}: {
  archiveMode?: "not-configured" | "after-days";
  archiveDays?: string;
  deleteMode?: "indefinite" | "after-days";
  deleteDays?: string;
  rule?: string;
} = {}) {
  const dialog = screen.getByRole("dialog", { name: "Create policy draft" });
  fireEvent.change(within(dialog).getByRole("textbox", { name: "Policy key" }), {
    target: { value: "mail.explicit" },
  });
  fireEvent.change(within(dialog).getByRole("textbox", { name: "Display name" }), {
    target: { value: "Explicit retention" },
  });
  fireEvent.change(within(dialog).getByRole("combobox", { name: "Resource kind" }), {
    target: { value: "mail_content" },
  });
  fireEvent.change(within(dialog).getByRole("spinbutton", {
    name: "Minimum retention days",
  }), { target: { value: "3650" } });
  fireEvent.change(within(dialog).getByRole("combobox", {
    name: "Archive configuration",
  }), { target: { value: archiveMode } });
  if (archiveMode === "after-days") {
    fireEvent.change(within(dialog).getByRole("spinbutton", {
      name: "Archive after",
    }), { target: { value: archiveDays ?? "30" } });
  }
  fireEvent.change(within(dialog).getByRole("combobox", {
    name: "Disposition configuration",
  }), { target: { value: deleteMode } });
  if (deleteMode === "after-days") {
    fireEvent.change(within(dialog).getByRole("spinbutton", {
      name: "Dispose after",
    }), { target: { value: deleteDays ?? "4000" } });
  }
  if (rule) {
    fireEvent.change(within(dialog).getByRole("textbox", {
      name: "Scope rule (JSON)",
    }), { target: { value: rule } });
  }
}

function reviewCreatePolicy() {
  fireEvent.click(screen.getByRole("button", {
    name: "Review submission summary",
  }));
  return screen.getByRole("dialog", { name: "Review policy draft" });
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("RetentionView authoritative version editor", () => {
  it("initializes every version field exactly from a loaded latest_version", async () => {
    const latest = version(4);
    renderView({
      getRetentionPolicy: vi.fn().mockResolvedValue(policy(latest)),
    });

    await loadPolicy();

    expectDraftValues({
      minimumDays: "731",
      archiveDays: "",
      deleteDays: "900",
      rule: latest.rule,
    });
    expect(versionFactsCard("Latest draft")).toHaveTextContent(
      "Archive after: Not configured",
    );
    expect(screen.queryByText("—", { exact: true })).not.toBeInTheDocument();
  });

  it("clears an unconsumed approval when a successful read changes the latest identity", async () => {
    const initialLatest = version(4);
    const newerLatest = version(5);
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(initialLatest))
      .mockResolvedValueOnce(policy(newerLatest));
    renderView({ getRetentionPolicy });
    await loadPolicy();

    const approval = screen.getByRole("textbox", { name: "Approval request ID" });
    fireEvent.change(approval, { target: { value: "approval-for-v4" } });
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Load policy" }));
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(2));
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v5");
    expect(approval).toHaveValue("");
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeDisabled();
  });

  it("keeps creation on demand, starts without governance defaults, and restores trigger focus on cancel", () => {
    renderView({});

    const trigger = screen.getByRole("button", { name: "Create policy draft" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Policy key" }))
      .not.toBeInTheDocument();
    trigger.focus();
    const dialog = openCreatePolicy();

    const policyKey = within(dialog).getByRole("textbox", { name: "Policy key" });
    expect(policyKey).toHaveValue("");
    expect(policyKey).toHaveFocus();
    expect(within(dialog).getByRole("textbox", { name: "Display name" }))
      .toHaveValue("");
    expect(within(dialog).getByRole("combobox", { name: "Resource kind" }))
      .toHaveValue("");
    expect((within(dialog).getByRole("spinbutton", {
      name: "Minimum retention days",
    }) as HTMLInputElement).value).toBe("");
    expect(within(dialog).getByRole("combobox", {
      name: "Archive configuration",
    })).toHaveValue("");
    expect(within(dialog).getByRole("combobox", {
      name: "Disposition configuration",
    })).toHaveValue("");
    const rule = within(dialog).getByRole("textbox", { name: "Scope rule (JSON)" });
    expect(rule).toHaveValue("");
    expect(rule).not.toHaveAttribute("placeholder");
    expect(within(dialog).getByText(/Leave empty to submit the exact empty JSON object/))
      .toBeVisible();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("renders the on-demand creation contract in Simplified Chinese", () => {
    render(
      <RetentionView
        client={{} as ControlPlaneClient}
        locale="zh-CN"
        t={zh}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "建立策略草稿" }));
    const dialog = screen.getByRole("dialog", { name: "建立策略草稿" });
    expect(within(dialog).getByText(/留空会明确提交空 JSON 对象/)).toBeVisible();
    expect(within(dialog).getByRole("option", { name: "不配置" })).toBeVisible();
    expect(within(dialog).getByRole("option", { name: "无限期保留" })).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "查看提交摘要" }))
      .toBeVisible();
  });

  it("rejects missing explicit creation choices before showing a confirmation", () => {
    const createRetentionPolicy = vi.fn();
    renderView({ createRetentionPolicy });
    const dialog = openCreatePolicy();

    fireEvent.click(within(dialog).getByRole("button", {
      name: "Review submission summary",
    }));
    expect(within(dialog).getByRole("alert"))
      .toHaveTextContent("retention_create_required");
    expect(createRetentionPolicy).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Create policy draft" }))
      .toBeInTheDocument();

    fireEvent.change(within(dialog).getByRole("textbox", { name: "Policy key" }), {
      target: { value: "mail.explicit" },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Display name" }), {
      target: { value: "Explicit retention" },
    });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Resource kind" }), {
      target: { value: "mail_content" },
    });
    fireEvent.change(within(dialog).getByRole("combobox", {
      name: "Archive configuration",
    }), { target: { value: "not-configured" } });
    fireEvent.change(within(dialog).getByRole("combobox", {
      name: "Disposition configuration",
    }), { target: { value: "indefinite" } });
    fireEvent.click(within(dialog).getByRole("button", {
      name: "Review submission summary",
    }));
    expect(within(dialog).getByRole("alert"))
      .toHaveTextContent("retention_minimum_days_required");
    expect(createRetentionPolicy).not.toHaveBeenCalled();
  });

  it("shows every exact value in the review and returns to the preserved editor without a POST", () => {
    const createRetentionPolicy = vi.fn();
    renderView({ createRetentionPolicy });
    openCreatePolicy();
    fillCreatePolicy({
      archiveMode: "after-days",
      archiveDays: "45",
      deleteMode: "after-days",
      deleteDays: "4000",
      rule: '{"mailbox_kind":["human","agent"],"region":"CN"}',
    });

    const review = reviewCreatePolicy();
    expect(review).toHaveTextContent("mail.explicit");
    expect(review).toHaveTextContent("Explicit retention");
    expect(review).toHaveTextContent("Mail content");
    expect(review).toHaveTextContent("3650 days");
    expect(review).toHaveTextContent("45 days");
    expect(review).toHaveTextContent("4000 days");
    expect(review).toHaveTextContent(
      '{"mailbox_kind":["human","agent"],"region":"CN"}',
    );
    expect(createRetentionPolicy).not.toHaveBeenCalled();

    fireEvent.click(within(review).getByRole("button", { name: "Back to editing" }));
    const editor = screen.getByRole("dialog", { name: "Create policy draft" });
    expect(within(editor).getByRole("textbox", { name: "Policy key" }))
      .toHaveValue("mail.explicit");
    expect(within(editor).getByRole("spinbutton", { name: "Archive after" }))
      .toHaveValue(45);
    expect(within(editor).getByRole("spinbutton", { name: "Dispose after" }))
      .toHaveValue(4000);
  });

  it("locks confirmation while creating and preserves exact success evidence across a failed read", async () => {
    const latest = version(1, {
      minimum_retention_days: 3650,
      archive_after_days: null,
      delete_after_days: null,
      rule: {},
    });
    const created = policy(latest, {
      policy_key: "mail.explicit",
      display_name: "Server-confirmed retention",
      resource_kind: "mail_content",
    });
    const pending = deferred<RetentionPolicy>();
    const createRetentionPolicy = vi.fn().mockReturnValue(pending.promise);
    const getRetentionPolicy = vi.fn().mockRejectedValue(new Error("read unavailable"));
    renderView({ createRetentionPolicy, getRetentionPolicy });
    const trigger = screen.getByRole("button", { name: "Create policy draft" });
    trigger.focus();
    openCreatePolicy();
    fillCreatePolicy();
    const review = reviewCreatePolicy();

    expect(review).toHaveTextContent("Not configured");
    expect(review).toHaveTextContent("Retain indefinitely");
    expect(review).toHaveTextContent("{}");
    fireEvent.click(within(review).getByRole("button", {
      name: "Confirm policy creation",
    }));

    expect(createRetentionPolicy).toHaveBeenCalledWith({
      policy_key: "mail.explicit",
      display_name: "Explicit retention",
      resource_kind: "mail_content",
      minimum_retention_days: 3650,
      archive_after_days: null,
      delete_after_days: null,
      rule: {},
    });
    await waitFor(() => expect(review).toHaveAttribute("aria-busy", "true"));
    const close = within(review).getByRole("button", { name: "Close dialog" });
    const back = within(review).getByRole("button", { name: "Back to editing" });
    const confirm = within(review).getByRole("button", {
      name: "Confirm policy creation",
    });
    expect(close).toBeDisabled();
    expect(back).toBeDisabled();
    expect(confirm).toBeDisabled();
    fireEvent.click(close);
    fireEvent.click(back);
    fireEvent.click(confirm);
    fireEvent.keyDown(document, { key: "Escape" });
    if (review.parentElement) fireEvent.mouseDown(review.parentElement);
    expect(screen.getByRole("dialog", { name: "Review policy draft" }))
      .toBeInTheDocument();
    expect(createRetentionPolicy).toHaveBeenCalledOnce();

    await act(async () => {
      pending.resolve(created);
      await pending.promise;
    });

    const evidence = await screen.findByRole("status", {
      name: "Policy draft created",
    });
    expect(evidence).toHaveTextContent("Server-confirmed retention");
    expect(evidence).toHaveTextContent(created.retention_policy_id);
    expect(evidence).toHaveTextContent("Not configured");
    expect(evidence).toHaveTextContent("Retain indefinitely");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expectDraftValues({
      minimumDays: "3650",
      archiveDays: "",
      deleteDays: "",
      rule: {},
    });

    fireEvent.click(screen.getByRole("button", { name: "Load policy" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("unexpected_error");
    expect(screen.getByRole("status", { name: "Policy draft created" }))
      .toHaveTextContent(created.retention_policy_id);
  });

  it("keeps a rejected creation in review with a bounded error and editable frozen values", async () => {
    const createRetentionPolicy = vi.fn().mockRejectedValue(new ApiError(409, {
      code: "retention_create_rejected",
      message: "sensitive internal retention failure",
      request_id: "retention-create-request-409",
      trace_id: "retention-create-secret-trace",
      retryable: false,
    }));
    renderView({ createRetentionPolicy });
    openCreatePolicy();
    fillCreatePolicy({ rule: '{"classification":"restricted"}' });
    const review = reviewCreatePolicy();
    fireEvent.click(within(review).getByRole("button", {
      name: "Confirm policy creation",
    }));

    const alert = await within(review).findByRole("alert");
    expect(alert).toHaveTextContent("Policy draft not submitted");
    expect(alert).toHaveTextContent("retention_create_rejected");
    expect(alert).toHaveTextContent("retention-create-request-409");
    expect(alert).not.toHaveTextContent("sensitive internal retention failure");
    expect(alert).not.toHaveTextContent("retention-create-secret-trace");
    expect(screen.queryByRole("status", { name: "Policy draft created" }))
      .not.toBeInTheDocument();
    expect(within(review).getByRole("button", { name: "Back to editing" }))
      .toBeEnabled();

    fireEvent.click(within(review).getByRole("button", { name: "Back to editing" }));
    const editor = screen.getByRole("dialog", { name: "Create policy draft" });
    expect(within(editor).getByRole("textbox", { name: "Policy key" }))
      .toHaveValue("mail.explicit");
    expect(within(editor).getByRole("textbox", { name: "Scope rule (JSON)" }))
      .toHaveValue('{"classification":"restricted"}');
    expect(createRetentionPolicy).toHaveBeenCalledOnce();
  });

  it("sends only the explicit version-editor values and then refreshes the policy", async () => {
    const initial = policy(version(4));
    const appended = version(5, {
      minimum_retention_days: 800,
      archive_after_days: 45,
      delete_after_days: null,
      rule: { mailbox_kind: "agent", classification: "restricted" },
    });
    const refreshed = policy(appended);
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(initial)
      .mockResolvedValueOnce(refreshed);
    const appendRetentionPolicyVersion = vi.fn().mockResolvedValue(appended);
    renderView({ getRetentionPolicy, appendRetentionPolicyVersion });
    await loadPolicy();

    const editor = within(versionEditor());
    fireEvent.change(editor.getByRole("spinbutton", { name: "Minimum retention days" }), {
      target: { value: "800" },
    });
    fireEvent.change(editor.getByRole("spinbutton", { name: en("archiveAfter") }), {
      target: { value: "45" },
    });
    fireEvent.change(editor.getByRole("spinbutton", { name: en("deleteAfter") }), {
      target: { value: "" },
    });
    fireEvent.change(editor.getByRole("textbox", { name: "Scope rule (JSON)" }), {
      target: {
        value: '{"mailbox_kind":"agent","classification":"restricted"}',
      },
    });
    fireEvent.click(screen.getByRole("button", { name: "Append draft" }));

    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(2));
    expect(appendRetentionPolicyVersion).toHaveBeenCalledOnce();
    expect(appendRetentionPolicyVersion).toHaveBeenCalledWith(policyId, {
      minimum_retention_days: 800,
      archive_after_days: 45,
      delete_after_days: null,
      rule: { mailbox_kind: "agent", classification: "restricted" },
    });
    expect(within(screen.getByRole("region", { name: "Version facts" }))
      .getByText("Latest draft", { selector: "strong" })
      .closest(".retention-version")).toHaveTextContent("v5");
    expectDraftValues({
      minimumDays: "800",
      archiveDays: "45",
      deleteDays: "",
      rule: appended.rule,
    });
  });

  it("keeps creation-form edits isolated from an appended version", async () => {
    const latest = version(8);
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(latest))
      .mockResolvedValueOnce(policy(version(9)));
    const appendRetentionPolicyVersion = vi.fn().mockResolvedValue(version(9));
    renderView({ getRetentionPolicy, appendRetentionPolicyVersion });
    await loadPolicy();

    const dialog = openCreatePolicy();
    fillCreatePolicy({
      archiveMode: "after-days",
      archiveDays: "365",
      deleteMode: "after-days",
      deleteDays: "10000",
      rule: '{"unrelated":"creation-form"}',
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    fireEvent.click(screen.getByRole("button", { name: "Append draft" }));

    await waitFor(() => expect(appendRetentionPolicyVersion).toHaveBeenCalledOnce());
    expect(appendRetentionPolicyVersion).toHaveBeenCalledWith(policyId, {
      minimum_retention_days: latest.minimum_retention_days,
      archive_after_days: latest.archive_after_days,
      delete_after_days: latest.delete_after_days,
      rule: latest.rule,
    });
  });

  it("rebinds locally edited fields to vN from a manual server refresh", async () => {
    const initial = version(2);
    const current = version(7, {
      minimum_retention_days: 5000,
      archive_after_days: 60,
      delete_after_days: null,
      rule: { source: "server-v7" },
    });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(initial))
      .mockResolvedValueOnce(policy(current));
    renderView({ getRetentionPolicy });
    await loadPolicy();

    fireEvent.change(
      within(versionEditor()).getByRole("spinbutton", {
        name: "Minimum retention days",
      }),
      { target: { value: "333" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Load policy" }));

    await waitFor(() => {
      expect(
        (within(versionEditor()).getByRole("spinbutton", {
          name: "Minimum retention days",
        }) as HTMLInputElement).value,
      ).toBe("5000");
    });
    expectDraftValues({
      minimumDays: "5000",
      archiveDays: "60",
      deleteDays: "",
      rule: current.rule,
    });
  });

  it("reports invalid version JSON as a local AdminUiError without calling append", async () => {
    const appendRetentionPolicyVersion = vi.fn();
    renderView({
      getRetentionPolicy: vi.fn().mockResolvedValue(policy(version(3))),
      appendRetentionPolicyVersion,
    });
    await loadPolicy();

    fireEvent.change(
      within(versionEditor()).getByRole("textbox", { name: "Scope rule (JSON)" }),
      { target: { value: "{" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Append draft" }));

    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_rule_invalid_json");
    expect(appendRetentionPolicyVersion).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("does not submit an append when the required minimum is empty", async () => {
    const appendRetentionPolicyVersion = vi.fn();
    renderView({
      getRetentionPolicy: vi.fn().mockResolvedValue(policy(version(3))),
      appendRetentionPolicyVersion,
    });
    await loadPolicy();

    const editor = within(versionEditor());
    const minimum = editor.getByRole("spinbutton", {
      name: "Minimum retention days",
    });
    fireEvent.change(minimum, { target: { value: "" } });
    fireEvent.click(editor.getByRole("button", { name: "Append draft" }));

    expect(minimum).toBeInvalid();
    expect(appendRetentionPolicyVersion).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("allows only one append request while the confirmed result is pending", async () => {
    const appended = version(4);
    let resolveAppend: ((value: RetentionPolicyVersion) => void) | undefined;
    const pendingAppend = new Promise<RetentionPolicyVersion>((resolve) => {
      resolveAppend = resolve;
    });
    const appendRetentionPolicyVersion = vi.fn().mockReturnValue(pendingAppend);
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(version(3)))
      .mockResolvedValueOnce(policy(appended));
    renderView({ getRetentionPolicy, appendRetentionPolicyVersion });
    await loadPolicy();

    const append = screen.getByRole("button", { name: "Append draft" });
    fireEvent.click(append);
    fireEvent.click(append);

    expect(appendRetentionPolicyVersion).toHaveBeenCalledOnce();
    expect(screen.getByRole("status")).toHaveTextContent("Loading");
    expect(append).toBeDisabled();

    resolveAppend?.(appended);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
  });

  it("keeps a created version separate from latest and locks mutations until refresh succeeds", async () => {
    const initial = policy(version(4));
    const appended = version(5, {
      minimum_retention_days: 1200,
      rule: { source: "confirmed-append" },
    });
    const concurrentLatest = version(6, {
      minimum_retention_days: 1300,
      rule: { source: "concurrent-server-version" },
    });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(initial)
      .mockRejectedValueOnce(new Error("policy refresh unavailable"))
      .mockResolvedValueOnce(policy(concurrentLatest));
    const appendRetentionPolicyVersion = vi.fn().mockResolvedValue(appended);
    renderView({ getRetentionPolicy, appendRetentionPolicyVersion });
    await loadPolicy();

    fireEvent.click(screen.getByRole("button", { name: "Append draft" }));

    const committed = (await screen.findByText("Retention change committed"))
      .closest("[role='status']");
    expect(committed).not.toBeNull();
    expect(committed).toHaveTextContent("Retention change committed");
    expect(committed).toHaveTextContent("Created version v5");
    expect(versionFactsCard("Last confirmed latest draft")).toHaveTextContent("v4");
    expect(screen.getByRole("button", { name: "Append draft" })).toBeDisabled();
    const approval = screen.getByRole("textbox", { name: "Approval request ID" });
    expect(approval).toBeDisabled();
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("unexpected_error");

    fireEvent.click(screen.getByRole("button", { name: "Load policy" }));

    await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v6");
    expect(screen.getByRole("button", { name: "Append draft" })).toBeEnabled();
    expect(approval).toBeEnabled();
    expectDraftValues({
      minimumDays: "1300",
      archiveDays: "",
      deleteDays: "900",
      rule: concurrentLatest.rule,
    });
  });

  it("binds an activated version before a failed full-policy refresh", async () => {
    const latest = version(7);
    const activated = { ...latest, created_at: "2026-08-24T03:00:00Z" };
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(latest))
      .mockRejectedValueOnce(new Error("policy refresh unavailable"));
    const activateRetentionPolicyVersion = vi.fn().mockResolvedValue(activated);
    renderView({ getRetentionPolicy, activateRetentionPolicyVersion });
    await loadPolicy();

    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-request-7" },
    });
    const activationReview = confirmLatestActivation();
    expect(activationReview).toHaveTextContent(policyId);
    expect(activationReview).toHaveTextContent(latest.retention_policy_version_id);
    expect(activationReview).toHaveTextContent("approval-request-7");

    const committed = (await screen.findByText("Retention change committed"))
      .closest("[role='status']");
    expect(committed).not.toBeNull();
    expect(committed).toHaveTextContent("Activated version v7");
    expect(versionFactsCard("Active version")).toHaveTextContent("v7");
    expect(screen.getByRole("textbox", { name: "Approval request ID" }))
      .toHaveValue("");
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeDisabled();
    expect(activateRetentionPolicyVersion).toHaveBeenCalledWith(
      policyId,
      latest.retention_policy_version_id,
      "approval-request-7",
    );
  });

  it("rejects stale successful snapshots until a manual read reaches the created version", async () => {
    const initialLatest = version(4);
    const appended = version(5, {
      minimum_retention_days: 1400,
      rule: { source: "confirmed-v5" },
    });
    const concurrentLatest = version(6, {
      minimum_retention_days: 1500,
      rule: { source: "server-v6" },
    });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(initialLatest))
      .mockResolvedValueOnce(policy(initialLatest))
      .mockResolvedValueOnce(policy(initialLatest))
      .mockResolvedValueOnce(policy(concurrentLatest));
    const appendRetentionPolicyVersion = vi.fn().mockResolvedValue(appended);
    renderView({ getRetentionPolicy, appendRetentionPolicyVersion });
    await loadPolicy();

    fireEvent.click(screen.getByRole("button", { name: "Append draft" }));

    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_snapshot_stale");
    expect(screen.getByText("Retention change committed").closest("[role='status']"))
      .toHaveTextContent("Created version v5");
    expect(versionFactsCard("Last confirmed latest draft")).toHaveTextContent("v4");
    expect(screen.getByRole("button", { name: "Append draft" })).toBeDisabled();

    const load = screen.getByRole("button", { name: "Load policy" });
    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(load).toBeEnabled());
    expect(screen.getByRole("alert")).toHaveTextContent("retention_snapshot_stale");
    expect(screen.getByText("Retention change committed")).toBeVisible();
    expect(screen.getByRole("button", { name: "Append draft" })).toBeDisabled();

    fireEvent.click(load);
    await waitFor(() => expect(screen.queryByText("Retention change committed"))
      .not.toBeInTheDocument());
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v6");
    expect(screen.getByRole("button", { name: "Append draft" })).toBeEnabled();
  });

  it("keeps the accepted latest high-water across later regressive and identity-conflicting reads", async () => {
    const initialLatest = version(4);
    const appended = version(5, { rule: { source: "confirmed-v5" } });
    const concurrentLatest = version(6, { rule: { source: "accepted-v6" } });
    const conflictingV6 = version(6, {
      retention_policy_version_id: "019b0000-0000-7000-8000-00000000bad6",
      rule: { source: "conflicting-v6" },
    });
    const regressedLatest = version(5, { rule: { source: "regressed-v5" } });
    const advancedLatest = version(7, { rule: { source: "accepted-v7" } });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(initialLatest))
      .mockResolvedValueOnce(policy(concurrentLatest))
      .mockResolvedValueOnce(policy(conflictingV6))
      .mockResolvedValueOnce(policy(regressedLatest))
      .mockResolvedValueOnce(policy(advancedLatest));
    const appendRetentionPolicyVersion = vi.fn().mockResolvedValue(appended);
    renderView({ getRetentionPolicy, appendRetentionPolicyVersion });
    await loadPolicy();

    fireEvent.click(screen.getByRole("button", { name: "Append draft" }));
    await waitFor(() => expect(screen.queryByText("Retention change committed"))
      .not.toBeInTheDocument());
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v6");

    const load = screen.getByRole("button", { name: "Load policy" });
    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(3));
    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_snapshot_stale");
    expect(versionFactsCard("Last confirmed latest draft")).toHaveTextContent("v6");
    expect(screen.getByRole("button", { name: "Append draft" })).toBeDisabled();

    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(4));
    expect(screen.getByRole("alert")).toHaveTextContent("retention_snapshot_stale");
    expect(versionFactsCard("Last confirmed latest draft")).toHaveTextContent("v6");

    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(5));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v7");
    expect(screen.getByRole("button", { name: "Append draft" })).toBeEnabled();
  });

  it("rejects a historical version identity conflict when a prior latest later appears as active", async () => {
    const priorActive = version(5);
    const seenLatest = version(6, { rule: { source: "seen-v6" } });
    const newerLatest = version(7, { rule: { source: "newer-v7" } });
    const conflictingActive = version(6, {
      retention_policy_version_id: "019b0000-0000-7000-8000-00000000bad6",
      rule: { source: "conflicting-active-v6" },
    });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(seenLatest, { active_version: priorActive }))
      .mockResolvedValueOnce(policy(newerLatest, { active_version: conflictingActive }))
      .mockResolvedValueOnce(policy(newerLatest, { active_version: seenLatest }));
    renderView({ getRetentionPolicy });
    await loadPolicy();

    const load = screen.getByRole("button", { name: "Load policy" });
    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_snapshot_stale");
    expect(versionFactsCard("Last confirmed latest draft")).toHaveTextContent(
        seenLatest.retention_policy_version_id,
      );
    expect(versionFactsCard("Active version")).toHaveTextContent(
        priorActive.retention_policy_version_id,
      );

    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v7");
    expect(versionFactsCard("Active version")).toHaveTextContent(
        seenLatest.retention_policy_version_id,
      );
  });

  it("registers an appended write identity before validating its reconciliation snapshot", async () => {
    const initialLatest = version(4);
    const appended = version(5, { rule: { source: "append-v5" } });
    const newerLatest = version(6, { rule: { source: "newer-v6" } });
    const conflictingActive = version(5, {
      retention_policy_version_id: "019b0000-0000-7000-8000-00000000bad5",
      rule: { source: "conflicting-active-v5" },
    });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(initialLatest))
      .mockResolvedValueOnce(policy(newerLatest, { active_version: conflictingActive }))
      .mockResolvedValueOnce(policy(newerLatest, { active_version: appended }));
    const appendRetentionPolicyVersion = vi.fn().mockResolvedValue(appended);
    renderView({ getRetentionPolicy, appendRetentionPolicyVersion });
    await loadPolicy();

    fireEvent.click(screen.getByRole("button", { name: "Append draft" }));
    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_snapshot_stale");
    expect(screen.getByText("Retention change committed").closest("[role='status']"))
      .toHaveTextContent("Created version v5");
    expect(versionFactsCard("Last confirmed latest draft")).toHaveTextContent("v4");

    fireEvent.click(screen.getByRole("button", { name: "Load policy" }));
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v6");
    expect(versionFactsCard("Active version")).toHaveTextContent(
        appended.retention_policy_version_id,
      );
  });

  it.each([
    ["a different returned version", version(8)],
    [
      "a different ID for the requested version",
      version(7, {
        retention_policy_version_id: "019b0000-0000-7000-8000-00000000bad7",
      }),
    ],
  ])("rejects %s from activation without patching or advancing its high-water", async (
    _case,
    invalidActivation,
  ) => {
    const priorActive = version(6);
    const latest = version(7);
    const initial = policy(latest, { active_version: priorActive });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(initial)
      .mockResolvedValueOnce(initial);
    const activateRetentionPolicyVersion = vi.fn().mockResolvedValue(invalidActivation);
    renderView({ getRetentionPolicy, activateRetentionPolicyVersion });
    await loadPolicy();

    const approval = screen.getByRole("textbox", { name: "Approval request ID" });
    fireEvent.change(approval, { target: { value: "approval-request-7" } });
    confirmLatestActivation();

    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_snapshot_stale");
    expect(activateRetentionPolicyVersion).toHaveBeenCalledOnce();
    expect(getRetentionPolicy).toHaveBeenCalledOnce();
    expect(screen.queryByText("Retention change committed")).not.toBeInTheDocument();
    expect(versionFactsCard("Active version")).toHaveTextContent("v6");
    expect(versionFactsCard("Last confirmed latest draft")).toHaveTextContent("v7");
    expect(approval).toHaveValue("");
    expect(approval).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Load policy" }));
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v7");
    expect(versionFactsCard("Active version")).toHaveTextContent("v6");
    expect(approval).toHaveValue("");
    expect(approval).toBeEnabled();
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeDisabled();
  });

  it("keeps an activation high-water across later regressions and never restores its consumed approval", async () => {
    const priorActive = version(6);
    const latest = version(7);
    const activated = { ...latest, created_at: "2026-08-24T03:00:00Z" };
    const concurrentActive = version(8, {
      created_at: "2026-08-24T04:00:00Z",
      rule: { source: "concurrent-active-v8" },
    });
    const laterLatest = version(9, {
      created_at: "2026-08-24T05:00:00Z",
      rule: { source: "later-latest-v9" },
    });
    const conflictingActive = version(8, {
      retention_policy_version_id: "019b0000-0000-7000-8000-00000000bad8",
      created_at: "2026-08-24T04:00:00Z",
    });
    const getRetentionPolicy = vi.fn()
      .mockResolvedValueOnce(policy(latest, { active_version: priorActive }))
      .mockResolvedValueOnce(policy(latest, { active_version: priorActive }))
      .mockResolvedValueOnce(policy(concurrentActive, { active_version: concurrentActive }))
      .mockResolvedValueOnce(policy(laterLatest, { active_version: conflictingActive }))
      .mockResolvedValueOnce(policy(laterLatest, { active_version: activated }));
    const activateRetentionPolicyVersion = vi.fn().mockResolvedValue(activated);
    renderView({ getRetentionPolicy, activateRetentionPolicyVersion });
    await loadPolicy();

    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-request-7" },
    });
    confirmLatestActivation();

    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_snapshot_stale");
    expect(screen.getByText("Retention change committed").closest("[role='status']"))
      .toHaveTextContent("Activated version v7");
    expect(versionFactsCard("Active version")).toHaveTextContent("v7");
    const approval = screen.getByRole("textbox", { name: "Approval request ID" });
    expect(approval).toHaveValue("");
    expect(approval).toBeDisabled();
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeDisabled();

    const load = screen.getByRole("button", { name: "Load policy" });
    fireEvent.click(load);
    await waitFor(() => expect(screen.queryByText("Retention change committed"))
      .not.toBeInTheDocument());
    expect(versionFactsCard("Active version")).toHaveTextContent("v8");
    expect(versionFactsCard("Latest draft")).toHaveTextContent("v8");
    expect(approval).toHaveValue("");
    expect(approval).toBeEnabled();
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeDisabled();

    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(4));
    expect(await screen.findByRole("alert"))
      .toHaveTextContent("retention_snapshot_stale");
    expect(versionFactsCard("Active version")).toHaveTextContent("v8");
    expect(approval).toHaveValue("");
    expect(approval).toBeDisabled();

    fireEvent.click(load);
    await waitFor(() => expect(getRetentionPolicy).toHaveBeenCalledTimes(5));
    expect(screen.getByRole("alert")).toHaveTextContent("retention_snapshot_stale");
    expect(versionFactsCard("Active version")).toHaveTextContent("v8");
    expect(screen.getByRole("button", { name: "Review latest-version activation" }))
      .toBeDisabled();
  });
});
