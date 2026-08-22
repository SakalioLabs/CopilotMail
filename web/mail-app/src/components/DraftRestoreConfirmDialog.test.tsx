// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../i18n";
import { DraftRestoreConfirmDialog } from "./DraftRestoreConfirmDialog";

const t = createTranslator("en");

afterEach(() => {
  cleanup();
  document.body.replaceChildren();
});

describe("DraftRestoreConfirmDialog", () => {
  it("isolates background content, focuses the non-destructive action, and restores focus", () => {
    const background = document.createElement("main");
    background.dataset.dialogBackground = "";
    const trigger = document.createElement("button");
    trigger.textContent = "Resume editing";
    background.append(trigger);
    document.body.append(background);
    trigger.focus();
    const { unmount } = render(
      <DraftRestoreConfirmDialog t={t} onCancel={vi.fn()} onConfirm={vi.fn()} />,
    );

    expect(screen.getByRole("alertdialog", { name: "Replace the current message?" }))
      .toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("button", { name: "Keep current content" })).toHaveFocus();
    expect(background).toHaveAttribute("inert");
    expect(background).toHaveAttribute("aria-hidden", "true");

    unmount();
    expect(background).not.toHaveAttribute("inert");
    expect(background).not.toHaveAttribute("aria-hidden");
    expect(trigger).toHaveFocus();
  });

  it("supports Escape and traps Tab between its two actions", () => {
    const onCancel = vi.fn();
    render(<DraftRestoreConfirmDialog t={t} onCancel={onCancel} onConfirm={vi.fn()} />);
    const cancel = screen.getByRole("button", { name: "Keep current content" });
    const confirm = screen.getByRole("button", { name: "Restore draft" });

    confirm.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("keeps focus inside the mounted dialog and blocks dismissal while restore is busy", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    const { rerender } = render(
      <DraftRestoreConfirmDialog
        t={t}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Restore draft" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);

    rerender(
      <DraftRestoreConfirmDialog
        busy
        t={t}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );
    const dialog = screen.getByRole("alertdialog", {
      name: "Replace the current message?",
    });
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(dialog).toHaveFocus();
    expect(screen.getByRole("button", { name: "Keep current content" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Restoring…" })).toBeDisabled();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(dialog).toHaveFocus();
  });
});
