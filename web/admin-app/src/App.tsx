import {
  Button,
  CheckIcon,
  ConfirmDialog,
  Dialog,
  Icon,
  IconButton,
  Panel,
  SignOutIcon,
  Spinner,
} from "@copilotmail/design-system";
import {
  Component,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { AdminNavIcon, type AdminNavIconName } from "./AdminNavIcon";
import { AdminErrorNotice } from "./AdminErrorNotice";
import { ControlPlaneClient } from "./api";
import { loadSession, logout, startLogin } from "./auth";
import type {
  CaseCommitStore,
  CommittedCaseEvidence,
  CommittedHoldEvidence,
  HoldCommitStore,
  QueryScopedCommit,
} from "./ComplianceViews";
import { configurationProblems, readRuntimeConfig } from "./config";
import { ADMIN_LOCALE_STORAGE_KEY, resolveInitialLocale, translator } from "./i18n";
import { Overview } from "./OverviewView";
import type {
  ComplianceCase,
  LegalHold,
  Locale,
} from "./types";

type View = "overview" | "cases" | "holds" | "approvals" | "roles" | "mail-domains" | "agent-inspector" | "audit" | "retention";
type AuthState =
  | { kind: "checking" }
  | { kind: "signed-out" }
  | { kind: "authenticated"; sessionContextKey: string; expiresAt: number }
  | { kind: "error"; message: string };

const config = readRuntimeConfig();

// Governance workspaces are loaded only when an operator opens them. This
// keeps the initial overview usable on constrained internal devices without
// prefetching privileged data or code for screens the session never visits.
function createLazyRoutes() {
  return {
    AuditView: lazy(() => import("./AuditView")),
    AgentInspectorView: lazy(() => import("./AgentInspectorView")),
    ApprovalView: lazy(() => import("./ApprovalView")),
    CasesView: lazy(async () => {
      const { CasesView } = await import("./ComplianceViews");
      return { default: CasesView };
    }),
    HoldsView: lazy(async () => {
      const { HoldsView } = await import("./ComplianceViews");
      return { default: HoldsView };
    }),
    MailDomainsView: lazy(() => import("./MailDomainsView")),
    RetentionView: lazy(() => import("./RetentionView")),
    RolesView: lazy(() => import("./RolesView")),
  };
}

// Compatibility exports for existing admin consumers while dialog ownership
// lives in the shared design system.
export { ConfirmDialog, Dialog };
export { Overview } from "./OverviewView";

interface RecoverableRouteBoundaryProps {
  children: ReactNode;
  reload: () => void;
  retry: () => void;
  t: ReturnType<typeof translator>;
}

/**
 * Contains rejected lazy chunks without exposing loader URLs or exception text.
 * Retry creates a fresh set of lazy wrappers; reload is an explicit final escape.
 */
export class RecoverableRouteBoundary extends Component<
  RecoverableRouteBoundaryProps,
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <Panel className="auth-card" role="alert">
        <h1>{this.props.t("workspaceLoadFailed")}</h1>
        <p>{this.props.t("workspaceLoadFailedHint")}</p>
        <div className="dialog__actions">
          <Button type="button" variant="secondary" onClick={this.props.reload}>
            {this.props.t("reloadPage")}
          </Button>
          <Button type="button" onClick={this.props.retry}>
            {this.props.t("retry")}
          </Button>
        </div>
      </Panel>
    );
  }
}

type NavigationGroupId = "command" | "operations" | "evidence" | "identity";

interface NavigationGroup {
  id: NavigationGroupId;
  items: ReadonlyArray<{ view: View; icon: AdminNavIconName }>;
}

/**
 * The admin rail groups work by operator intent, not implementation layer.
 * Every route remains independently addressable through the same local view
 * state; this only changes the navigation hierarchy presented to a Human.
 */
const navigationGroups: ReadonlyArray<NavigationGroup> = [
  {
    id: "command",
    items: [{ view: "overview", icon: "overview" }],
  },
  {
    id: "operations",
    items: [
      { view: "mail-domains", icon: "mail-domains" },
      { view: "approvals", icon: "approvals" },
      { view: "cases", icon: "cases" },
      { view: "holds", icon: "holds" },
    ],
  },
  {
    id: "evidence",
    items: [
      { view: "audit", icon: "audit" },
      { view: "retention", icon: "retention" },
    ],
  },
  {
    id: "identity",
    items: [
      { view: "agent-inspector", icon: "agent-inspector" },
      { view: "roles", icon: "roles" },
    ],
  },
];

const navigationLabels = {
  overview: "overview",
  cases: "cases",
  holds: "holds",
  approvals: "approvals",
  roles: "roles",
  "mail-domains": "domains",
  "agent-inspector": "agentInspector",
  audit: "audit",
  retention: "retention",
} as const;

const navigationGroupLabels = {
  command: "navigationGroupCommand",
  operations: "navigationGroupOperations",
  evidence: "navigationGroupEvidence",
  identity: "navigationGroupIdentity",
} as const;

function shortId(value: string): string {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
}

export function AdminNavigation({
  view,
  filter,
  navigate,
  t,
  variant = "rail",
}: {
  view: View;
  filter?: string;
  navigate: (view: View) => void;
  t: ReturnType<typeof translator>;
  variant?: "rail" | "drawer";
}) {
  const normalizedFilter = filter?.trim().toLowerCase() ?? "";
  const filteredGroups = navigationGroups
    .map((group) => ({
      ...group,
      items: normalizedFilter
        ? group.items.filter(({ view: item }) =>
            t(navigationLabels[item]).toLowerCase().includes(normalizedFilter))
        : group.items,
    }))
    .filter((group) => group.items.length > 0);
  return (
    <nav
      aria-label={variant === "drawer" ? t("adminNavigation") : t("adminCenter")}
      className={`admin-navigation admin-navigation--${variant}`}
    >
      {filteredGroups.length === 0 ? (
        <p className="admin-navigation__empty">{t("navSearchEmpty")}</p>
      ) : null}
      {filteredGroups.map((group) => {
        const groupLabel = t(navigationGroupLabels[group.id]);
        const groupLabelId = `admin-navigation-${variant}-${group.id}`;
        return (
          <div
            key={group.id}
            aria-labelledby={groupLabelId}
            className="admin-navigation__group"
            role="group"
          >
            <h2 className="admin-navigation__group-label" id={groupLabelId}>
              {groupLabel}
            </h2>
            <div className="admin-navigation__items">
              {group.items.map(({ view: item, icon }) => {
                const label = t(navigationLabels[item]);
                return (
                  <button
                    key={item}
                    aria-current={view === item ? "page" : undefined}
                    aria-label={label}
                    className={`app-nav-item${view === item ? " is-active" : ""}`}
                    type="button"
                    onClick={() => navigate(item)}
                  >
                    <AdminNavIcon name={icon} />
                    <span className="app-nav-item__label">{label}</span>
                  </button>
                );
              })}
            </div>
          </div>
        );
      })}
    </nav>
  );
}

/**
 * Below the full desktop rail, keep the current location visible while moving
 * complete navigation and session controls into a modal drawer. The shared
 * Dialog owns keyboard trapping, Escape/backdrop dismissal, background
 * isolation, and exact focus restoration to the menu trigger.
 */
export function AdaptiveNavigation({
  view,
  filter,
  navigate,
  sessionControls,
  t,
}: {
  view: View;
  filter?: string;
  navigate: (view: View) => void;
  sessionControls?: ReactNode;
  t: ReturnType<typeof translator>;
}) {
  const [drawerOpen, setDrawerOpen] = useState(false);
  const currentLabel = t(navigationLabels[view]);

  useEffect(() => {
    if (!drawerOpen || typeof window.matchMedia !== "function") return;
    const media = window.matchMedia("(max-width: 1279px)");
    const closeWhenWide = () => {
      if (!media.matches) setDrawerOpen(false);
    };

    closeWhenWide();
    media.addEventListener?.("change", closeWhenWide);
    return () => media.removeEventListener?.("change", closeWhenWide);
  }, [drawerOpen]);

  const navigateFromDrawer = (next: View) => {
    navigate(next);
    setDrawerOpen(false);
  };

  return (
    <>
      <div className="mobile-navigation-context">
        <div className="mobile-navigation-context__page">
          <span>{t("currentPage")}</span>
          <strong>{currentLabel}</strong>
        </div>
        <Button
          aria-label={t("openNavigation")}
          aria-expanded={drawerOpen}
          aria-haspopup="dialog"
          className="mobile-navigation-context__toggle"
          type="button"
          variant="secondary"
          onClick={() => setDrawerOpen(true)}
        >
          <Icon aria-hidden name="panel" size={18} />
          <span className="mobile-navigation-context__toggle-label">{t("openNavigation")}</span>
        </Button>
      </div>
      {drawerOpen ? (
        <Dialog
          backdropClassName="adaptive-nav-backdrop"
          className="adaptive-nav-drawer"
          closeLabel={t("closeNavigation")}
          title={t("adminNavigation")}
          onClose={() => setDrawerOpen(false)}
        >
          <div className="adaptive-nav-drawer__body">
            <p className="adaptive-nav-drawer__current">
              <span>{t("currentPage")}</span>
              <strong>{currentLabel}</strong>
            </p>
            <AdminNavigation
              filter={filter}
              navigate={navigateFromDrawer}
              t={t}
              variant="drawer"
              view={view}
            />
            {sessionControls ? (
              <div className="adaptive-nav-drawer__session">
                {sessionControls}
              </div>
            ) : null}
          </div>
        </Dialog>
      ) : null}
    </>
  );
}

export function AdminSessionControls({
  locale,
  organizationId,
  onLocaleChange,
  onSignOut,
  t,
}: {
  locale: Locale;
  organizationId: string;
  onLocaleChange: (locale: Locale) => void;
  onSignOut: () => void;
  t: ReturnType<typeof translator>;
}) {
  return (
    <div className="sidebar__footer">
      <div className="sidebar__tenant">
        <span>{t("tenant")}</span>
        <code title={organizationId}>{shortId(organizationId)}</code>
      </div>
      <LocaleSwitch locale={locale} setLocale={onLocaleChange} t={t} />
      <Button type="button" variant="quiet" onClick={onSignOut}>
        <SignOutIcon aria-hidden="true" size={17} />
        {t("signOut")}
      </Button>
    </div>
  );
}

function AuthenticatedWorkspace({
  client,
  locale,
  onLocaleChange,
  onSignOut,
  organizationId,
  sessionRefreshing,
  t,
}: {
  client: ControlPlaneClient;
  locale: Locale;
  onLocaleChange: (locale: Locale) => void;
  onSignOut: () => void;
  organizationId: string;
  sessionRefreshing: boolean;
  t: ReturnType<typeof translator>;
}) {
  const [view, setView] = useState<View>("overview");
  const [navQuery, setNavQuery] = useState("");
  const [caseFilter, setCaseFilter] = useState<string>();
  const [caseFocusId, setCaseFocusId] = useState<string>();
  const [toast, setToast] = useState<string>();
  const [navigationAnnouncement, setNavigationAnnouncement] = useState<string>();
  const [lazyGeneration, setLazyGeneration] = useState(0);
  const [committedCaseEvidence, setCommittedCaseEvidence] = useState<CommittedCaseEvidence>();
  const [committedHoldEvidence, setCommittedHoldEvidence] = useState<CommittedHoldEvidence>();
  const committedCases = useRef(new Map<string, QueryScopedCommit<ComplianceCase>>());
  const committedHolds = useRef(new Map<string, QueryScopedCommit<LegalHold>>());
  const workspaceRef = useRef<HTMLElement>(null);
  const pendingDecisionFocus = useRef(false);
  const lazyRoutes = useMemo(createLazyRoutes, [lazyGeneration]);
  const caseCommitStore: CaseCommitStore = {
    terminal: committedCases.current,
    evidence: committedCaseEvidence,
    setEvidence: setCommittedCaseEvidence,
  };
  const holdCommitStore: HoldCommitStore = {
    terminal: committedHolds.current,
    evidence: committedHoldEvidence,
    setEvidence: setCommittedHoldEvidence,
  };
  useEffect(() => {
    if (!toast) return;
    const timeout = window.setTimeout(() => setToast(undefined), 5000);
    return () => window.clearTimeout(timeout);
  }, [toast]);
  useEffect(() => {
    if (!sessionRefreshing) return;
    const targets = Array.from(document.querySelectorAll<HTMLElement>(
      "[data-dialog-background], .dialog-backdrop",
    ));
    const previous = targets.map((target) => ({
      target,
      ariaHidden: target.getAttribute("aria-hidden"),
      inert: target.getAttribute("inert"),
    }));
    targets.forEach((target) => {
      target.setAttribute("aria-hidden", "true");
      target.setAttribute("inert", "");
    });
    return () => previous.forEach(({ target, ariaHidden, inert }) => {
      if (ariaHidden === null) target.removeAttribute("aria-hidden");
      else target.setAttribute("aria-hidden", ariaHidden);
      if (inert === null) target.removeAttribute("inert");
      else target.setAttribute("inert", inert);
    });
  }, [sessionRefreshing]);
  useEffect(() => {
    if (!pendingDecisionFocus.current) return;
    const frame = window.requestAnimationFrame(() => {
      workspaceRef.current?.focus();
      pendingDecisionFocus.current = false;
    });
    return () => window.cancelAnimationFrame(frame);
  }, [view]);
  function navigate(next: View, focusWorkspace = false) {
    if (next !== view && focusWorkspace) {
      pendingDecisionFocus.current = true;
      setNavigationAnnouncement(`${t("navigationChanged")}: ${t(navigationLabels[next])}`);
    }
    setView(next);
    if (next !== "holds") setCaseFilter(undefined);
    if (next !== "cases") setCaseFocusId(undefined);
  }
  function openHoldsForCase(caseId: string) {
    setCaseFocusId(undefined);
    setCaseFilter(caseId);
    setView("holds");
  }
  function openCaseForHold(caseId: string) {
    setCaseFilter(undefined);
    setCaseFocusId(caseId);
    setView("cases");
  }
  const {
    AgentInspectorView,
    ApprovalView,
    AuditView,
    CasesView,
    HoldsView,
    MailDomainsView,
    RetentionView,
    RolesView,
  } = lazyRoutes;
  return (
    <>
    <div className="app-shell" data-dialog-background>
      <header className="app-topbar admin-topbar">
        <div className="admin-topbar__start">
          <AdaptiveNavigation
            filter={navQuery}
            navigate={navigate}
            sessionControls={(
              <AdminSessionControls
                locale={locale}
                organizationId={organizationId}
                onLocaleChange={onLocaleChange}
                onSignOut={onSignOut}
                t={t}
              />
            )}
            t={t}
            view={view}
          />
          <span className="app-wordmark">
            {t("product")}
            <small>{t("adminCenter")}</small>
          </span>
        </div>
        <div className="admin-topbar__search search-omnibox" role="search">
          <Icon aria-hidden className="admin-topbar__search-icon" name="search" size={20} />
          <input
            aria-label={t("navSearchAria")}
            maxLength={64}
            placeholder={t("navSearchPlaceholder")}
            value={navQuery}
            onChange={(event) => setNavQuery(event.target.value)}
          />
          {navQuery ? (
            <IconButton
              className="admin-topbar__search-clear"
              icon="close"
              label={t("clearFilter")}
              onClick={() => setNavQuery("")}
            />
          ) : null}
        </div>
        <div
          aria-label={t("sessionRegion")}
          className="admin-topbar__session"
          role="group"
        >
          <LocaleSwitch locale={locale} setLocale={onLocaleChange} t={t} />
          <button className="topbar-sign-out" type="button" onClick={onSignOut}>
            <Icon aria-hidden name="sign-out" size={17} />
            {t("signOut")}
          </button>
        </div>
      </header>
      <aside className="sidebar">
        <AdminNavigation filter={navQuery} navigate={navigate} t={t} view={view} />
        <div className="sidebar__footer">
          <div className="sidebar__tenant">
            <span>{t("tenant")}</span>
            <code title={organizationId}>{shortId(organizationId)}</code>
          </div>
        </div>
      </aside>
      <main
        aria-label={t(navigationLabels[view])}
        className="workspace"
        ref={workspaceRef}
        tabIndex={-1}
      >
        <RecoverableRouteBoundary
          key={`${view}:${lazyGeneration}`}
          reload={() => window.location.reload()}
          retry={() => setLazyGeneration((generation) => generation + 1)}
          t={t}
        >
          <Suspense fallback={<Spinner label={t("loading")} />}>
            {view === "overview" ? <Overview client={client} navigate={(destination) => navigate(destination, true)} t={t} /> : null}
            {view === "cases" ? <CasesView client={client} commitStore={caseCommitStore} initialCaseId={caseFocusId} clearInitialCase={() => setCaseFocusId(undefined)} locale={locale} t={t} notify={setToast} openHolds={openHoldsForCase} /> : null}
            {view === "holds" ? <HoldsView client={client} commitStore={holdCommitStore} openCase={openCaseForHold} locale={locale} t={t} notify={setToast} initialCaseId={caseFilter} clearInitialCase={() => setCaseFilter(undefined)} /> : null}
            {view === "approvals" ? <ApprovalView client={client} locale={locale} t={t} /> : null}
            {view === "roles" ? <RolesView client={client} locale={locale} t={t} /> : null}
            {view === "mail-domains" ? <MailDomainsView client={client} locale={locale} t={t} /> : null}
            {view === "agent-inspector" ? <AgentInspectorView client={client} locale={locale} t={t} /> : null}
            {view === "audit" ? <AuditView client={client} locale={locale} t={t} /> : null}
            {view === "retention" ? <RetentionView client={client} locale={locale} t={t} /> : null}
          </Suspense>
        </RecoverableRouteBoundary>
      </main>
      <div className="sr-only" role="status" aria-live="polite">{navigationAnnouncement}</div>
      <div className="sr-only" role="status" aria-live="polite">{toast}</div>
      {toast ? <div className="toast" aria-hidden="true"><CheckIcon aria-hidden size={17} />{toast}</div> : null}
    </div>
    {sessionRefreshing ? (
      <div className="session-refresh-lock" role="status" aria-live="polite">
        <Spinner label={t("loading")} />
      </div>
    ) : null}
    </>
  );
}

export default function App() {
  const [locale, setLocale] = useState<Locale>(() => {
    let storedLocale: string | null = null;
    try {
      storedLocale = window.localStorage.getItem(ADMIN_LOCALE_STORAGE_KEY);
    } catch {
      // Storage can be denied by browser policy; the declared zh-CN default
      // remains a deterministic, non-blocking fallback.
    }
    return resolveInitialLocale(storedLocale);
  });
  const [auth, setAuth] = useState<AuthState>({ kind: "checking" });
  const [sessionRefreshing, setSessionRefreshing] = useState(false);
  const sessionSequence = useRef(0);
  const sessionRefresh = useRef<Promise<void> | undefined>(undefined);
  const t = useMemo(() => translator(locale), [locale]);
  const problems = configurationProblems(config);
  const hasConfigurationProblems = problems.length > 0;

  const revalidateSession = useCallback((): Promise<void> => {
    if (hasConfigurationProblems) return Promise.resolve();
    if (sessionRefresh.current) return sessionRefresh.current;
    const sequence = sessionSequence.current + 1;
    sessionSequence.current = sequence;
    setSessionRefreshing(true);
    const pending = loadSession(config)
      .then((session) => {
        if (sequence !== sessionSequence.current) return;
        setAuth(session ? {
          kind: "authenticated",
          sessionContextKey: JSON.stringify([
            session.principal_id,
            session.organization_id,
            session.expires_at,
          ]),
          expiresAt: Date.parse(session.expires_at),
        } : { kind: "signed-out" });
      })
      .catch((error: Error) => {
        if (sequence === sessionSequence.current) {
          setAuth({ kind: "error", message: error.message });
        }
      })
      .finally(() => {
        if (sessionRefresh.current === pending) sessionRefresh.current = undefined;
        if (sequence === sessionSequence.current) setSessionRefreshing(false);
      });
    sessionRefresh.current = pending;
    return pending;
  }, [hasConfigurationProblems]);

  const sessionContextKey = auth.kind === "authenticated"
    ? auth.sessionContextKey
    : undefined;
  const client = useMemo(() => (
    sessionContextKey
      ? new ControlPlaneClient(config.apiBaseUrl, config.organizationId, () => {
        void revalidateSession();
      })
      : undefined
  ), [revalidateSession, sessionContextKey]);

  useEffect(() => {
    document.documentElement.lang = locale;
    try {
      window.localStorage.setItem(ADMIN_LOCALE_STORAGE_KEY, locale);
    } catch {
      // Language selection still applies to the current document when browser
      // storage is unavailable.
    }
  }, [locale]);
  useEffect(() => {
    void revalidateSession();
  }, [revalidateSession]);
  useEffect(() => {
    if (auth.kind !== "authenticated") return;
    const onFocus = () => void revalidateSession();
    window.addEventListener("focus", onFocus);
    const remaining = Math.max(0, auth.expiresAt - Date.now());
    const timeout = window.setTimeout(
      () => void revalidateSession(),
      Math.min(remaining, 2_147_000_000),
    );
    return () => {
      window.removeEventListener("focus", onFocus);
      window.clearTimeout(timeout);
    };
  }, [auth, revalidateSession]);

  if (problems.length) {
    return <main className="centered"><Panel className="auth-card"><span className="app-wordmark auth-card__wordmark">{t("product")}</span><h1>{t("configurationRequired")}</h1><p>{t("configurationHint")}</p><div className="notice"><strong>{t("missingFields")}</strong><code>{problems.join(", ")}</code></div><LocaleSwitch locale={locale} setLocale={setLocale} t={t} /></Panel></main>;
  }
  if (auth.kind === "checking") return <main className="centered"><Spinner label={t("loading")} /></main>;
  if (auth.kind !== "authenticated") {
    return <main className="centered"><Panel className="auth-card"><span className="app-wordmark auth-card__wordmark">{t("product")}</span><h1>{t("secureSignIn")}</h1><p>{t("secureSignInHint")}</p>{auth.kind === "error" ? <AdminErrorNotice error={new Error(auth.message)} t={t} /> : null}<Button onClick={() => { try { startLogin(config); } catch (error) { setAuth({ kind: "error", message: error instanceof Error ? error.message : "login_failed" }); } }}>{t("signIn")}</Button><LocaleSwitch locale={locale} setLocale={setLocale} t={t} /></Panel></main>;
  }

  return (
    <AuthenticatedWorkspace
      key={auth.sessionContextKey}
      client={client!}
      locale={locale}
      onLocaleChange={setLocale}
      onSignOut={() => {
        sessionSequence.current += 1;
        sessionRefresh.current = undefined;
        setSessionRefreshing(false);
        setAuth({ kind: "checking" });
        logout(config)
          .then(() => setAuth({ kind: "signed-out" }))
          .catch((error: Error) => setAuth({ kind: "error", message: error.message }));
      }}
      organizationId={config.organizationId}
      sessionRefreshing={sessionRefreshing}
      t={t}
    />
  );
}

function LocaleSwitch({
  locale,
  setLocale,
  t,
}: {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: ReturnType<typeof translator>;
}) {
  return <label className="locale-switch"><span>{t("language")}</span><select value={locale} onChange={(event) => setLocale(event.target.value as Locale)}><option value="zh-CN">{t("languageNameZhCN")}</option><option value="en">{t("languageNameEn")}</option></select></label>;
}
