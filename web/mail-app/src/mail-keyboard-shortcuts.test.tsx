import { cleanup, fireEvent, render, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  useMailKeyboardShortcuts,
  type MailKeyboardShortcutBindings,
} from "./mail-keyboard-shortcuts";

function createBindings(overrides: Partial<MailKeyboardShortcutBindings> = {}) {
  const onOpenCommandPalette = vi.fn(() => true);
  const onCompose = vi.fn(() => true);
  const onFocusInboxSearch = vi.fn(() => true);
  const moveMessage = vi.fn((_direction: -1 | 1) => true);
  const onOpenSelectedMessage = vi.fn(() => true);
  const onCloseSelectedMessage = vi.fn(() => true);
  const onReply = vi.fn(() => true);
  const onToggleFlag = vi.fn(() => true);
  const onViewDrafts = vi.fn(() => true);
  const onViewInbox = vi.fn(() => true);
  const onViewSent = vi.fn(() => true);
  return {
    bindings: {
      enabled: true,
      modalOpen: false,
      onOpenCommandPalette,
      onCompose,
      onFocusInboxSearch,
      onMoveMessage: moveMessage,
      onOpenSelectedMessage,
      onCloseSelectedMessage,
      onReply,
      onToggleFlag,
      onViewDrafts,
      onViewInbox,
      onViewSent,
      ...overrides,
    },
    callbacks: {
      onOpenCommandPalette,
      onCompose,
      onFocusInboxSearch,
      onOpenSelectedMessage,
      onCloseSelectedMessage,
      onReply,
      onToggleFlag,
      onViewDrafts,
      onViewInbox,
      onViewSent,
    },
    moveMessage,
  };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("useMailKeyboardShortcuts", () => {
  it("opens the command palette with Ctrl or Command K before regular editable-field guards", () => {
    const { bindings, callbacks } = createBindings();
    const { getByLabelText } = render(<input aria-label="Search" />);
    const { unmount } = renderHook(() => useMailKeyboardShortcuts(bindings));

    fireEvent.keyDown(getByLabelText("Search"), { key: "k", ctrlKey: true });
    fireEvent.keyDown(document, { key: "k", metaKey: true });

    expect(callbacks.onOpenCommandPalette).toHaveBeenCalledTimes(2);
    unmount();
  });

  it("maps the promised mail navigation keys and g-prefixed view changes", () => {
    const { bindings, callbacks, moveMessage } = createBindings();
    const { unmount } = renderHook(() => useMailKeyboardShortcuts(bindings));

    fireEvent.keyDown(document, { key: "c" });
    fireEvent.keyDown(document, { key: "/" });
    fireEvent.keyDown(document, { key: "j" });
    fireEvent.keyDown(document, { key: "k" });
    fireEvent.keyDown(document, { key: "Enter" });
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.keyDown(document, { key: "r" });
    fireEvent.keyDown(document, { key: "s" });

    expect(moveMessage).toHaveBeenNthCalledWith(1, 1);
    expect(moveMessage).toHaveBeenNthCalledWith(2, -1);
    expect(callbacks.onCompose).toHaveBeenCalledTimes(1);
    expect(callbacks.onFocusInboxSearch).toHaveBeenCalledTimes(1);
    expect(callbacks.onOpenSelectedMessage).toHaveBeenCalledTimes(1);
    expect(callbacks.onCloseSelectedMessage).toHaveBeenCalledTimes(1);
    expect(callbacks.onReply).toHaveBeenCalledTimes(1);
    expect(callbacks.onToggleFlag).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(document, { key: "g" });
    fireEvent.keyDown(document, { key: "i" });
    expect(callbacks.onViewInbox).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(document, { key: "g" });
    fireEvent.keyDown(document, { key: "d" });
    expect(callbacks.onViewDrafts).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(document, { key: "g" });
    fireEvent.keyDown(document, { key: "s" });
    expect(callbacks.onViewSent).toHaveBeenCalledTimes(1);

    unmount();
  });

  it("leaves editable fields and modal interactions to their local controls", () => {
    const { bindings, callbacks } = createBindings();
    const { rerender, unmount } = renderHook(
      ({ modalOpen }) => useMailKeyboardShortcuts({ ...bindings, modalOpen }),
      { initialProps: { modalOpen: false } },
    );
    const { getByLabelText } = render(<input aria-label="Search" />);

    fireEvent.keyDown(getByLabelText("Search"), { key: "c" });
    expect(callbacks.onCompose).not.toHaveBeenCalled();

    rerender({ modalOpen: true });
    fireEvent.keyDown(document, { key: "c" });
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    expect(callbacks.onCompose).not.toHaveBeenCalled();
    expect(callbacks.onOpenCommandPalette).not.toHaveBeenCalled();

    unmount();
  });

  it("does not turn a held action key into repeated privileged actions", () => {
    const { bindings, callbacks } = createBindings();
    const { unmount } = renderHook(() => useMailKeyboardShortcuts(bindings));

    fireEvent.keyDown(document, { key: "c", repeat: true });
    fireEvent.keyDown(document, { key: "s", repeat: true });
    fireEvent.keyDown(document, { key: "r", repeat: true });

    expect(callbacks.onCompose).not.toHaveBeenCalled();
    expect(callbacks.onToggleFlag).not.toHaveBeenCalled();
    expect(callbacks.onReply).not.toHaveBeenCalled();
    unmount();
  });
});
