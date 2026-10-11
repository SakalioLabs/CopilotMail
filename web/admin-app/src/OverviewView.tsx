import { Badge, Button, Panel, Spinner } from "@copilotmail/design-system";
import { useCallback, useEffect, useRef, useState } from "react";
import { AdminErrorNotice } from "./AdminErrorNotice";
import { ApiError, ControlPlaneClient } from "./api";
import { translator } from "./i18n";

export type OverviewDestination = "cases" | "holds";

type OverviewMetricState =
  | { kind: "loading" }
  | { kind: "ready"; count: string; empty: boolean }
  | { kind: "denied"; error: unknown }
  | { kind: "error"; error: unknown };

export type OverviewDecisionKind =
  | "cases"
  | "holds"
  | "empty"
  | "unknown";

/**
 * The Overview is deliberately a bounded decision surface, not a dashboard.
 * It can only rank a next action after both protected reads make that ranking
 * true. In particular, a denied, loading, or failed read is never treated as
 * an empty result.
 */
export function deriveOverviewDecision(
  cases: OverviewMetricState,
  holds: OverviewMetricState,
): OverviewDecisionKind {
  if (cases.kind === "ready" && !cases.empty) return "cases";
  if (cases.kind === "ready" && cases.empty && holds.kind === "ready" && !holds.empty) {
    return "holds";
  }
  if (cases.kind === "ready" && cases.empty && holds.kind === "ready" && holds.empty) {
    return "empty";
  }
  return "unknown";
}

function OverviewFact({
  emptyHint,
  label,
  retry,
  state,
  t,
}: {
  emptyHint: string;
  label: string;
  retry: () => void;
  state: OverviewMetricState;
  t: ReturnType<typeof translator>;
}) {
  return (
    <Panel
      aria-label={label}
      className="overview-fact"
      data-overview-state={state.kind}
    >
      <span className="overview-fact__label">{label}</span>
      <div className="overview-fact__state" aria-live="polite">
        {state.kind === "loading" ? <Spinner label={t("loading")} /> : null}
        {state.kind === "ready" ? (
          <>
            <strong>{state.count}</strong>
            {state.empty ? <p>{emptyHint}</p> : null}
          </>
        ) : null}
        {state.kind === "denied" ? (
          <>
            <Badge tone="critical">{t("denied")}</Badge>
            <AdminErrorNotice error={state.error} t={t} />
          </>
        ) : null}
        {state.kind === "error" ? (
          <AdminErrorNotice error={state.error} retry={retry} t={t} />
        ) : null}
      </div>
    </Panel>
  );
}

function DecisionSurface({
  decision,
  navigate,
  t,
}: {
  decision: OverviewDecisionKind;
  navigate: (destination: OverviewDestination) => void;
  t: ReturnType<typeof translator>;
}) {
  const content = {
    cases: {
      hint: t("overviewDecisionCasesHint"),
      title: t("overviewDecisionCasesTitle"),
    },
    holds: {
      hint: t("overviewDecisionHoldsHint"),
      title: t("overviewDecisionHoldsTitle"),
    },
    empty: {
      hint: t("overviewDecisionEmptyHint"),
      title: t("overviewDecisionEmptyTitle"),
    },
    unknown: {
      hint: t("overviewDecisionUnknownHint"),
      title: t("overviewDecisionUnknownTitle"),
    },
  }[decision];
  const destination: OverviewDestination | undefined = decision === "cases"
    ? "cases"
    : decision === "holds"
      ? "holds"
      : undefined;
  const eyebrow = decision === "empty"
    ? t("overviewVerifiedState")
    : decision === "unknown"
      ? t("overviewDecisionBoundary")
      : t("overviewDecision");

  return (
    <section
      aria-labelledby="overview-decision-heading"
      className="overview-decision"
      data-overview-decision={decision}
    >
      <p className="eyebrow">{eyebrow}</p>
      <h2 id="overview-decision-heading">{content.title}</h2>
      <p>{content.hint}</p>
      {destination ? (
        <Button type="button" onClick={() => navigate(destination)}>
          {destination === "cases" ? t("viewOpenCases") : t("viewActiveHolds")}
        </Button>
      ) : null}
    </section>
  );
}

export function Overview({
  client,
  navigate,
  t,
}: {
  client: ControlPlaneClient;
  navigate: (destination: OverviewDestination) => void;
  t: ReturnType<typeof translator>;
}) {
  const [casesState, setCasesState] = useState<OverviewMetricState>({ kind: "loading" });
  const [holdsState, setHoldsState] = useState<OverviewMetricState>({ kind: "loading" });
  const casesSequence = useRef(0);
  const holdsSequence = useRef(0);
  const loadCases = useCallback(async () => {
    const sequence = casesSequence.current + 1;
    casesSequence.current = sequence;
    setCasesState({ kind: "loading" });
    try {
      const page = await client.listCases({ status: "open", limit: 100 });
      if (sequence !== casesSequence.current) return;
      setCasesState({
        kind: "ready",
        count: `${page.items.length}${page.has_more ? "+" : ""}`,
        empty: page.items.length === 0,
      });
    } catch (error) {
      if (sequence !== casesSequence.current) return;
      setCasesState({
        kind: error instanceof ApiError && error.status === 403 ? "denied" : "error",
        error,
      });
    }
  }, [client]);
  const loadHolds = useCallback(async () => {
    const sequence = holdsSequence.current + 1;
    holdsSequence.current = sequence;
    setHoldsState({ kind: "loading" });
    try {
      const page = await client.listHolds({ status: "active", limit: 100 });
      if (sequence !== holdsSequence.current) return;
      setHoldsState({
        kind: "ready",
        count: `${page.items.length}${page.has_more ? "+" : ""}`,
        empty: page.items.length === 0,
      });
    } catch (error) {
      if (sequence !== holdsSequence.current) return;
      setHoldsState({
        kind: error instanceof ApiError && error.status === 403 ? "denied" : "error",
        error,
      });
    }
  }, [client]);

  useEffect(() => {
    void loadCases();
    void loadHolds();
    return () => {
      casesSequence.current += 1;
      holdsSequence.current += 1;
    };
  }, [loadCases, loadHolds]);

  const decision = deriveOverviewDecision(casesState, holdsState);

  return (
    <div className="overview-workbench">
      <header className="page-heading overview-heading">
        <div>
          <p className="eyebrow">{t("adminCenter")}</p>
          <h1>{t("overviewTitle")}</h1>
          <p>{t("overviewHint")}</p>
        </div>
      </header>
      <DecisionSurface decision={decision} navigate={navigate} t={t} />
      <section aria-labelledby="overview-facts-heading" className="overview-facts">
        <div className="overview-facts__heading">
          <h2 id="overview-facts-heading">{t("overviewFacts")}</h2>
        </div>
        <div className="overview-fact-grid">
          <OverviewFact
            emptyHint={t("noOpenCases")}
            label={t("openCases")}
            retry={() => void loadCases()}
            state={casesState}
            t={t}
          />
          <OverviewFact
            emptyHint={t("noActiveHolds")}
            label={t("activeHolds")}
            retry={() => void loadHolds()}
            state={holdsState}
            t={t}
          />
        </div>
      </section>
    </div>
  );
}
