import type { ReactNode } from "react";

interface MailWorkspaceShellProps {
  sessionLocked: boolean;
  topbar: ReactNode;
  navigation: ReactNode;
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
  sessionLocked,
  topbar,
  navigation,
  content,
  overlays,
}: MailWorkspaceShellProps) {
  return (
    <main
      className="mail-shell"
      aria-busy={sessionLocked || undefined}
      data-dialog-background
      inert={sessionLocked || undefined}
    >
      {topbar}
      {navigation}
      {content}
      {overlays}
    </main>
  );
}
