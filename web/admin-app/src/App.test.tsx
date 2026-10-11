// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AdaptiveNavigation,
  AdminNavigation,
  AdminSessionControls,
  ConfirmDialog,
  Dialog,
  Overview,
  RecoverableRouteBoundary,
} from "./App";
import { CasesView, HoldsView, Pagination } from "./ComplianceViews";
import { ApiError, ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type { ComplianceCase, LegalHold } from "./types";

const adminStyles = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");

function unavailableError(resource: string): ApiError {
  return new ApiError(503, {
    code: `${resource}_unavailable`,
    message: `${resource} is temporarily unavailable`,
    request_id: `${resource}-request-503`,
    trace_id: `${resource}-trace-503`,
    retryable: true,
  });
}

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<Value>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function caseRecord(
  overrides: Partial<ComplianceCase> = {},
): ComplianceCase {
  return {
    case_id: "case-mutation-1",
    organization_id: "organization-1",
    external_reference: "CASE-MUTATION-1",
    title: "Mutation case",
    purpose: "Verify monotonic case closure",
    status: "open",
    created_by: "principal-1",
    created_at: "2026-08-24T01:00:00Z",
    closed_by: null,
    closed_at: null,
    ...overrides,
  };
}

function holdRecord(
  overrides: Partial<LegalHold> = {},
): LegalHold {
  return {
    legal_hold_id: "hold-mutation-1",
    organization_id: "organization-1",
    case_id: "case-mutation-1",
    archive_id: "archive-mutation-1",
    reason: "Verify monotonic hold release",
    status: "active",
    apply_approval_request_id: "approval-apply-1",
    created_by: "principal-1",
    created_at: "2026-08-24T01:00:00Z",
    release_approval_request_id: null,
    released_by: null,
    released_at: null,
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe("admin privileged-write recovery boundaries", () => {
  const en = translator("en");

  it("freezes an unknown Case create when no exact read target exists", async () => {
    const committed = caseRecord({
      case_id: "case-unknown-create",
      external_reference: "CASE-UNKNOWN-1",
      title: "Unknown create result",
      purpose: "Converge an ambiguous transport result",
    });
    const emptyPage = { items: [], next_cursor: null, has_more: false };
    const listCases = vi.fn().mockResolvedValue(emptyPage);
    const createCase = vi.fn().mockRejectedValue(new TypeError("network transport failed"));
    const client = { createCase, listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    await screen.findByText("No compliance cases", { exact: true });
    fireEvent.click(screen.getAllByRole("button", { name: "New case" })[0]!);
    fireEvent.change(screen.getByRole("textbox", { name: "External reference" }), {
      target: { value: committed.external_reference },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Title" }), {
      target: { value: committed.title },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), {
      target: { value: committed.purpose },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create case" }));

    expect(await screen.findByText(/only read-only reconciliation can confirm/i)).toBeVisible();
    expect(screen.getByRole("textbox", { name: "External reference" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Close dialog" })).toBeDisabled();
    expect(createCase).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Retry exact request" })).not.toBeInTheDocument();
    expect(screen.getByText(/did not return an exact resource ID/i)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Reconcile read-only state" }))
      .not.toBeInTheDocument();
    expect(listCases).toHaveBeenCalledTimes(1);
  });

  it("keeps an unknown Case close frozen until a later exact read confirms it", async () => {
    const active = caseRecord({ case_id: "case-unknown-close", title: "Unknown close result" });
    const closed = caseRecord({
      ...active,
      status: "closed",
      closed_by: "principal-closer",
      closed_at: "2026-08-24T02:00:00Z",
    });
    const listCases = vi.fn().mockResolvedValue({
      items: [active],
      next_cursor: null,
      has_more: false,
    });
    const closeCase = vi.fn().mockRejectedValue(new TypeError("connection reset"));
    const getCase = vi.fn()
      .mockResolvedValueOnce(active)
      .mockResolvedValueOnce(closed);
    const client = { closeCase, getCase, listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: active.title }));
    fireEvent.click(screen.getByRole("button", { name: "Close case" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm close" }));

    expect(await screen.findByText(/only read-only reconciliation can confirm/i)).toBeVisible();
    const unknownLock = screen.getByRole("status", {
      name: "Case or legal-hold write outcome is not confirmed",
    });
    expect(unknownLock).toHaveClass("unknown-write-lock");
    expect(unknownLock).toHaveAttribute("data-write-state", "unknown");
    expect(unknownLock).toHaveTextContent(active.case_id);
    expect(closeCase).toHaveBeenCalledTimes(1);
    expect(getCase).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Retry exact request" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reconcile read-only state" }));

    expect(await screen.findByRole("status", { name: "Case closed" }))
      .toHaveTextContent(active.case_id);
    expect(closeCase).toHaveBeenCalledTimes(1);
    expect(getCase).toHaveBeenCalledTimes(2);
    expect(getCase).toHaveBeenCalledWith(active.case_id);
  });

  it("keeps an unknown Hold release frozen until a later exact read confirms it", async () => {
    const active = holdRecord({ legal_hold_id: "hold-unknown-release" });
    const released = holdRecord({
      legal_hold_id: active.legal_hold_id,
      status: "released",
      release_approval_request_id: "approval-release-unknown",
      released_by: "principal-releaser",
      released_at: "2026-08-24T02:00:00Z",
    });
    const listHolds = vi.fn().mockResolvedValue({
      items: [active],
      next_cursor: null,
      has_more: false,
    });
    const releaseHold = vi.fn().mockRejectedValue(new TypeError("connection reset"));
    const getHold = vi.fn()
      .mockResolvedValueOnce(active)
      .mockResolvedValueOnce(released);
    const client = { getHold, listHolds, releaseHold } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: active.archive_id }));
    fireEvent.click(screen.getByRole("button", { name: "Release legal hold" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-release-unknown" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Confirm release" }));

    expect(await screen.findByText(/only read-only reconciliation can confirm/i)).toBeVisible();
    const unknownLock = screen.getByRole("status", {
      name: "Case or legal-hold write outcome is not confirmed",
    });
    expect(unknownLock).toHaveClass("unknown-write-lock");
    expect(unknownLock).toHaveAttribute("data-write-state", "unknown");
    expect(unknownLock).toHaveTextContent(active.legal_hold_id);
    expect(unknownLock).toHaveTextContent("approval-release-unknown");
    expect(releaseHold).toHaveBeenCalledTimes(1);
    expect(getHold).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Retry exact request" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reconcile read-only state" }));

    expect(await screen.findByRole("status", { name: "Legal hold released" }))
      .toHaveTextContent(active.legal_hold_id);
    expect(releaseHold).toHaveBeenCalledTimes(1);
    expect(getHold).toHaveBeenCalledTimes(2);
    expect(getHold).toHaveBeenCalledWith(active.legal_hold_id);
  });

  it("freezes an unknown Hold apply when no exact read target exists", async () => {
    const committed = holdRecord({
      legal_hold_id: "hold-unknown-apply",
      archive_id: "archive-unknown-apply",
      case_id: "case-unknown-apply",
      apply_approval_request_id: "approval-unknown-apply",
      reason: "Converge an ambiguous hold apply result",
    });
    const emptyPage = { items: [], next_cursor: null, has_more: false };
    const listHolds = vi.fn().mockResolvedValue(emptyPage);
    const applyHold = vi.fn().mockRejectedValue(new TypeError("network transport failed"));
    const client = { applyHold, listHolds } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );

    await screen.findByText("No legal holds", { exact: true });
    fireEvent.click(screen.getAllByRole("button", { name: "Apply legal hold" })[0]!);
    fireEvent.change(screen.getByRole("textbox", { name: "Audit archive ID" }), {
      target: { value: committed.archive_id },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Case ID" }), {
      target: { value: committed.case_id },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: committed.apply_approval_request_id },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Reason" }), {
      target: { value: committed.reason },
    });
    fireEvent.click(within(screen.getByRole("dialog", { name: "Apply legal hold" }))
      .getByRole("button", { name: "Apply legal hold" }));

    expect(await screen.findByText(/only read-only reconciliation can confirm/i)).toBeVisible();
    expect(screen.getByRole("textbox", { name: "Audit archive ID" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Close dialog" })).toBeDisabled();
    expect(applyHold).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Retry exact request" })).not.toBeInTheDocument();
    expect(screen.getByText(/did not return an exact resource ID/i)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Reconcile read-only state" }))
      .not.toBeInTheDocument();
    expect(listHolds).toHaveBeenCalledTimes(1);
  });

  it("contains lazy route failures and offers bounded retry and reload controls", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const reload = vi.fn();
    function SecretFailure(): never {
      throw new Error("secret chunk host and stack");
    }
    function Harness() {
      const [generation, setGeneration] = useState(0);
      const [broken, setBroken] = useState(true);
      return (
        <RecoverableRouteBoundary
          key={generation}
          reload={reload}
          retry={() => {
            setBroken(false);
            setGeneration((value) => value + 1);
          }}
          t={en}
        >
          {broken ? <SecretFailure /> : <h1>Recovered workspace</h1>}
        </RecoverableRouteBoundary>
      );
    }

    render(<Harness />);
    expect(screen.getByRole("alert")).toHaveTextContent("The admin workspace did not load");
    expect(screen.queryByText(/secret chunk host/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reload page" }));
    expect(reload).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(screen.getByRole("heading", { name: "Recovered workspace" })).toBeVisible();
    consoleError.mockRestore();
  });
});

function useNarrowAdminViewport() {
  vi.stubGlobal("matchMedia", vi.fn().mockImplementation((query: string) => ({
    matches: query === "(max-width: 1080px)",
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })));
}

describe("Dialog", () => {
  it("recovers focus when it unexpectedly moves outside the dialog", () => {
    const trigger = document.createElement("button");
    trigger.textContent = "Open case";
    const outside = document.createElement("button");
    outside.textContent = "Outside control";
    document.body.append(trigger, outside);
    trigger.focus();

    const { unmount } = render(
      <Dialog title="Create case" closeLabel="Close dialog" onClose={vi.fn()}>
        <input aria-label="Case title" />
        <button type="button">Create</button>
      </Dialog>,
    );

    const dialog = screen.getByRole("dialog", { name: "Create case" });
    const close = screen.getByRole("button", { name: "Close dialog" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog.parentElement?.parentElement).toBe(document.body);
    expect(close).toHaveFocus();

    outside.focus();

    expect(dialog.contains(document.activeElement)).toBe(true);
    expect(close).toHaveFocus();

    unmount();
    expect(trigger).toHaveFocus();
  });

  it("traps forward and reverse Tab movement at the dialog boundary", () => {
    render(
      <Dialog title="Create case" closeLabel="Close dialog" onClose={vi.fn()}>
        <input aria-label="Case title" />
        <button type="button">Create</button>
      </Dialog>,
    );

    const close = screen.getByRole("button", { name: "Close dialog" });
    const create = screen.getByRole("button", { name: "Create" });
    close.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(create).toHaveFocus();

    fireEvent.keyDown(document, { key: "Tab" });
    expect(close).toHaveFocus();
  });

  it("closes on Escape without allowing the keystroke to escape the dialog", () => {
    const onClose = vi.fn();
    render(
      <Dialog title="Create case" closeLabel="Close dialog" onClose={onClose}>
        <button type="button">Create</button>
      </Dialog>,
    );

    const escape = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(escape);

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(escape.defaultPrevented).toBe(true);
  });

  it("isolates the declared application background and restores its exact attributes", () => {
    const background = document.createElement("main");
    background.dataset.dialogBackground = "";
    background.setAttribute("aria-hidden", "false");
    background.setAttribute("inert", "previously-set");
    const trigger = document.createElement("button");
    trigger.textContent = "Open case";
    background.append(trigger);
    document.body.append(background);
    trigger.focus();

    const { unmount } = render(
      <Dialog title="Create case" closeLabel="Close dialog" onClose={vi.fn()}>
        <button type="button">Create</button>
      </Dialog>,
    );

    expect(background).toHaveAttribute("aria-hidden", "true");
    expect(background).toHaveAttribute("inert", "");

    unmount();

    expect(background).toHaveAttribute("aria-hidden", "false");
    expect(background).toHaveAttribute("inert", "previously-set");
    expect(trigger).toHaveFocus();
  });
});

function ConfirmDialogHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>Release evidence</button>
      {open ? (
        <ConfirmDialog
          cancelLabel="Keep hold"
          closeLabel="Close confirmation"
          confirmLabel="Release hold"
          description="Releasing this hold allows the evidence retention period to resume."
          title="Release legal hold"
          onCancel={() => setOpen(false)}
          onConfirm={vi.fn()}
        />
      ) : null}
    </>
  );
}

function RemovedOpenerConfirmHarness() {
  const [open, setOpen] = useState(false);
  const [showOpener, setShowOpener] = useState(true);
  const fallbackRef = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={fallbackRef} type="button">Stable fallback</button>
      {showOpener ? (
        <button type="button" onClick={() => setOpen(true)}>Remove record</button>
      ) : null}
      {open ? (
        <ConfirmDialog
          cancelLabel="Cancel"
          closeLabel="Close confirmation"
          confirmLabel="Confirm removal"
          description="The triggering row will be removed."
          returnFocusRef={fallbackRef}
          title="Remove record"
          onCancel={() => setOpen(false)}
          onConfirm={() => {
            setShowOpener(false);
            setOpen(false);
          }}
        />
      ) : null}
    </>
  );
}

describe("ConfirmDialog", () => {
  it("portals an explicitly described dangerous action and focuses cancel by default", () => {
    const { container } = render(<ConfirmDialogHarness />);
    const trigger = screen.getByRole("button", { name: "Release evidence" });

    trigger.focus();
    fireEvent.click(trigger);

    const dialog = screen.getByRole("dialog", { name: "Release legal hold" });
    const cancel = screen.getByRole("button", { name: "Keep hold" });
    const confirm = screen.getByRole("button", { name: "Release hold" });
    expect(container.querySelector("[role='dialog']")).toBeNull();
    expect(dialog).toHaveAccessibleDescription(
      "Releasing this hold allows the evidence retention period to resume.",
    );
    expect(cancel).toHaveFocus();
    expect(confirm).toHaveClass("button--danger");

    fireEvent.click(cancel);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    trigger.focus();
    fireEvent.click(trigger);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("locks every dismissal and submission path while a dangerous request is busy", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        busy
        cancelLabel="Keep hold"
        closeLabel="Close confirmation"
        confirmLabel="Release hold"
        description="Releasing this hold allows the evidence retention period to resume."
        title="Release legal hold"
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "Release legal hold" });
    const backdrop = dialog.parentElement;
    const form = dialog.querySelector("form");
    const close = screen.getByRole("button", { name: "Close confirmation" });
    const cancel = screen.getByRole("button", { name: "Keep hold" });
    const confirm = screen.getByRole("button", { name: "Release hold" });
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(close).toBeDisabled();
    expect(cancel).toBeDisabled();
    expect(confirm).toBeDisabled();

    fireEvent.click(close);
    fireEvent.click(cancel);
    fireEvent.click(confirm);
    fireEvent.keyDown(document, { key: "Escape" });
    if (backdrop) fireEvent.click(backdrop);
    if (form) fireEvent.submit(form);

    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
    expect(dialog).toBeInTheDocument();
  });

  it("uses the explicit fallback when a successful action removes its opener", async () => {
    render(<RemovedOpenerConfirmHarness />);
    fireEvent.click(screen.getByRole("button", { name: "Remove record" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove record" }))
      .not.toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Stable fallback" }))
        .toHaveFocus();
    });
  });
});

describe("admin accessibility and dashboard states", () => {
  const zh = translator("zh-CN");
  const en = translator("en");

  it("groups every localized route by operator intent and preserves its current-page semantics", () => {
    const navigate = vi.fn();
    render(<AdminNavigation view="overview" navigate={navigate} t={zh} />);

    const navigation = screen.getByRole("navigation", { name: "企业管理中心" });
    expect(within(navigation).getByRole("heading", { name: "指挥台" })).toBeVisible();
    expect(within(navigation).getByRole("heading", { name: "执行与审核" })).toBeVisible();
    expect(within(navigation).getByRole("heading", { name: "证据与留存" })).toBeVisible();
    expect(within(navigation).getByRole("heading", { name: "身份与自动化" })).toBeVisible();
    expect(within(navigation).getAllByRole("button")).toHaveLength(9);

    const audit = screen.getByRole("button", { name: "审计检索" });
    expect(audit).toHaveAttribute("aria-label", "审计检索");
    expect(audit).toHaveTextContent("审计检索");
    fireEvent.click(audit);
    expect(navigate).toHaveBeenCalledWith("audit");

    const inspector = screen.getByRole("button", { name: "Agent 检视" });
    expect(inspector).toHaveAttribute("aria-label", "Agent 检视");
    expect(inspector).toHaveTextContent("Agent 检视");
    fireEvent.click(inspector);
    expect(navigate).toHaveBeenCalledWith("agent-inspector");
    expect(screen.getByRole("button", { name: "概览" })).toHaveAttribute("aria-current", "page");

    expect(adminStyles).toContain(".admin-navigation { display: grid;");
    expect(adminStyles).toContain(".admin-navigation__group-label");
    expect(adminStyles).toContain("@media (min-width: 1024px) and (max-width: 1279px)");
    expect(adminStyles).toContain("@media (max-width: 1023px)");
    expect(adminStyles).toContain(".sidebar > .sidebar__footer { display: none; }");
    expect(adminStyles).not.toContain(".sidebar nav { display: grid;");
    expect(adminStyles).not.toContain(".admin-navigation button > span { display: none;");
  });

  it("keeps the current page visible and closes the narrow-screen drawer by keyboard or route selection", () => {
    const navigate = vi.fn();
    render(
      <div data-dialog-background>
        <AdaptiveNavigation navigate={navigate} t={en} view="mail-domains" />
      </div>,
    );

    const trigger = screen.getByRole("button", { name: "Open navigation" });
    expect(screen.getByText("Mail domains")).toBeVisible();
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    trigger.focus();
    fireEvent.click(trigger);
    const drawer = screen.getByRole("dialog", { name: "Admin navigation" });
    const close = within(drawer).getByRole("button", { name: "Close navigation" });
    expect(drawer).toHaveAttribute("aria-modal", "true");
    expect(within(drawer).getByText("Mail domains", { selector: "strong" })).toBeVisible();
    expect(within(drawer).getByRole("button", { name: "Mail domains" }))
      .toHaveAttribute("aria-current", "page");
    expect(close).toHaveFocus();
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Admin navigation" })).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(trigger);
    fireEvent.click(within(screen.getByRole("dialog", { name: "Admin navigation" }))
      .getByRole("button", { name: "Audit search" }));
    expect(navigate).toHaveBeenCalledWith("audit");
    expect(screen.queryByRole("dialog", { name: "Admin navigation" })).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expect(adminStyles).toContain(".adaptive-nav-backdrop { padding: 0; place-items: stretch start; }");
    expect(adminStyles).toContain(".dialog.adaptive-nav-drawer");
  });

  it("keeps tenant, language, and sign-out controls reachable from the compact drawer", () => {
    const onLocaleChange = vi.fn();
    const onSignOut = vi.fn();
    render(
      <AdaptiveNavigation
        navigate={vi.fn()}
        sessionControls={(
          <AdminSessionControls
            locale="en"
            organizationId="019b0000-0000-7000-8000-000000000001"
            onLocaleChange={onLocaleChange}
            onSignOut={onSignOut}
            t={en}
          />
        )}
        t={en}
        view="overview"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    const drawer = screen.getByRole("dialog", { name: "Admin navigation" });

    expect(within(drawer).getByText("Organization")).toBeVisible();
    const language = within(drawer).getByRole("combobox", { name: "Language" });
    expect(language).toBeVisible();
    fireEvent.change(language, { target: { value: "zh-CN" } });
    expect(onLocaleChange).toHaveBeenCalledWith("zh-CN");

    const signOut = within(drawer).getByRole("button", { name: "Sign out" });
    expect(signOut).toBeVisible();
    fireEvent.click(signOut);
    expect(onSignOut).toHaveBeenCalledTimes(1);
    expect(signOut.closest(".adaptive-nav-drawer__session")).toBeInTheDocument();
    expect(adminStyles).toContain(".adaptive-nav-drawer__session .sidebar__footer");
    expect(adminStyles).toContain(".sidebar > .sidebar__footer { display: none; }");
  });

  it("localizes the pagination landmark", () => {
    render(<Pagination canGoBack={false} canGoNext onBack={vi.fn()} onNext={vi.fn()} t={zh} />);

    expect(screen.getByRole("navigation", { name: "分页导航" })).toBeInTheDocument();
  });

  it("keeps a selected case in factual context, a bounded close decision, and closed evidence", async () => {
    const item = caseRecord({
      case_id: "case-factual-context",
      external_reference: "SEC-FACT-1",
      title: "Factual case context",
      purpose: "Preserve the verified review record.",
      created_by: "principal-case-creator",
    });
    const openHolds = vi.fn();
    const client = {
      listCases: vi.fn().mockResolvedValue({ items: [item], next_cursor: null, has_more: false }),
    } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={openHolds}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: item.title }));
    const detail = screen.getByRole("heading", { name: item.title })
      .closest(".case-hold-detail") as HTMLElement;
    const context = within(detail).getByRole("region", { name: "Case context" });
    expect(context).toHaveTextContent(item.case_id);
    expect(context).toHaveTextContent(item.external_reference);
    expect(context).toHaveTextContent("Open");
    fireEvent.click(within(context).getByRole("button", { name: "View holds for this case" }));
    expect(openHolds).toHaveBeenCalledWith(item.case_id);

    expect(within(detail).getByRole("heading", { name: "Case facts" })).toBeVisible();
    expect(within(detail).getByText(item.purpose)).toBeVisible();
    expect(within(detail).getByRole("heading", { name: "Current case decision" })).toBeVisible();
    const evidence = within(detail).getByText("Case evidence", { exact: true }).closest("details")!;
    expect(evidence).not.toHaveAttribute("open");
    expect(evidence).toHaveTextContent(item.case_id);
    expect(evidence).toHaveTextContent(item.created_by);

    fireEvent.click(within(detail).getByRole("button", { name: "Close case" }));
    const dialog = screen.getByRole("dialog", { name: "Close case" });
    expect(within(dialog).getByRole("heading", { name: "Review before confirming" })).toBeVisible();
    expect(dialog).toHaveTextContent(item.case_id);
    expect(dialog).toHaveTextContent(item.external_reference);
    expect(dialog).toHaveTextContent("After confirmation, this case will become closed.");
    expect(dialog).toHaveTextContent("A case can close only after all of its legal holds are released.");
  });

  it("keeps hold approval evidence closed and makes release confirmation resource-bound", async () => {
    const item = holdRecord({
      legal_hold_id: "hold-fact-1",
      archive_id: "archive-fact-1",
      case_id: "case-fact-1",
      apply_approval_request_id: "approval-applied-context",
      created_by: "principal-hold-creator",
      reason: "Preserve the evidence while the factual case is reviewed.",
    });
    const client = {
      listHolds: vi.fn().mockResolvedValue({ items: [item], next_cursor: null, has_more: false }),
    } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: item.archive_id }));
    const detail = screen.getByRole("heading", { name: "hold-fact-1" })
      .closest(".case-hold-detail") as HTMLElement;
    const context = within(detail).getByRole("region", { name: "Legal hold context" });
    expect(context).toHaveTextContent(item.legal_hold_id);
    expect(context).toHaveTextContent(item.archive_id);
    expect(context).toHaveTextContent(item.case_id);
    expect(within(detail).getByRole("heading", { name: "Legal hold facts" })).toBeVisible();
    expect(within(detail).getByText(item.reason)).toBeVisible();
    const evidence = within(detail).getByText("Legal-hold evidence", { exact: true }).closest("details")!;
    expect(evidence).not.toHaveAttribute("open");
    expect(evidence).toHaveTextContent(item.apply_approval_request_id);
    expect(evidence).toHaveTextContent(item.created_by);

    fireEvent.click(within(detail).getByRole("button", { name: "Release legal hold" }));
    const dialog = screen.getByRole("dialog", { name: "Release legal hold" });
    expect(within(dialog).getByRole("heading", { name: "Review before confirming" })).toBeVisible();
    expect(dialog).toHaveTextContent(item.legal_hold_id);
    expect(dialog).toHaveTextContent(item.archive_id);
    expect(dialog).toHaveTextContent(item.case_id);
    expect(dialog).toHaveTextContent(item.apply_approval_request_id);
    expect(dialog).toHaveTextContent("After the server verifies the approval request you enter, this legal hold will be released.");
    expect(within(dialog).getByRole("textbox", { name: "Approval request ID" })).toBeVisible();
  });

  it("opens a Hold's linked Case only through the exact Case ID carried by the Hold", async () => {
    const item = holdRecord({
      legal_hold_id: "hold-link-1",
      case_id: "case-linked-from-hold-1",
    });
    const openCase = vi.fn();
    const client = {
      listHolds: vi.fn().mockResolvedValue({ items: [item], next_cursor: null, has_more: false }),
    } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        openCase={openCase}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: item.archive_id }));
    const detail = screen.getByRole("heading", { name: item.legal_hold_id })
      .closest(".case-hold-detail") as HTMLElement;
    fireEvent.click(within(detail).getByRole("button", { name: "View linked case" }));

    expect(openCase).toHaveBeenCalledTimes(1);
    expect(openCase).toHaveBeenCalledWith(item.case_id);
  });

  it("renders a Hold-linked Case only after its exact detail read succeeds", async () => {
    const linked = caseRecord({
      case_id: "case-linked-exact-read-1",
      status: "closed",
      title: "Case confirmed by exact read",
      closed_by: "principal-closer",
      closed_at: "2026-08-24T02:00:00Z",
    });
    const listCases = vi.fn().mockResolvedValue({ items: [], next_cursor: null, has_more: false });
    const getCase = vi.fn().mockResolvedValue(linked);
    const client = { getCase, listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        clearInitialCase={vi.fn()}
        client={client}
        initialCaseId={linked.case_id}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    const detailTitle = await screen.findByRole("heading", { name: linked.title });
    const detail = detailTitle.closest(".case-hold-detail") as HTMLElement;
    expect(within(detail).getByRole("region", { name: "Case context" }))
      .toHaveTextContent(linked.case_id);
    expect(getCase).toHaveBeenCalledTimes(1);
    expect(getCase).toHaveBeenCalledWith(linked.case_id);
    expect(listCases).toHaveBeenCalledTimes(1);
  });

  it("keeps 403 and 503 overview facts isolated and retries only the retryable card", async () => {
    const listCases = vi.fn().mockRejectedValue(new ApiError(403, {
      code: "forbidden",
      message: "sensitive server authorization detail",
      request_id: "cases-request-403",
      trace_id: "cases-trace-secret",
      retryable: false,
    }));
    const listHolds = vi.fn()
      .mockRejectedValueOnce(unavailableError("holds"))
      .mockResolvedValueOnce({ items: [{}, {}], next_cursor: null, has_more: true });
    const client = { listCases, listHolds } as unknown as ControlPlaneClient;

    render(<Overview client={client} navigate={vi.fn()} t={en} />);

    expect(screen.getAllByRole("status")).toHaveLength(2);
    const casesCard = screen.getByRole("region", { name: "Open cases" });
    const holdsCard = screen.getByRole("region", { name: "Active legal holds" });
    expect(await within(casesCard).findByRole("alert")).toHaveTextContent("forbidden");
    expect(casesCard).toHaveAttribute("data-overview-state", "denied");
    expect(within(casesCard).getByText("Denied")).toBeVisible();
    expect(within(casesCard).queryByRole("button", { name: "Retry" }))
      .not.toBeInTheDocument();

    expect(await within(holdsCard).findByRole("alert"))
      .toHaveTextContent("holds_unavailable");
    fireEvent.click(within(holdsCard).getByRole("button", { name: "Retry" }));

    await waitFor(() => {
      expect(holdsCard).toHaveAttribute("data-overview-state", "ready");
    });
    expect(within(holdsCard).getByText("2+")).toBeVisible();
    expect(listCases).toHaveBeenCalledTimes(1);
    expect(listHolds).toHaveBeenCalledTimes(2);
  });

  it("shows independent verified empty and bounded overview counts without a static permission claim", async () => {
    const listCases = vi.fn().mockResolvedValue({
      items: [],
      next_cursor: null,
      has_more: false,
    });
    const listHolds = vi.fn().mockResolvedValue({
      items: [{}, {}, {}],
      next_cursor: "holds-next",
      has_more: true,
    });
    const client = { listCases, listHolds } as unknown as ControlPlaneClient;

    const navigate = vi.fn();
    render(<Overview client={client} navigate={navigate} t={en} />);

    const casesCard = screen.getByRole("region", { name: "Open cases" });
    const holdsCard = screen.getByRole("region", { name: "Active legal holds" });
    expect(await within(casesCard).findByText("0")).toBeVisible();
    expect(within(casesCard).getByText("There are no open cases.")).toBeVisible();
    expect(await within(holdsCard).findByText("3+")).toBeVisible();
    const holdsAction = screen.getByRole("button", { name: "View active legal holds" });
    fireEvent.click(holdsAction);
    expect(navigate).toHaveBeenCalledWith("holds");
    expect(listCases).toHaveBeenCalledTimes(1);
    expect(listHolds).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/legal_hold\.manage/)).not.toBeInTheDocument();
  });

  it("prioritizes readable open cases without creating another protected read", async () => {
    const listCases = vi.fn().mockResolvedValue({
      items: [{}, {}],
      next_cursor: null,
      has_more: false,
    });
    const listHolds = vi.fn().mockResolvedValue({
      items: [{}],
      next_cursor: null,
      has_more: false,
    });
    const navigate = vi.fn();
    const client = { listCases, listHolds } as unknown as ControlPlaneClient;

    render(<Overview client={client} navigate={navigate} t={en} />);

    expect(await screen.findByRole("heading", {
      name: "Open cases are present in the current readable scope",
    })).toBeVisible();
    expect(screen.getByRole("button", { name: "View open cases" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "View active legal holds" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "View open cases" }));
    expect(navigate).toHaveBeenCalledWith("cases");
    expect(listCases).toHaveBeenCalledTimes(1);
    expect(listHolds).toHaveBeenCalledTimes(1);
  });

  it("does not manufacture a next action when either overview fact is unknown", async () => {
    const listCases = vi.fn().mockRejectedValue(new ApiError(403, {
      code: "forbidden",
      message: "sensitive server authorization detail",
      request_id: "cases-request-403",
      trace_id: "cases-trace-secret",
      retryable: false,
    }));
    const listHolds = vi.fn().mockResolvedValue({
      items: [{}],
      next_cursor: null,
      has_more: false,
    });
    const client = { listCases, listHolds } as unknown as ControlPlaneClient;

    render(<Overview client={client} navigate={vi.fn()} t={en} />);

    expect(await screen.findByRole("heading", {
      name: "A verified next action is not available yet",
    })).toBeVisible();
    expect(screen.queryByRole("button", { name: "View open cases" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "View active legal holds" })).not.toBeInTheDocument();
    expect(listCases).toHaveBeenCalledTimes(1);
    expect(listHolds).toHaveBeenCalledTimes(1);
  });

  it("shows the verified empty decision only after both protected reads are empty", async () => {
    const listCases = vi.fn().mockResolvedValue({ items: [], next_cursor: null, has_more: false });
    const listHolds = vi.fn().mockResolvedValue({ items: [], next_cursor: null, has_more: false });
    const client = { listCases, listHolds } as unknown as ControlPlaneClient;

    render(<Overview client={client} navigate={vi.fn()} t={en} />);

    expect(await screen.findByRole("heading", {
      name: "No pending item is present in the current readable scope",
    })).toBeVisible();
    expect(screen.queryByRole("button", { name: /View (open cases|active legal holds)/ })).not.toBeInTheDocument();
  });

  it("does not turn an initial cases 503 into an empty result", async () => {
    const listCases = vi.fn()
      .mockRejectedValueOnce(unavailableError("cases"))
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const client = { listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("cases_unavailable");
    expect(screen.queryByText("No compliance cases", { exact: true })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("No compliance cases", { exact: true })).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(listCases).toHaveBeenCalledTimes(2);
  });

  it("does not turn an initial holds 503 into an empty result", async () => {
    const listHolds = vi.fn()
      .mockRejectedValueOnce(unavailableError("holds"))
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const client = { listHolds } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("holds_unavailable");
    expect(screen.queryByText("No legal holds", { exact: true })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("No legal holds", { exact: true })).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(listHolds).toHaveBeenCalledTimes(2);
  });

  it("locks every case-create dismissal path and keeps exact success evidence outside a stale empty query", async () => {
    const mutation = deferred<ComplianceCase>();
    const listCases = vi.fn()
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const createCase = vi.fn().mockReturnValue(mutation.promise);
    const notify = vi.fn();
    const client = { createCase, listCases } as unknown as ControlPlaneClient;
    const committed = caseRecord({
      case_id: "case-create-committed",
      external_reference: "CASE-CREATE-1",
      title: "Created from exact response",
      purpose: "Preserve the exact POST success fact",
    });

    render(
      <CasesView
        client={client}
        locale="en"
        notify={notify}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    await screen.findByText("No compliance cases", { exact: true });
    expect(screen.getAllByRole("button", { name: "New case" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "New case" }));
    const dialog = screen.getByRole("dialog", { name: "New case" });
    expect(within(dialog).getByRole("textbox", { name: "External reference" }))
      .toHaveFocus();
    fireEvent.change(within(dialog).getByRole("textbox", { name: "External reference" }), {
      target: { value: committed.external_reference },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Title" }), {
      target: { value: committed.title },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Purpose" }), {
      target: { value: committed.purpose },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create case" }));

    expect(createCase).toHaveBeenCalledWith({
      external_reference: committed.external_reference,
      title: committed.title,
      purpose: committed.purpose,
    });
    await waitFor(() => expect(dialog).toHaveAttribute("aria-busy", "true"));
    const close = within(dialog).getByRole("button", { name: "Close dialog" });
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    expect(close).toBeDisabled();
    expect(cancel).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Create case" })).toBeDisabled();
    expect(within(dialog).getByRole("textbox", { name: "Title" })).toBeDisabled();

    fireEvent.click(close);
    fireEvent.click(cancel);
    fireEvent.keyDown(document, { key: "Escape" });
    if (dialog.parentElement) fireEvent.click(dialog.parentElement);
    expect(screen.getByRole("dialog", { name: "New case" })).toBeInTheDocument();
    expect(createCase).toHaveBeenCalledTimes(1);

    await act(async () => {
      mutation.resolve(committed);
      await mutation.promise;
    });

    const evidence = await screen.findByRole("status", { name: "Case created" });
    expect(evidence).toHaveTextContent(committed.title);
    expect(evidence).toHaveTextContent(committed.case_id);
    await waitFor(() => expect(listCases).toHaveBeenCalledTimes(2));
    expect(screen.getByText("No compliance cases", { exact: true })).toBeVisible();
    expect(screen.queryByRole("button", { name: committed.title })).not.toBeInTheDocument();
    expect(notify).toHaveBeenCalledWith("Case created");
  });

  it("keeps committed case-create evidence when its follow-up read returns 503", async () => {
    const committed = caseRecord({
      case_id: "case-create-read-error",
      external_reference: "CASE-READ-503",
      title: "Created before read failure",
      purpose: "Separate write success from read failure",
    });
    const listCases = vi.fn()
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false })
      .mockRejectedValueOnce(unavailableError("cases_read"));
    const createCase = vi.fn().mockResolvedValue(committed);
    const client = { createCase, listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    await screen.findByText("No compliance cases", { exact: true });
    fireEvent.click(screen.getAllByRole("button", { name: "New case" })[0]);
    const dialog = screen.getByRole("dialog", { name: "New case" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "External reference" }), {
      target: { value: committed.external_reference },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Title" }), {
      target: { value: committed.title },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Purpose" }), {
      target: { value: committed.purpose },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create case" }));

    const evidence = await screen.findByRole("status", { name: "Case created" });
    expect(evidence).toHaveTextContent(committed.case_id);
    expect(await screen.findByRole("alert")).toHaveTextContent("cases_read_unavailable");
    expect(screen.getByRole("status", { name: "Case created" }))
      .toHaveTextContent(committed.title);
    expect(screen.queryByText("Operation did not complete")).not.toBeInTheDocument();
  });

  it("locks every hold-apply dismissal path and keeps exact success evidence outside a stale empty query", async () => {
    const mutation = deferred<LegalHold>();
    const listHolds = vi.fn()
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const applyHold = vi.fn().mockReturnValue(mutation.promise);
    const notify = vi.fn();
    const client = { applyHold, listHolds } as unknown as ControlPlaneClient;
    const committed = holdRecord({
      legal_hold_id: "hold-apply-committed",
      archive_id: "archive-apply-committed",
      case_id: "case-apply-committed",
      apply_approval_request_id: "approval-apply-committed",
      reason: "Preserve exact apply response",
    });

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={notify}
        t={en}
      />,
    );

    await screen.findByText("No legal holds", { exact: true });
    expect(screen.getAllByRole("button", { name: "Apply legal hold" }))
      .toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Apply legal hold" }));
    const dialog = screen.getByRole("dialog", { name: "Apply legal hold" });
    expect(within(dialog).getByRole("textbox", { name: "Audit archive ID" }))
      .toHaveFocus();
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Audit archive ID" }), {
      target: { value: committed.archive_id },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Case ID" }), {
      target: { value: committed.case_id },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: committed.apply_approval_request_id },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Reason" }), {
      target: { value: committed.reason },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply legal hold" }));

    expect(applyHold).toHaveBeenCalledWith(committed.archive_id, {
      case_id: committed.case_id,
      approval_request_id: committed.apply_approval_request_id,
      reason: committed.reason,
    });
    await waitFor(() => expect(dialog).toHaveAttribute("aria-busy", "true"));
    const close = within(dialog).getByRole("button", { name: "Close dialog" });
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    expect(close).toBeDisabled();
    expect(cancel).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Apply legal hold" }))
      .toBeDisabled();

    fireEvent.click(close);
    fireEvent.click(cancel);
    fireEvent.keyDown(document, { key: "Escape" });
    if (dialog.parentElement) fireEvent.click(dialog.parentElement);
    expect(screen.getByRole("dialog", { name: "Apply legal hold" }))
      .toBeInTheDocument();
    expect(applyHold).toHaveBeenCalledTimes(1);

    await act(async () => {
      mutation.resolve(committed);
      await mutation.promise;
    });

    const evidence = await screen.findByRole("status", {
      name: "Legal hold applied and verified by remote read-back",
    });
    expect(evidence).toHaveTextContent(committed.archive_id);
    expect(evidence).toHaveTextContent(committed.legal_hold_id);
    await waitFor(() => expect(listHolds).toHaveBeenCalledTimes(2));
    expect(screen.getByText("No legal holds", { exact: true })).toBeVisible();
    expect(screen.queryByRole("button", { name: committed.archive_id }))
      .not.toBeInTheDocument();
    expect(notify).toHaveBeenCalledWith(
      "Legal hold applied and verified by remote read-back",
    );
  });

  it("keeps committed hold-apply evidence when its follow-up read returns 503", async () => {
    const committed = holdRecord({
      legal_hold_id: "hold-apply-read-error",
      archive_id: "archive-apply-read-error",
      case_id: "case-apply-read-error",
      apply_approval_request_id: "approval-apply-read-error",
      reason: "Separate apply success from read failure",
    });
    const listHolds = vi.fn()
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false })
      .mockRejectedValueOnce(unavailableError("holds_read"));
    const applyHold = vi.fn().mockResolvedValue(committed);
    const client = { applyHold, listHolds } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );

    await screen.findByText("No legal holds", { exact: true });
    fireEvent.click(screen.getAllByRole("button", { name: "Apply legal hold" })[0]);
    const dialog = screen.getByRole("dialog", { name: "Apply legal hold" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Audit archive ID" }), {
      target: { value: committed.archive_id },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Case ID" }), {
      target: { value: committed.case_id },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: committed.apply_approval_request_id },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Reason" }), {
      target: { value: committed.reason },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply legal hold" }));

    const evidence = await screen.findByRole("status", {
      name: "Legal hold applied and verified by remote read-back",
    });
    expect(evidence).toHaveTextContent(committed.legal_hold_id);
    expect(await screen.findByRole("alert")).toHaveTextContent("holds_read_unavailable");
    expect(screen.getByRole("status", {
      name: "Legal hold applied and verified by remote read-back",
    })).toHaveTextContent(committed.archive_id);
    expect(screen.queryByText("Operation did not complete")).not.toBeInTheDocument();
  });

  it("uses the shared narrow list-detail contract for cases and clears the controlled selection on Back", async () => {
    useNarrowAdminViewport();
    const item: ComplianceCase = {
      case_id: "case-narrow-1",
      organization_id: "organization-1",
      external_reference: "CASE-NARROW-1",
      title: "Narrow case",
      purpose: "Confirmed narrow-layout purpose",
      status: "open",
      created_by: "principal-1",
      created_at: "2026-08-24T01:00:00Z",
      closed_by: null,
      closed_at: null,
    };
    const client = {
      listCases: vi.fn().mockResolvedValue({ items: [item], next_cursor: null, has_more: false }),
    } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    const trigger = await screen.findByRole("button", { name: "Narrow case" });
    const layout = trigger.closest(".admin-list-detail");
    expect(layout).toHaveAttribute("data-detail-open", "false");
    expect(trigger).not.toHaveAttribute("aria-current");

    fireEvent.click(trigger);
    expect(layout).toHaveAttribute("data-detail-open", "true");
    expect(trigger).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("heading", { name: "Narrow case" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: en("backToCases") }));
    expect(layout).toHaveAttribute("data-detail-open", "false");
    expect(trigger).not.toHaveAttribute("aria-current");
    expect(screen.queryByRole("heading", { name: "Narrow case" })).not.toBeInTheDocument();
  });

  it("uses each legal-hold identity in the shared narrow list-detail selection", async () => {
    useNarrowAdminViewport();
    const hold: LegalHold = {
      legal_hold_id: "hold-narrow-1",
      organization_id: "organization-1",
      case_id: "case-1",
      archive_id: "archive-narrow",
      reason: "Preserve narrow-layout evidence",
      status: "active",
      apply_approval_request_id: "approval-1",
      created_by: "principal-1",
      created_at: "2026-08-24T01:00:00Z",
      release_approval_request_id: null,
      released_by: null,
      released_at: null,
    };
    const client = {
      listHolds: vi.fn().mockResolvedValue({ items: [hold], next_cursor: null, has_more: false }),
    } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );

    const trigger = await screen.findByRole("button", { name: "archive-narrow" });
    const layout = trigger.closest(".admin-list-detail");
    expect(layout).toHaveAttribute("data-detail-open", "false");
    expect(trigger).not.toHaveAttribute("aria-current");

    fireEvent.click(trigger);
    expect(layout).toHaveAttribute("data-detail-open", "true");
    expect(trigger).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("heading", { name: "hold-narrow-1" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: en("backToHolds") }));
    expect(layout).toHaveAttribute("data-detail-open", "false");
    expect(trigger).not.toHaveAttribute("aria-current");
    expect(screen.queryByRole("heading", { name: "hold-narrow-1" })).not.toBeInTheDocument();
  });

  it("rebinds case details to refreshed data and clears a removed selection", async () => {
    const original: ComplianceCase = {
      case_id: "case-refresh-1",
      organization_id: "organization-1",
      external_reference: "CASE-1",
      title: "Original case",
      purpose: "Original confirmed purpose",
      status: "open",
      created_by: "principal-1",
      created_at: "2026-08-24T01:00:00Z",
      closed_by: null,
      closed_at: null,
    };
    const refreshed: ComplianceCase = {
      ...original,
      title: "Refreshed case",
      purpose: "Refreshed confirmed purpose",
    };
    const listCases = vi.fn()
      .mockResolvedValueOnce({ items: [original], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [refreshed], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const client = { listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Original case" }));
    expect(screen.getByText("Original confirmed purpose")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Refreshed confirmed purpose")).toBeVisible();
    expect(screen.queryByText("Original confirmed purpose")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("No compliance cases")).toBeVisible();
    expect(screen.queryByText("Refreshed confirmed purpose")).not.toBeInTheDocument();
  });

  it("clears legal-hold details when refresh removes the selected record", async () => {
    const hold: LegalHold = {
      legal_hold_id: "hold-refresh-1",
      organization_id: "organization-1",
      case_id: "case-1",
      archive_id: "archive-visible",
      reason: "Preserve confirmed evidence",
      status: "active",
      apply_approval_request_id: "approval-1",
      created_by: "principal-1",
      created_at: "2026-08-24T01:00:00Z",
      release_approval_request_id: null,
      released_by: null,
      released_at: null,
    };
    const listHolds = vi.fn()
      .mockResolvedValueOnce({ items: [hold], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const client = { listHolds } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "archive-visible" }));
    expect(screen.getAllByText("Preserve confirmed evidence")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("No legal holds")).toBeVisible();
    expect(screen.queryByText("Preserve confirmed evidence")).not.toBeInTheDocument();
  });

  it("keeps an exact case target mounted when a list refresh finishes during a busy close", async () => {
    const item = caseRecord({
      case_id: "case-close-busy",
      title: "Case targeted before refresh",
    });
    const closed = caseRecord({
      ...item,
      status: "closed",
      closed_by: "principal-closer",
      closed_at: "2026-08-24T02:00:00Z",
    });
    const refresh = deferred<{ items: ComplianceCase[]; next_cursor: null; has_more: false }>();
    const mutation = deferred<ComplianceCase>();
    const listCases = vi.fn()
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockReturnValueOnce(refresh.promise)
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const closeCase = vi.fn().mockReturnValue(mutation.promise);
    const notify = vi.fn();
    const client = { closeCase, listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={notify}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: item.title }));
    fireEvent.click(screen.getByRole("button", { name: "Close case" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm close" }));

    expect(closeCase).toHaveBeenCalledWith(item.case_id);
    await waitFor(() => expect(
      screen.getByRole("dialog", { name: "Close case" }),
    ).toHaveAttribute("aria-busy", "true"));

    await act(async () => {
      refresh.resolve({ items: [], next_cursor: null, has_more: false });
      await refresh.promise;
    });

    expect(screen.getByRole("dialog", { name: "Close case" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Confirm close" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();

    await act(async () => {
      mutation.resolve(closed);
      await mutation.promise;
    });

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Close case" }))
      .not.toBeInTheDocument());
    expect(notify).toHaveBeenCalledWith("Case closed");
  });

  it("keeps an exact hold target mounted when a list refresh finishes during a busy release", async () => {
    const item = holdRecord({
      legal_hold_id: "hold-release-busy",
      archive_id: "archive-busy",
    });
    const released = holdRecord({
      ...item,
      status: "released",
      release_approval_request_id: "approval-release-1",
      released_by: "principal-releaser",
      released_at: "2026-08-24T02:00:00Z",
    });
    const refresh = deferred<{ items: LegalHold[]; next_cursor: null; has_more: false }>();
    const mutation = deferred<LegalHold>();
    const listHolds = vi.fn()
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockReturnValueOnce(refresh.promise)
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const releaseHold = vi.fn().mockReturnValue(mutation.promise);
    const notify = vi.fn();
    const client = { listHolds, releaseHold } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={notify}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: item.archive_id }));
    fireEvent.click(screen.getByRole("button", { name: "Release legal hold" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-release-1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm release" }));

    expect(releaseHold).toHaveBeenCalledWith(
      item.legal_hold_id,
      "approval-release-1",
    );
    await waitFor(() => expect(
      screen.getByRole("dialog", { name: "Release legal hold" }),
    ).toHaveAttribute("aria-busy", "true"));

    await act(async () => {
      refresh.resolve({ items: [], next_cursor: null, has_more: false });
      await refresh.promise;
    });

    expect(screen.getByRole("dialog", { name: "Release legal hold" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Confirm release" })).toBeDisabled();

    await act(async () => {
      mutation.resolve(released);
      await mutation.promise;
    });

    await waitFor(() => expect(screen.queryByRole("dialog", {
      name: "Release legal hold",
    })).not.toBeInTheDocument());
    expect(notify).toHaveBeenCalledWith("Legal hold released");
  });

  it("restores an absent committed case only in the query that produced it", async () => {
    const item = caseRecord({
      case_id: "case-close-absent",
      title: "Case absent from refreshed page",
    });
    const closed = caseRecord({
      ...item,
      status: "closed",
      closed_by: "principal-closer",
      closed_at: "2026-08-24T02:00:00Z",
    });
    const busyRefresh = deferred<{
      items: ComplianceCase[];
      next_cursor: null;
      has_more: false;
    }>();
    const mutation = deferred<ComplianceCase>();
    const listCases = vi.fn()
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockReturnValueOnce(busyRefresh.promise)
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const closeCase = vi.fn().mockReturnValue(mutation.promise);
    const client = { closeCase, listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={vi.fn()}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    await screen.findByRole("button", { name: item.title });
    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), {
      target: { value: "" },
    });
    fireEvent.click(await screen.findByRole("button", { name: item.title }));
    fireEvent.click(screen.getByRole("button", { name: "Close case" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm close" }));

    await act(async () => {
      busyRefresh.resolve({ items: [], next_cursor: null, has_more: false });
      await busyRefresh.promise;
    });
    expect(screen.getByRole("dialog", { name: "Close case" })).toBeInTheDocument();

    await act(async () => {
      mutation.resolve(closed);
      await mutation.promise;
    });

    const evidence = await screen.findByRole("status", { name: "Case closed" });
    await waitFor(() => expect(listCases).toHaveBeenCalledTimes(4));
    expect(evidence).toHaveTextContent(item.title);
    expect(evidence).toHaveTextContent(item.case_id);
    expect(screen.getByRole("button", { name: item.title })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Close case" })).not.toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), {
      target: { value: "open" },
    });

    expect(await screen.findByText("No compliance cases")).toBeVisible();
    expect(screen.queryByRole("button", { name: item.title })).not.toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Case closed" }))
      .toHaveTextContent(item.case_id);
  });

  it("restores an absent committed hold only on its exact cursor page", async () => {
    const item = holdRecord({
      legal_hold_id: "hold-release-absent",
      archive_id: "archive-absent",
    });
    const released = holdRecord({
      ...item,
      status: "released",
      release_approval_request_id: "approval-release-absent",
      released_by: "principal-releaser",
      released_at: "2026-08-24T02:00:00Z",
    });
    const busyRefresh = deferred<{
      items: LegalHold[];
      next_cursor: null;
      has_more: false;
    }>();
    const mutation = deferred<LegalHold>();
    const listHolds = vi.fn()
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockReturnValueOnce(busyRefresh.promise)
      .mockResolvedValueOnce({ items: [], next_cursor: "hold-cursor-2", has_more: true })
      .mockResolvedValueOnce({ items: [], next_cursor: null, has_more: false });
    const releaseHold = vi.fn().mockReturnValue(mutation.promise);
    const client = { listHolds, releaseHold } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={vi.fn()}
        t={en}
      />,
    );

    await screen.findByRole("button", { name: item.archive_id });
    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), {
      target: { value: "" },
    });
    fireEvent.click(await screen.findByRole("button", { name: item.archive_id }));
    fireEvent.click(screen.getByRole("button", { name: "Release legal hold" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-release-absent" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm release" }));

    await act(async () => {
      busyRefresh.resolve({ items: [], next_cursor: null, has_more: false });
      await busyRefresh.promise;
    });
    expect(screen.getByRole("dialog", { name: "Release legal hold" }))
      .toBeInTheDocument();

    await act(async () => {
      mutation.resolve(released);
      await mutation.promise;
    });

    const evidence = await screen.findByRole("status", { name: "Legal hold released" });
    await waitFor(() => expect(listHolds).toHaveBeenCalledTimes(4));
    expect(evidence).toHaveTextContent(item.archive_id);
    expect(evidence).toHaveTextContent(item.legal_hold_id);
    expect(screen.getByRole("button", { name: item.archive_id })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Release legal hold" }))
      .not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(await screen.findByText("No legal holds")).toBeVisible();
    expect(screen.queryByRole("button", { name: item.archive_id })).not.toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Legal hold released" }))
      .toHaveTextContent(item.legal_hold_id);
  });

  it("does not let a stale post-close list response reopen a committed case", async () => {
    const item = caseRecord({
      case_id: "case-close-stale",
      title: "Case closed before stale read",
    });
    const closed = caseRecord({
      ...item,
      status: "closed",
      closed_by: "principal-closer",
      closed_at: "2026-08-24T02:00:00Z",
    });
    const staleRead = deferred<{ items: ComplianceCase[]; next_cursor: null; has_more: false }>();
    const listCases = vi.fn()
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockReturnValueOnce(staleRead.promise);
    const closeCase = vi.fn().mockResolvedValue(closed);
    const notify = vi.fn();
    const client = { closeCase, listCases } as unknown as ControlPlaneClient;

    render(
      <CasesView
        client={client}
        locale="en"
        notify={notify}
        openHolds={vi.fn()}
        t={en}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: item.title }));
    fireEvent.click(screen.getByRole("button", { name: "Close case" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm close" }));

    expect(await screen.findByText("No compliance cases")).toBeVisible();
    expect(notify).toHaveBeenCalledWith("Case closed");
    expect(screen.queryByRole("button", { name: "Close case" }))
      .not.toBeInTheDocument();

    await act(async () => {
      staleRead.resolve({ items: [item], next_cursor: null, has_more: false });
      await staleRead.promise;
    });

    await waitFor(() => expect(listCases).toHaveBeenCalledTimes(2));
    expect(screen.getByText("No compliance cases")).toBeVisible();
    expect(screen.queryByRole("button", { name: item.title })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close case" }))
      .not.toBeInTheDocument();
  });

  it("keeps a committed released hold visible under All when its follow-up read fails", async () => {
    const item = holdRecord({
      legal_hold_id: "hold-release-read-failure",
      archive_id: "archive-read503",
    });
    const released = holdRecord({
      ...item,
      status: "released",
      release_approval_request_id: "approval-release-503",
      released_by: "principal-releaser",
      released_at: "2026-08-24T02:00:00Z",
    });
    const listHolds = vi.fn()
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockResolvedValueOnce({ items: [item], next_cursor: null, has_more: false })
      .mockRejectedValueOnce(unavailableError("holds_read"));
    const releaseHold = vi.fn().mockResolvedValue(released);
    const notify = vi.fn();
    const client = { listHolds, releaseHold } as unknown as ControlPlaneClient;

    render(
      <HoldsView
        client={client}
        clearInitialCase={vi.fn()}
        locale="en"
        notify={notify}
        t={en}
      />,
    );

    await screen.findByRole("button", { name: item.archive_id });
    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), {
      target: { value: "" },
    });
    fireEvent.click(await screen.findByRole("button", { name: item.archive_id }));
    fireEvent.click(screen.getByRole("button", { name: "Release legal hold" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Approval request ID" }), {
      target: { value: "approval-release-503" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Confirm release" }));

    const readError = await screen.findByRole("alert");
    expect(readError).toHaveTextContent("holds_read_unavailable");
    expect(readError).not.toHaveTextContent("release");
    expect(notify).toHaveBeenCalledWith("Legal hold released");
    expect(screen.getByRole("button", { name: item.archive_id })).toBeVisible();
    expect(screen.getAllByText("Released").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "Release legal hold" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Release legal hold" }))
      .not.toBeInTheDocument();
    expect(releaseHold).toHaveBeenCalledTimes(1);
  });
});
