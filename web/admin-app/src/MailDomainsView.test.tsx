// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import MailDomainsView from "./MailDomainsView";
import type { ControlPlaneClient } from "./api";
import { translator } from "./i18n";
import type { MailDomain } from "./types";

afterEach(() => cleanup());

const organizationId = "019b0000-0000-7000-8000-000000000400";
const domainId = "019b0000-0000-7000-8000-000000000401";
const otherDomainId = "019b0000-0000-7000-8000-000000000402";

function domain(overrides: Partial<MailDomain> = {}): MailDomain {
  return {
    mail_domain_id: domainId,
    organization_id: organizationId,
    name: "pilot.example.test",
    status: "pending_dns",
    route_mode: "direct_mx",
    ownership_challenge_version: 2,
    route_version: 7,
    inbound_enabled: false,
    outbound_enabled: true,
    smart_host_ref: null,
    dkim_key_ref: "kms://mail/dkim/pilot",
    mail_engine_domain_ref: null,
    mail_engine_projection_status: "not_projected",
    mail_engine_projected_enabled: null,
    mail_engine_projection_last_attempt_at: null,
    mail_engine_projection_succeeded_at: null,
    mail_engine_projection_error_code: null,
    latest_dns_observed_at: null,
    latest_dns_valid_until: null,
    latest_dns_activation_ready: false,
    latest_dns_failure_codes: [],
    ...overrides,
  };
}

function createClient(
  initialDomains: MailDomain[] = [domain()],
  selected = domain(),
) {
  const listMailDomains = vi.fn().mockResolvedValue({
    items: initialDomains,
    next_after: null,
  });
  const getMailDomain = vi.fn().mockResolvedValue(selected);
  const createMailDomain = vi.fn().mockResolvedValue({
    mail_domain_id: domainId,
    organization_id: organizationId,
    name: "pilot.example.test",
    status: "pending_dns",
    ownership_challenge_version: 1,
    route_version: 1,
    ownership_record_name: "_copilotmail-verification.pilot.example.test",
    ownership_token: "cmv1_one-time-token",
  });
  const rotateMailDomainOwnershipChallenge = vi.fn().mockResolvedValue({
    mail_domain_id: domainId,
    status: "pending_dns",
    ownership_challenge_version: 3,
    ownership_record_name: "_copilotmail-verification.pilot.example.test",
    ownership_token: "cmv1_rotated-token",
  });
  const verifyMailDomainDns = vi.fn().mockResolvedValue({
    mail_domain_id: domainId,
    status: "verified",
    activation_ready: true,
    valid_until: "2026-08-24T12:00:00Z",
    failure_codes: [],
  });
  const activateMailDomain = vi.fn().mockResolvedValue({
    mail_domain_id: domainId,
    status: "active",
  });
  const suspendMailDomain = vi.fn().mockResolvedValue({
    mail_domain_id: domainId,
    status: "suspended",
  });
  const updateMailDomainRoute = vi.fn().mockResolvedValue({
    mail_domain_id: domainId,
    version: 8,
    inbound_enabled: true,
    outbound_enabled: false,
  });

  return {
    client: {
      listMailDomains,
      getMailDomain,
      createMailDomain,
      rotateMailDomainOwnershipChallenge,
      verifyMailDomainDns,
      activateMailDomain,
      suspendMailDomain,
      updateMailDomainRoute,
    } as unknown as ControlPlaneClient,
    listMailDomains,
    getMailDomain,
    createMailDomain,
    rotateMailDomainOwnershipChallenge,
    verifyMailDomainDns,
    activateMailDomain,
    suspendMailDomain,
    updateMailDomainRoute,
  };
}

function renderView(client: ControlPlaneClient) {
  return render(
    <MailDomainsView client={client} locale="en" t={translator("en")} />,
  );
}

async function selectDomain() {
  fireEvent.click(await screen.findByRole("button", { name: "pilot.example.test" }));
  await screen.findByRole("heading", { name: "pilot.example.test" });
}

function openEvidenceInspector() {
  const inspector = screen.getByText("Evidence & supporting configuration", { exact: true })
    .closest("details");
  expect(inspector).not.toHaveAttribute("open");
  fireEvent.click(within(inspector!).getByText("Evidence & supporting configuration", {
    exact: true,
  }));
  expect(inspector).toHaveAttribute("open");
  return inspector!;
}

describe("MailDomainsView", () => {
  it("renders DTO-backed lifecycle facts, one current decision, and collapsed evidence", async () => {
    const detail = domain({ status: "verified", latest_dns_activation_ready: true });
    const { client, getMailDomain } = createClient([domain()], detail);
    renderView(client);

    await selectDomain();

    expect(getMailDomain).toHaveBeenCalledWith(domainId);
    const lifecycle = screen.getByRole("region", { name: "Lifecycle facts" });
    const lifecycleStages = document.querySelector<HTMLElement>(".mail-domain-lifecycle");
    expect(lifecycle).toHaveTextContent("pilot.example.test");
    expect(lifecycle).toHaveTextContent("Route version");
    expect(lifecycleStages).not.toBeNull();
    expect(within(lifecycleStages!).getByText("Registered", { exact: true })).toBeVisible();
    expect(within(lifecycleStages!).getByText("Latest DNS evidence", { exact: true })).toBeVisible();
    expect(within(lifecycleStages!).getByText("Control-plane activation", { exact: true })).toBeVisible();
    expect(within(lifecycleStages!).getByText("Mail-engine projection", { exact: true })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Current decision" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Explicitly activate domain" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Verify DNS" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save route configuration" })).not.toBeInTheDocument();

    openEvidenceInspector();
    expect(screen.getByText("Ownership verification")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Mail-engine projection" })).toBeVisible();
  });

  it("discloses the registration token only in the immediate one-time dialog and erases it when closed", async () => {
    const created = domain({ ownership_challenge_version: 1, route_version: 1 });
    const { client, createMailDomain, listMailDomains } = createClient([], created);
    listMailDomains
      .mockResolvedValueOnce({ items: [], next_after: null })
      .mockResolvedValue({ items: [created], next_after: null });
    renderView(client);

    fireEvent.click(await screen.findByRole("button", { name: "Register domain" }));
    const dialog = screen.getByRole("dialog", { name: "Register mail domain" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Domain name" }), {
      target: { value: "pilot.example.test" },
    });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Outbound route mode" }), {
      target: { value: "direct_mx" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Register and get TXT token" }));

    const tokenDialog = await screen.findByRole("dialog", {
      name: "One-time DNS verification token",
    });
    expect(createMailDomain).toHaveBeenCalledWith({
      name: "pilot.example.test",
      route_mode: "direct_mx",
    });
    expect(within(tokenDialog).getByDisplayValue("cmv1_one-time-token")).toBeVisible();
    expect(await screen.findByRole("heading", { name: "pilot.example.test" })).toBeVisible();

    fireEvent.click(within(tokenDialog).getAllByRole("button", { name: "Close dialog" }).at(-1)!);
    await waitFor(() => expect(screen.queryByDisplayValue("cmv1_one-time-token")).not.toBeInTheDocument());
  });

  it("sends DNS verification only with the selected canonical domain and the transient token", async () => {
    const detail = domain();
    const { client, verifyMailDomainDns } = createClient([detail], detail);
    renderView(client);
    await selectDomain();

    fireEvent.change(screen.getByLabelText("TXT record value"), {
      target: { value: "cmv1_current-token" },
    });
    fireEvent.change(screen.getByLabelText("DKIM selector"), {
      target: { value: "mail2026" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Verify DNS" }));

    await waitFor(() => expect(verifyMailDomainDns).toHaveBeenCalledWith(domainId, {
      domain: "pilot.example.test",
      ownership_token: "cmv1_current-token",
      dkim_selector: "mail2026",
    }));
    await waitFor(() => expect(screen.getAllByText("DNS verification completed").length)
      .toBeGreaterThanOrEqual(1));
  });

  it("uses the detail route version as the exact CAS precondition", async () => {
    const detail = domain({
      status: "active",
      latest_dns_activation_ready: true,
      inbound_enabled: false,
      outbound_enabled: true,
      route_version: 7,
    });
    const { client, updateMailDomainRoute } = createClient([detail], detail);
    renderView(client);
    await selectDomain();

    await screen.findByRole("button", { name: "Save route configuration" });

    fireEvent.click(screen.getByRole("checkbox", { name: "Enable inbound" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Enable outbound" }));
    fireEvent.click(screen.getByRole("button", { name: "Save route configuration" }));

    await waitFor(() => expect(updateMailDomainRoute).toHaveBeenCalledWith(domainId, {
      expected_version: 7,
      inbound_enabled: true,
      outbound_enabled: false,
      smart_host_ref: null,
      dkim_key_ref: "kms://mail/dkim/pilot",
    }));
  });

  it("keeps route editing out of a pending-DNS current decision", async () => {
    const pending = domain({ inbound_enabled: false, outbound_enabled: false, dkim_key_ref: null });
    const { client, updateMailDomainRoute } = createClient([pending], pending);
    renderView(client);
    await selectDomain();

    expect(screen.getByRole("button", { name: "Verify DNS" })).toBeVisible();
    const evidence = openEvidenceInspector();
    expect(within(evidence).getByText("Route configuration and version")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Save route configuration" })).not.toBeInTheDocument();
    expect(updateMailDomainRoute).not.toHaveBeenCalled();
  });

  it("rejects invalid local DNS and suspension fields before a write", async () => {
    const active = domain({
      status: "active",
      latest_dns_activation_ready: false,
      inbound_enabled: false,
      outbound_enabled: false,
      dkim_key_ref: null,
    });
    const { client, verifyMailDomainDns, suspendMailDomain } = createClient([active], active);
    renderView(client);
    await selectDomain();

    fireEvent.change(screen.getByLabelText("TXT record value"), {
      target: { value: "cmv1_current-token" },
    });
    fireEvent.change(screen.getByLabelText("DKIM selector"), {
      target: { value: "-invalid" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Verify DNS" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "DKIM selector must contain 1–63 letters",
    );
    expect(verifyMailDomainDns).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Suspend domain" }));
    const suspension = screen.getByRole("dialog", { name: "Suspend domain" });
    fireEvent.change(within(suspension).getByRole("textbox", { name: "Suspension reason code" }), {
      target: { value: "Invalid Reason" },
    });
    fireEvent.click(within(suspension).getByRole("button", { name: "Suspend domain" }));
    expect(await within(suspension).findByRole("alert")).toHaveTextContent(
      "Suspension reason code may contain lowercase letters",
    );
    expect(suspendMailDomain).not.toHaveBeenCalled();
  });

  it("requires explicit confirmation for activation and leaves a suspended domain without a lifecycle write", async () => {
    const verified = domain({ status: "verified", latest_dns_activation_ready: true });
    const active = domain({ status: "active", latest_dns_activation_ready: true });
    const suspended = domain({
      status: "suspended",
      latest_dns_activation_ready: false,
      inbound_enabled: false,
      outbound_enabled: false,
    });
    const { client, activateMailDomain, suspendMailDomain, getMailDomain } = createClient(
      [verified],
      verified,
    );
    getMailDomain
      .mockResolvedValueOnce(verified)
      .mockResolvedValueOnce(active)
      .mockResolvedValue(suspended);
    renderView(client);
    await selectDomain();

    await screen.findByRole("button", { name: "Explicitly activate domain" });

    fireEvent.click(screen.getByRole("button", { name: "Explicitly activate domain" }));
    const activation = screen.getByRole("dialog", { name: "Explicitly activate domain" });
    fireEvent.click(within(activation).getByRole("button", { name: "Explicitly activate domain" }));
    await waitFor(() => expect(activateMailDomain).toHaveBeenCalledWith(domainId));

    await screen.findByRole("button", { name: "Suspend domain" });
    fireEvent.click(screen.getByRole("button", { name: "Suspend domain" }));
    const suspension = screen.getByRole("dialog", { name: "Suspend domain" });
    fireEvent.change(within(suspension).getByRole("textbox", { name: "Suspension reason code" }), {
      target: { value: "maintenance_window" },
    });
    fireEvent.click(within(suspension).getByRole("button", { name: "Suspend domain" }));
    await waitFor(() => expect(suspendMailDomain).toHaveBeenCalledWith(domainId, {
      reason_code: "maintenance_window",
    }));
    await screen.findByText("The current server state exposes no further lifecycle action.");
    expect(screen.queryByRole("button", { name: "Verify DNS" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save route configuration" })).not.toBeInTheDocument();
  });

  it("freezes an existing-domain unknown write and reconciles only its exact GET target", async () => {
    const detail = domain({ status: "active", latest_dns_activation_ready: true });
    const other = domain({
      mail_domain_id: otherDomainId,
      name: "other.example.test",
      status: "active",
    });
    const { client, updateMailDomainRoute, listMailDomains, getMailDomain } = createClient(
      [detail, other],
      detail,
    );
    updateMailDomainRoute.mockRejectedValue(new Error("connection interrupted"));
    renderView(client);
    await selectDomain();

    await screen.findByRole("button", { name: "Save route configuration" });

    fireEvent.click(await screen.findByRole("button", { name: "Save route configuration" }));
    expect(await screen.findByText("The domain write outcome is not confirmed")).toBeVisible();
    expect(screen.getByRole("button", { name: "Register domain" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save route configuration" })).toBeDisabled();
    expect(updateMailDomainRoute).toHaveBeenCalledTimes(1);

    // A frozen view does not let a different list item become a surrogate
    // reconciliation target for the unresolved route update.
    expect(screen.getByRole("button", { name: "other.example.test" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Refresh state read-only" }));
    await waitFor(() => expect(getMailDomain).toHaveBeenCalledTimes(2));
    expect(getMailDomain).toHaveBeenLastCalledWith(domainId);
    expect(listMailDomains).toHaveBeenCalledTimes(1);
    expect(updateMailDomainRoute).toHaveBeenCalledTimes(1);
  });

  it("freezes an unknown create without guessing an exact target from a broad list", async () => {
    const { client, createMailDomain, listMailDomains, getMailDomain } = createClient([]);
    createMailDomain.mockRejectedValue(new Error("connection interrupted"));
    renderView(client);

    fireEvent.click(await screen.findByRole("button", { name: "Register domain" }));
    const dialog = screen.getByRole("dialog", { name: "Register mail domain" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Domain name" }), {
      target: { value: "pilot.example.test" },
    });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Outbound route mode" }), {
      target: { value: "direct_mx" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Register and get TXT token" }));

    expect(await screen.findByText("The domain write outcome is not confirmed")).toBeVisible();
    expect(createMailDomain).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Refresh state read-only" })).not.toBeInTheDocument();
    expect(listMailDomains).toHaveBeenCalledTimes(1);
    expect(getMailDomain).not.toHaveBeenCalled();
    screen.getAllByRole("button", { name: "Register domain" }).forEach((button) => {
      expect(button).toBeDisabled();
    });
  });
});
