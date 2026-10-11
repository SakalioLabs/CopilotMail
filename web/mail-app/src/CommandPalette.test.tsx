import { useState } from "react";
import { CommandPalette, type CommandPaletteCommand } from "@copilotmail/design-system";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const commands: CommandPaletteCommand[] = [{
  id: "compose",
  label: "Compose",
  description: "Create a new message",
  icon: "compose",
  shortcut: "C",
}, {
  id: "star",
  label: "Add star",
  description: "Update this message",
  icon: "star",
  shortcut: "S",
}, {
  id: "restricted",
  label: "Restricted action",
  description: "Unavailable to this mailbox",
  icon: "send",
  disabled: true,
  disabledReason: "This mailbox is read-only",
}];

function PaletteHarness({
  availableCommands = commands,
  onCommand = vi.fn(),
}: {
  availableCommands?: CommandPaletteCommand[];
  onCommand?: (command: CommandPaletteCommand) => void;
}) {
  const [open, setOpen] = useState(false);
  const [returnFocusTo, setReturnFocusTo] = useState<HTMLElement | null>(null);
  return (
    <>
      <div
        aria-hidden="false"
        data-dialog-background
        data-testid="dialog-background"
      >
        <button
          type="button"
          onClick={(event) => {
            setReturnFocusTo(event.currentTarget);
            setOpen(true);
          }}
        >
          Open palette
        </button>
      </div>
      <CommandPalette
        closeHint="Close"
        commands={availableCommands}
        dismissLabel="Close command palette"
        emptyQueryLabel="Available actions"
        emptyState="Nothing can be done"
        noResults="No matching actions"
        open={open}
        returnFocusTo={returnFocusTo}
        searchLabel="Search commands"
        searchPlaceholder="Search actions"
        title="Command palette"
        onCommand={onCommand}
        onDismiss={() => setOpen(false)}
      />
    </>
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("CommandPalette", () => {
  it("filters real commands and runs only the active enabled command", async () => {
    const onCommand = vi.fn();
    const { container } = render(<PaletteHarness onCommand={onCommand} />);

    fireEvent.click(screen.getByRole("button", { name: "Open palette" }));
    const search = await screen.findByRole("combobox", { name: "Search commands" });
    expect(screen.getByRole("dialog", { name: "Command palette" })).toHaveAttribute(
      "aria-modal",
      "true",
    );
    expect(container.querySelector("[role='dialog']")).toBeNull();

    fireEvent.change(search, { target: { value: "star" } });
    expect(screen.getByRole("option", { name: "Add star" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Compose" })).not.toBeInTheDocument();
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onCommand).toHaveBeenCalledWith(expect.objectContaining({ id: "star" }));
  });

  it("announces actual empty and no-result states", async () => {
    const { rerender } = render(<PaletteHarness availableCommands={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Open palette" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Nothing can be done");

    fireEvent.keyDown(screen.getByRole("combobox", { name: "Search commands" }), {
      key: "Escape",
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    rerender(<PaletteHarness availableCommands={commands} />);
    fireEvent.click(screen.getByRole("button", { name: "Open palette" }));
    fireEvent.change(await screen.findByRole("combobox", { name: "Search commands" }), {
      target: { value: "does-not-exist" },
    });
    expect(await screen.findByRole("status")).toHaveTextContent("No matching actions");
  });

  it("makes marked application backgrounds inert and restores their exact attributes", async () => {
    render(<PaletteHarness />);
    const background = screen.getByTestId("dialog-background");
    expect(background).toHaveAttribute("aria-hidden", "false");
    expect(background).not.toHaveAttribute("inert");

    fireEvent.click(screen.getByRole("button", { name: "Open palette" }));
    await screen.findByRole("dialog", { name: "Command palette" });
    expect(background).toHaveAttribute("aria-hidden", "true");
    expect(background).toHaveAttribute("inert", "");

    fireEvent.keyDown(screen.getByRole("combobox", { name: "Search commands" }), {
      key: "Escape",
    });
    await waitFor(() => {
      expect(background).toHaveAttribute("aria-hidden", "false");
      expect(background).not.toHaveAttribute("inert");
    });
  });

  it("traps Tab, restores the opener after Escape, and cannot execute disabled actions", async () => {
    const onCommand = vi.fn();
    render(<PaletteHarness onCommand={onCommand} />);
    const opener = screen.getByRole("button", { name: "Open palette" });
    fireEvent.click(opener);
    const search = await screen.findByRole("combobox", { name: "Search commands" });
    await waitFor(() => expect(search).toHaveFocus());

    const dismiss = screen.getByRole("button", { name: "Close command palette" });
    dismiss.focus();
    fireEvent.keyDown(dismiss, { key: "Tab", shiftKey: true });
    await waitFor(() => expect(screen.getByRole("option", { name: "Compose" })).toHaveFocus());
    fireEvent.keyDown(screen.getByRole("option", { name: "Compose" }), { key: "Tab" });
    await waitFor(() => expect(dismiss).toHaveFocus());
    search.focus();

    fireEvent.change(search, { target: { value: "restricted" } });
    const restricted = screen.getByRole("option", { name: "Restricted action" });
    expect(restricted).toBeDisabled();
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onCommand).not.toHaveBeenCalled();

    fireEvent.keyDown(search, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(opener).toHaveFocus();
    });
  });

  it("reclaims escaped focus and dismisses only a complete direct backdrop gesture", async () => {
    render(<PaletteHarness />);
    const opener = screen.getByRole("button", { name: "Open palette" });
    fireEvent.click(opener);
    const search = await screen.findByRole("combobox", { name: "Search commands" });
    await waitFor(() => expect(search).toHaveFocus());

    opener.focus();
    expect(search).toHaveFocus();

    const dialog = screen.getByRole("dialog", { name: "Command palette" });
    const backdrop = dialog.parentElement;
    expect(backdrop).not.toBeNull();
    if (!backdrop) return;

    fireEvent.pointerDown(dialog, { pointerId: 7 });
    fireEvent.pointerUp(backdrop, { pointerId: 7 });
    fireEvent.click(backdrop);
    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();

    fireEvent.pointerDown(backdrop, { pointerId: 8 });
    fireEvent.pointerUp(backdrop, { pointerId: 8 });
    fireEvent.click(backdrop);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() => expect(opener).toHaveFocus());
  });
});
