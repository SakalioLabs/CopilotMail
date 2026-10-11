import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConfirmDialog, Dialog } from "./dialog";

afterEach(() => {
  cleanup();
  document.body.replaceChildren();
});

describe("shared dialog focus and dismissal contract", () => {
  it("portals, isolates declared backgrounds, and restores exact attributes and focus", () => {
    const background = document.createElement("main");
    background.dataset.dialogBackground = "";
    background.setAttribute("aria-hidden", "false");
    background.setAttribute("inert", "preserved");
    const trigger = document.createElement("button");
    trigger.textContent = "Open";
    background.append(trigger);
    document.body.append(background);
    trigger.focus();

    const { container, unmount } = render(
      <Dialog closeLabel="Close" title="Evidence" onClose={vi.fn()}>
        <button type="button">Continue</button>
      </Dialog>,
    );

    const dialog = screen.getByRole("dialog", { name: "Evidence" });
    expect(container.querySelector("[role='dialog']")).toBeNull();
    expect(dialog.parentElement?.parentElement).toBe(document.body);
    expect(screen.getByRole("button", { name: "Close" })).toHaveFocus();
    expect(background).toHaveAttribute("aria-hidden", "true");
    expect(background).toHaveAttribute("inert", "");

    unmount();
    expect(background).toHaveAttribute("aria-hidden", "false");
    expect(background).toHaveAttribute("inert", "preserved");
    expect(trigger).toHaveFocus();
  });

  it("reclaims escaped focus and wraps both Tab directions", () => {
    const outside = document.createElement("button");
    outside.textContent = "Outside";
    document.body.append(outside);
    render(
      <Dialog closeLabel="Close" title="Evidence" onClose={vi.fn()}>
        <input aria-label="Reference" />
        <button type="button">Continue</button>
      </Dialog>,
    );

    const close = screen.getByRole("button", { name: "Close" });
    const last = screen.getByRole("button", { name: "Continue" });
    outside.focus();
    expect(close).toHaveFocus();

    last.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(close).toHaveFocus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
  });

  it("blocks close, Escape, and backdrop dismissal while busy", () => {
    const onClose = vi.fn();
    render(
      <Dialog
        busy
        closeLabel="Close"
        dismissDisabled
        title="Evidence"
        onClose={onClose}
      >
        <button type="button">Continue</button>
      </Dialog>,
    );

    const dialog = screen.getByRole("dialog", { name: "Evidence" });
    const close = screen.getByRole("button", { name: "Close" });
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(close).toBeDisabled();
    fireEvent.click(close);
    fireEvent.keyDown(document, { key: "Escape" });
    if (dialog.parentElement) fireEvent.click(dialog.parentElement);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("dismisses only a pointer gesture that both starts and ends on the backdrop", () => {
    const onClose = vi.fn();
    render(
      <Dialog closeLabel="Close" title="Evidence" onClose={onClose}>
        <button type="button">Continue</button>
      </Dialog>,
    );

    const dialog = screen.getByRole("dialog", { name: "Evidence" });
    const backdrop = dialog.parentElement;
    expect(backdrop).not.toBeNull();
    if (!backdrop) return;

    fireEvent.pointerDown(dialog, { pointerId: 7 });
    fireEvent.pointerUp(backdrop, { pointerId: 7 });
    fireEvent.click(backdrop);
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.pointerDown(backdrop, { pointerId: 8 });
    fireEvent.pointerUp(backdrop, { pointerId: 8 });
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("supports a close-icon-free alertdialog and moves focus off disabled busy actions", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    const returnFocusRef = createRef<HTMLButtonElement>();
    const { rerender } = render(
      <ConfirmDialog
        cancelLabel="Keep current"
        closeLabel="Close confirmation"
        confirmLabel="Restore"
        description="The current edit will be replaced."
        returnFocusRef={returnFocusRef}
        role="alertdialog"
        showCloseButton={false}
        title="Replace edit?"
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );

    const confirm = screen.getByRole("button", { name: "Restore" });
    expect(screen.getByRole("alertdialog", { name: "Replace edit?" }))
      .toHaveAccessibleDescription("The current edit will be replaced.");
    expect(screen.queryByRole("button", { name: "Close confirmation" }))
      .not.toBeInTheDocument();
    confirm.focus();
    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);

    rerender(
      <ConfirmDialog
        busy
        cancelLabel="Keep current"
        closeLabel="Close confirmation"
        confirmLabel="Restoring"
        description="The current edit will be replaced."
        returnFocusRef={returnFocusRef}
        role="alertdialog"
        showCloseButton={false}
        title="Replace edit?"
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );
    const dialog = screen.getByRole("alertdialog", { name: "Replace edit?" });
    expect(dialog).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).not.toHaveBeenCalled();
  });
});
