import type { ReactNode } from "react";

interface MailWorkspaceShellProps {
  messageOpen: boolean;
  directoryView: boolean;
  sessionLocked: boolean;
  desktopNavigation: ReactNode;
  mobileToolbar: ReactNode;
  content: ReactNode;
  overlays: ReactNode;
}

/**
 * Structural boundary for the authenticated mail workspace.
 *
 * It intentionally owns only the stable DOM shell and explicit render slots.
 * Data loading, authorization, focus restoration, keyboard interaction, and
 * all state transitions remain with the authenticated workspace controller.
 */
export function MailWorkspaceShell({
  messageOpen,
  directoryView,
  sessionLocked,
  desktopNavigation,
  mobileToolbar,
  content,
  overlays,
}: MailWorkspaceShellProps) {
  return (
    <main
      className={[
        "mail-shell",
        messageOpen ? "message-open" : "",
        directoryView ? "mail-shell--directory" : "",
      ].filter(Boolean).join(" ")}
      aria-busy={sessionLocked || undefined}
      data-dialog-background
      inert={sessionLocked || undefined}
    >
      {desktopNavigation}
      {mobileToolbar}
      {content}
      {overlays}
    </main>
  );
}
