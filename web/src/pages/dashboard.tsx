import type React from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import Layout from "@theme/Layout";
import Head from "@docusaurus/Head";
import BrowserOnly from "@docusaurus/BrowserOnly";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import { supabase } from "../lib/supabase";
import { proRequiredMessage } from "../lib/pro-required";
import { confirmsAccountDeletion, deleteAccount } from "../lib/account";
import {
  GITHUB_APP_INSTALL_URL,
  STRIPE_PUBLISHABLE_KEY,
} from "../lib/constants";
import {
  fetchBillingPortalUrl,
  fetchCheckoutUrl,
  functionErrorMessage,
} from "../lib/billing";
import {
  formatPeriodEnd,
  isProSubscription,
  subscriptionPeriodLabel,
  subscriptionStatusLabel,
  type Subscription,
} from "../lib/subscription";
import {
  extraSeats,
  forgetPendingInvite,
  invitePage,
  pendingInviteToken,
  stashInviteForSignIn,
  TEAM_EXTRA_SEAT_USD,
  TEAM_INCLUDED_SEATS,
  type OrganizationAction,
} from "../lib/teams";
import type { User, Session } from "@supabase/supabase-js";

interface GithubRepo {
  id: number;
  owner: string;
  name: string;
  full_name: string;
  private: boolean;
  default_branch: string;
  installation_id: number;
}

type Tab = "account" | "integrations";

// Older links (pricing, invite emails, GitHub callback) still say
// ?tab=subscription or ?tab=team. Both now live on Account.
function tabFromSearch(search: string): Tab {
  return new URLSearchParams(search).get("tab") === "integrations"
    ? "integrations"
    : "account";
}

// ── Repo row ─────────────────────────────────────────────────────────────────

function RepoRow({ repo }: { repo: GithubRepo }) {
  return (
    <div style={styles.repoCard}>
      <div style={styles.repoHeader}>
        <div style={styles.repoMeta}>
          <span style={styles.repoName}>{repo.full_name}</span>
          {repo.private && <span style={styles.privateBadge}>private</span>}
        </div>
      </div>
    </div>
  );
}

// ── Integrations tab ─────────────────────────────────────────────────────────

function IntegrationsTab({ onUpgrade }: { onUpgrade: () => void }) {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as
    | { githubApp?: boolean }
    | undefined;
  const githubAppEnabled = Boolean(flags?.githubApp);

  const [repos, setRepos] = useState<GithubRepo[]>([]);
  const [loadingRepos, setLoadingRepos] = useState(true);
  const [installError, setInstallError] = useState<string | null>(null);
  // The server refused the install because the user has no Pro.
  const [installNeedsPro, setInstallNeedsPro] = useState<string | null>(null);

  // Installation linking requires a server-created single-use intent; the
  // opaque state travels through GitHub's install flow back to our callback.
  const startInstall = useCallback(async () => {
    if (!githubAppEnabled) return;
    setInstallError(null);
    setInstallNeedsPro(null);
    const { data, error } = await supabase.functions.invoke(
      "create-github-install-intent",
      { body: {} }
    );
    const needsPro = await proRequiredMessage(error);
    if (needsPro) {
      setInstallNeedsPro(needsPro);
      return;
    }
    if (error || !data?.state) {
      setInstallError(error?.message ?? "Failed to start installation");
      return;
    }
    window.location.href = `${GITHUB_APP_INSTALL_URL}?state=${encodeURIComponent(data.state)}`;
  }, [githubAppEnabled]);

  const loadRepos = useCallback(async () => {
    setLoadingRepos(true);
    const { data } = await supabase
      .from("github_repositories")
      .select("*, github_app_installations!inner(linked_user_id)")
      .order("full_name");
    setRepos((data ?? []) as GithubRepo[]);
    setLoadingRepos(false);
  }, []);

  useEffect(() => {
    loadRepos();
  }, [loadRepos]);

  const isConnected = repos.length > 0;

  return (
    <div>
      <h2 style={styles.sectionTitle}>Integrations</h2>

      {/* GitHub App */}
      <div style={styles.card}>
        <div className="dashboard-integration-header" style={styles.integrationHeader}>
          <div style={styles.integrationInfo}>
            <div style={styles.integrationIcon}>
              <svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
                <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
              </svg>
            </div>
            <div>
              <div style={styles.integrationName}>GitHub</div>
              <div style={styles.integrationDesc}>
                GitHub management powered by Treq.
              </div>
              <ul style={styles.integrationFeatureList}>
                <li>
                  <code>gh</code> CLI Integration
                </li>
                <li>Stacked PRs</li>
                <li>Issues and PR management</li>
                <li>
                  GitHub App Integration{" "}
                  <span style={styles.featureProBadge}>PRO</span>
                </li>
              </ul>
            </div>
          </div>
          <div style={styles.integrationActions}>
            {isConnected ? (
              <span style={styles.connectedBadge}>Connected</span>
            ) : (
              <div>
                <span
                  title={!githubAppEnabled ? "Coming Soon" : undefined}
                  style={
                    !githubAppEnabled
                      ? { display: "inline-block", cursor: "not-allowed" }
                      : undefined
                  }
                >
                  <button
                    type="button"
                    onClick={startInstall}
                    disabled={!githubAppEnabled}
                    style={{
                      ...styles.primaryButton,
                      border: "none",
                      ...(!githubAppEnabled
                        ? styles.primaryButtonDisabled
                        : { cursor: "pointer" }),
                    }}
                  >
                    Install GitHub App
                  </button>
                </span>
                {!githubAppEnabled && (
                  <div style={styles.comingSoonSubtext}>Coming Soon</div>
                )}
              </div>
            )}
          </div>
        </div>

        {installError && (
          <div style={{ color: "#ef4444", fontSize: "0.85rem", marginTop: "0.5rem" }}>
            {installError}
          </div>
        )}

        {installNeedsPro && (
          <div style={{ fontSize: "0.85rem", marginTop: "0.75rem" }}>
            <div>
              {installNeedsPro}
              <span style={styles.featureProBadge}>PRO</span>
            </div>
            <button
              type="button"
              onClick={onUpgrade}
              style={{ ...styles.primaryButton, marginTop: "0.5rem" }}
            >
              Upgrade to Pro
            </button>
          </div>
        )}

        {isConnected && (
          <div style={styles.repoSection}>
            <div className="dashboard-row" style={styles.repoSectionHeader}>
              <span style={styles.repoSectionTitle}>
                {repos.length} {repos.length === 1 ? "repository" : "repositories"}
              </span>
              {githubAppEnabled && (
                <a
                  href={GITHUB_APP_INSTALL_URL}
                  className="dashboard-link-button" style={styles.manageLink}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  Manage access →
                </a>
              )}
            </div>
            {loadingRepos ? (
              <div style={styles.loadingText}>Loading repositories…</div>
            ) : (
              repos.map((repo) => <RepoRow key={repo.id} repo={repo} />)
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// Lucide "credit-card", sized to sit before a button label.
function CreditCardIcon() {
  return (
    <svg
      aria-hidden="true"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{ verticalAlign: "-3px", marginRight: "0.4rem" }}
    >
      <rect width="20" height="14" x="2" y="5" rx="2" />
      <line x1="2" x2="22" y1="10" y2="10" />
    </svg>
  );
}

// ── Subscription tab ─────────────────────────────────────────────────────────

// After embedded Checkout returns, Pro appears once the stripe-webhook
// function has recorded the subscription. Poll until then.
const CHECKOUT_RETURN_POLL_MS = 2000;
const CHECKOUT_RETURN_MAX_POLLS = 15;

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div role="group" aria-label={label} className="dashboard-field" style={styles.field}>
      <span style={styles.fieldLabel}>{label}</span>
      <div style={styles.fieldValue}>{children}</div>
    </div>
  );
}

// Sends the browser to Stripe-hosted Checkout. billing-checkout opens the
// session: Pro for the user, or Team when an organization id is given.
// Stripe returns to the dashboard with ?session_id when payment completes.
function CheckoutRedirect({
  onError,
  organizationId,
}: {
  onError: (message: string) => void;
  organizationId?: string;
}) {
  useEffect(() => {
    let cancelled = false;
    fetchCheckoutUrl(
      organizationId
        ? { plan: "team", organization_id: organizationId }
        : { plan: "pro" },
    )
      .then((url) => {
        if (!cancelled) window.location.assign(url);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          onError(err instanceof Error ? err.message : "Could not start checkout");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [onError, organizationId]);

  return (
    <div role="status" style={{ marginTop: "1rem" }}>
      Opening Stripe Checkout…
    </div>
  );
}

function SubscriptionTab({
  subscription,
  reloadSubscription,
  stripePaymentsEnabled,
}: {
  subscription: Subscription | null;
  reloadSubscription: () => Promise<Subscription | null>;
  stripePaymentsEnabled: boolean;
}) {
  const isPro = isProSubscription(subscription);
  // Production builds open Checkout only once the live key is filled in.
  const checkoutAvailable = stripePaymentsEnabled && STRIPE_PUBLISHABLE_KEY !== "";

  const [billingCustomer, setBillingCustomer] = useState<
    { trial_used_at: string | null } | null | undefined
  >(undefined);
  const [checkoutOpen, setCheckoutOpen] = useState(false);
  const [checkoutReturn, setCheckoutReturn] = useState<
    "waiting" | "timed_out" | null
  >(() =>
    new URLSearchParams(window.location.search).has("session_id")
      ? "waiting"
      : null,
  );
  const [portalPending, setPortalPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // An owner can also read their organizations' customers. Only the
    // user's own one belongs here.
    supabase
      .from("billing_customers")
      .select("trial_used_at")
      .eq("owner_type", "user")
      .maybeSingle()
      .then(({ data }) => setBillingCustomer(data ?? null));
  }, [isPro]);

  useEffect(() => {
    if (checkoutReturn !== "waiting") return;
    const finish = (outcome: "done" | "timed_out") => {
      const url = new URL(window.location.href);
      url.searchParams.delete("session_id");
      window.history.replaceState(null, "", url.toString());
      setCheckoutReturn(outcome === "done" ? null : "timed_out");
    };
    if (isPro) {
      finish("done");
      return;
    }
    let polls = 0;
    const timer = window.setInterval(() => {
      polls += 1;
      void reloadSubscription().then((latest) => {
        if (isProSubscription(latest)) return; // the isPro change finishes
        if (polls >= CHECKOUT_RETURN_MAX_POLLS) {
          window.clearInterval(timer);
          finish("timed_out");
        }
      });
    }, CHECKOUT_RETURN_POLL_MS);
    return () => window.clearInterval(timer);
  }, [checkoutReturn, isPro, reloadSubscription]);

  const onCheckoutError = useCallback((message: string) => {
    setCheckoutOpen(false);
    setError(message);
  }, []);

  const openPortal = async () => {
    setPortalPending(true);
    setError(null);
    try {
      window.location.href = await fetchBillingPortalUrl();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not open billing management");
      setPortalPending(false);
    }
  };

  const status = subscription?.status;
  const periodEnd = subscription?.current_period_end;

  return (
    <section aria-labelledby="subscription-heading">
      <h2 id="subscription-heading" style={styles.sectionTitle}>
        Personal
      </h2>
      <div style={styles.card}>
        {checkoutReturn === "waiting" && (
          <div role="status" style={styles.notice}>
            Payment received. Activating Pro…
          </div>
        )}
        {checkoutReturn === "timed_out" && (
          <div role="status" style={styles.notice}>
            Payment received. Pro will appear here within a few minutes.
          </div>
        )}
        <Field label="Current Plan">
          <span style={isPro ? styles.proBadge : styles.freeBadge}>
            {isPro ? "Pro" : "Free"}
          </span>
        </Field>
        {isPro && status && (
          <Field label="Status">
            <span
              style={{
                color:
                  status === "active" || status === "trialing"
                    ? "#10b981"
                    : "#f59e0b",
              }}
            >
              {subscriptionStatusLabel(status)}
            </span>
          </Field>
        )}
        {isPro && status && periodEnd && (
          <Field label={subscriptionPeriodLabel(status)}>
            {formatPeriodEnd(periodEnd)}
          </Field>
        )}
        {isPro && status === "past_due" && (
          <p style={styles.billingNote}>
            Your last payment failed. Update your card in Manage billing to keep
            Pro.
          </p>
        )}
        {isPro && status === "canceled" && periodEnd && (
          <p style={styles.billingNote}>
            Your subscription is set to cancel. Pro stays on until{" "}
            {formatPeriodEnd(periodEnd)}.
          </p>
        )}
        {isPro && billingCustomer === null && (
          <p style={styles.billingNote}>
            Your Pro comes from a team. Its owners manage billing in the Teams
            section below.
          </p>
        )}

        <div style={styles.billingActions}>
          {!isPro && !checkoutOpen && (
            <div>
              <span
                title={!checkoutAvailable ? "Coming Soon" : undefined}
                style={
                  !checkoutAvailable
                    ? { display: "inline-block", cursor: "not-allowed" }
                    : undefined
                }
              >
                <button
                  type="button"
                  onClick={() => {
                    setError(null);
                    setCheckoutOpen(true);
                  }}
                  disabled={!checkoutAvailable}
                  style={{
                    ...styles.primaryButton,
                    ...(!checkoutAvailable ? styles.primaryButtonDisabled : {}),
                  }}
                >
                  <CreditCardIcon />
                  Upgrade to Pro
                </button>
              </span>
              {!checkoutAvailable && (
                <div style={styles.comingSoonSubtext}>Coming Soon</div>
              )}
            </div>
          )}
          {billingCustomer && (
            <button
              type="button"
              onClick={openPortal}
              disabled={portalPending}
              style={{ ...styles.secondaryButton, marginLeft: "auto" }}
            >
              Manage billing
            </button>
          )}
        </div>

        {error && (
          <div role="alert" style={styles.errorText}>
            {error}
          </div>
        )}

        {checkoutOpen && <CheckoutRedirect onError={onCheckoutError} />}
      </div>
    </section>
  );
}

// ── Team tab ─────────────────────────────────────────────────────────────────
//
// Organizations and Team (prds/billing-and-teams.md, "Organizations and
// seats"). Reads go through RLS. Every change goes through the organizations
// Edge Function, whose SQL enforces the rules; this tab only shows them.

function roleLabel(role: OrgMembership["role"] | undefined): string {
  if (role === "owner") return "Owner";
  if (role === "admin") return "Admin";
  return "Member";
}

interface OrgMembership {
  org_id: string;
  user_id: string;
  role: "owner" | "admin" | "member";
  created_at: string;
  organizations: { id: string; name: string } | null;
}

interface MemberProfile {
  id: string;
  email: string | null;
  full_name: string | null;
}

interface PendingInvite {
  id: string;
  org_id: string;
  email: string;
  expires_at: string;
}

interface TeamSubscription {
  owner_id: string;
  status: string;
  current_period_end: string | null;
  cancel_at_period_end: boolean;
}

interface Installation {
  id: number;
  account_login: string;
  account_type: string;
  organization_id: string | null;
  linked_user_id: string | null;
}

interface TeamData {
  memberships: OrgMembership[];
  profiles: Map<string, MemberProfile>;
  invites: PendingInvite[];
  subscriptions: TeamSubscription[];
  billingOrgIds: Set<string>;
  installations: Installation[];
}

async function loadTeamData(): Promise<TeamData> {
  const { data: membershipRows } = await supabase
    .from("organization_members")
    .select("org_id, user_id, role, created_at, organizations(id, name)")
    .order("created_at");
  const memberships = (membershipRows ?? []) as unknown as OrgMembership[];
  const userIds = [...new Set(memberships.map((m) => m.user_id))];
  const [profiles, invites, subscriptions, customers, installations] =
    await Promise.all([
      userIds.length > 0
        ? supabase.from("profiles").select("id, email, full_name").in("id", userIds)
        : Promise.resolve({ data: [] }),
      supabase
        .from("organization_invites")
        .select("id, org_id, email, expires_at")
        .is("accepted_at", null)
        .is("revoked_at", null)
        .gt("expires_at", new Date().toISOString())
        .order("created_at"),
      supabase
        .from("billing_subscriptions")
        .select("owner_id, status, current_period_end, cancel_at_period_end")
        .eq("owner_type", "organization")
        .in("status", ["trialing", "active", "past_due"]),
      supabase.from("billing_customers").select("owner_id").eq("owner_type", "organization"),
      supabase
        .from("github_app_installations")
        .select("id, account_login, account_type, organization_id, linked_user_id")
        .order("account_login"),
    ]);
  return {
    memberships,
    profiles: new Map(
      ((profiles.data ?? []) as MemberProfile[]).map((p) => [p.id, p]),
    ),
    invites: (invites.data ?? []) as PendingInvite[],
    subscriptions: (subscriptions.data ?? []) as TeamSubscription[],
    billingOrgIds: new Set(
      ((customers.data ?? []) as { owner_id: string }[]).map((c) => c.owner_id),
    ),
    installations: (installations.data ?? []) as Installation[],
  };
}

async function organizationsRequest<T = Record<string, unknown>>(
  body: OrganizationAction,
): Promise<T> {
  const { data, error } = await supabase.functions.invoke("organizations", { body });
  if (error) {
    throw new Error(await functionErrorMessage(error, "Something went wrong. Try again."));
  }
  return data as T;
}

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : "Something went wrong. Try again.";
}

function memberName(profile: MemberProfile | undefined): string {
  return profile?.full_name || profile?.email || "Unknown member";
}

function OrganizationCard({
  org,
  userId,
  data,
  checkoutAvailable,
  onChanged,
}: {
  org: { id: string; name: string };
  userId: string;
  data: TeamData;
  checkoutAvailable: boolean;
  onChanged: () => Promise<unknown>;
}) {
  const members = data.memberships.filter((m) => m.org_id === org.id);
  const myRole = members.find((m) => m.user_id === userId)?.role;
  const isOwner = myRole === "owner";
  // Owners and admins manage the team's billing account and its invites.
  const isManager = isOwner || myRole === "admin";
  const ownerCount = members.filter((m) => m.role === "owner").length;
  const invites = data.invites.filter((i) => i.org_id === org.id);
  const subscription = data.subscriptions.find((s) => s.owner_id === org.id);
  const attached = data.installations.filter((i) => i.organization_id === org.id);
  const attachable = data.installations.filter(
    (i) => i.organization_id === null && i.linked_user_id === userId,
  );

  const [inviteEmail, setInviteEmail] = useState("");
  // Accept links are shown once: the server stores only a hash of the token.
  const [links, setLinks] = useState<Record<string, string>>({});
  const [copied, setCopied] = useState<string | null>(null);
  const [checkoutOpen, setCheckoutOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (work: () => Promise<unknown>) => {
    setPending(true);
    setError(null);
    try {
      await work();
      await onChanged();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  const invite = (email: string) =>
    run(async () => {
      const result = await organizationsRequest<{
        invite: { id: string };
        accept_url: string;
      }>({ action: "invite", organization_id: org.id, email });
      setLinks((current) => ({ ...current, [result.invite.id]: result.accept_url }));
      setInviteEmail("");
    });

  const copyLink = async (inviteId: string) => {
    try {
      await navigator.clipboard.writeText(links[inviteId]);
      setCopied(inviteId);
    } catch {
      setError("Could not copy. Select the link and copy it yourself.");
    }
  };

  const openPortal = () =>
    run(async () => {
      window.location.href = await fetchBillingPortalUrl({ organization_id: org.id });
    });

  const onCheckoutError = useCallback((message: string) => {
    setCheckoutOpen(false);
    setError(message);
  }, []);

  return (
    <section aria-label={org.name} style={styles.orgCard}>
      <div style={styles.orgName}>{org.name}</div>
      <Field label="Your role">{roleLabel(myRole)}</Field>
      <Field label="Plan">
        {subscription ? (
          <span>
            <span style={styles.proBadge}>Team</span>{" "}
            {subscriptionStatusLabel(
              subscription.cancel_at_period_end ? "canceled" : subscription.status,
            )}
          </span>
        ) : (
          "No Team subscription"
        )}
      </Field>
      {subscription?.current_period_end && (
        <Field
          label={subscription.cancel_at_period_end ? "Team until" : "Renews on"}
        >
          {formatPeriodEnd(subscription.current_period_end)}
        </Field>
      )}
      {isManager ? (
        <Field label="Seats">
          {members.length} of {TEAM_INCLUDED_SEATS} included
          {extraSeats(members.length) > 0 &&
            `, ${extraSeats(members.length)} extra at US$${TEAM_EXTRA_SEAT_USD} per month each`}
        </Field>
      ) : (
        <Field label="Members">{members.length}</Field>
      )}

      {isManager && (
        <div style={styles.billingActions}>
          {!subscription && !checkoutOpen && (
            <div>
              <span
                title={!checkoutAvailable ? "Coming Soon" : undefined}
                style={
                  !checkoutAvailable
                    ? { display: "inline-block", cursor: "not-allowed" }
                    : undefined
                }
              >
                <button
                  type="button"
                  onClick={() => {
                    setError(null);
                    setCheckoutOpen(true);
                  }}
                  disabled={!checkoutAvailable}
                  style={{
                    ...styles.primaryButton,
                    ...(!checkoutAvailable ? styles.primaryButtonDisabled : {}),
                  }}
                >
                  Buy Team
                </button>
              </span>
              <div style={styles.comingSoonSubtext}>
                {checkoutAvailable
                  ? `US$199 per month for ${TEAM_INCLUDED_SEATS} members, then US$${TEAM_EXTRA_SEAT_USD} per extra member. Every member gets Pro.`
                  : "Coming Soon"}
              </div>
            </div>
          )}
          {data.billingOrgIds.has(org.id) && (
            <button
              type="button"
              onClick={openPortal}
              disabled={pending}
              style={{ ...styles.secondaryButton, marginLeft: "auto" }}
            >
              Manage billing
            </button>
          )}
        </div>
      )}
      {checkoutOpen && (
        <CheckoutRedirect organizationId={org.id} onError={onCheckoutError} />
      )}

      {error && (
        <div role="alert" style={styles.errorText}>
          {error}
        </div>
      )}

      <div style={styles.teamSubheading}>Members</div>
      <ul aria-label="Members" style={styles.teamList}>
        {members.map((m) => {
          const name = memberName(data.profiles.get(m.user_id));
          return (
            <li key={m.user_id} className="dashboard-row" style={styles.teamRow}>
              <span>
                {name}
                {m.user_id === userId ? " (you)" : ""}
                <span style={styles.teamMuted}>
                  {" "}
                  {data.profiles.get(m.user_id)?.email ?? ""}
                </span>
              </span>
              <span className="dashboard-row-actions" style={styles.teamRowActions}>
                <span style={styles.teamMuted}>
                  {roleLabel(m.role)}
                </span>
                {isManager && m.user_id !== userId && m.role !== "owner" && (
                  <button
                    type="button"
                    aria-label={
                      m.role === "admin"
                        ? `Remove admin from ${name}`
                        : `Make ${name} an admin`
                    }
                    disabled={pending}
                    className="dashboard-link-button" style={styles.linkButton}
                    onClick={() =>
                      void run(() =>
                        organizationsRequest({
                          action: "set_role",
                          organization_id: org.id,
                          user_id: m.user_id,
                          role: m.role === "admin" ? "member" : "admin",
                        }),
                      )
                    }
                  >
                    {m.role === "admin" ? "Remove admin" : "Make admin"}
                  </button>
                )}
                {isOwner && m.user_id !== userId && m.role !== "owner" && (
                  <button
                    type="button"
                    aria-label={`Make ${name} an owner`}
                    disabled={pending}
                    className="dashboard-link-button" style={styles.linkButton}
                    onClick={() => {
                      if (
                        window.confirm(
                          `Make ${name} an owner of ${org.name}? Owners invite and remove members, manage billing, and can delete the organization.`,
                        )
                      ) {
                        void run(() =>
                          organizationsRequest({
                            action: "promote_member",
                            organization_id: org.id,
                            user_id: m.user_id,
                          }),
                        );
                      }
                    }}
                  >
                    Make owner
                  </button>
                )}
                {isOwner && m.user_id !== userId && m.role === "owner" && (
                  <button
                    type="button"
                    aria-label={`Make ${name} a member`}
                    disabled={pending}
                    className="dashboard-link-button" style={styles.linkButton}
                    onClick={() => {
                      if (window.confirm(`Make ${name} a member of ${org.name} instead of an owner?`)) {
                        void run(() =>
                          organizationsRequest({
                            action: "demote_owner",
                            organization_id: org.id,
                            user_id: m.user_id,
                          }),
                        );
                      }
                    }}
                  >
                    Make member
                  </button>
                )}
                {isOwner && m.user_id !== userId && (
                  <button
                    type="button"
                    aria-label={`Remove ${name}`}
                    disabled={pending}
                    className="dashboard-link-button" style={styles.linkButton}
                    onClick={() => {
                      if (
                        window.confirm(
                          `Remove ${name} from ${org.name}? They lose Team at once.`,
                        )
                      ) {
                        void run(() =>
                          organizationsRequest({
                            action: "remove_member",
                            organization_id: org.id,
                            user_id: m.user_id,
                          }),
                        );
                      }
                    }}
                  >
                    Remove
                  </button>
                )}
              </span>
            </li>
          );
        })}
      </ul>

      {isManager && (
        <>
          <div style={styles.teamSubheading}>Pending invites</div>
          {invites.length === 0 ? (
            <p style={styles.teamMuted}>No pending invites.</p>
          ) : (
            <ul aria-label="Pending invites" style={styles.teamList}>
              {invites.map((i) => (
                <li key={i.id} style={styles.teamInvite}>
                  <div style={styles.teamRow}>
                    <span style={styles.inviteEmail}>
                      <span>{i.email}</span>
                      <span style={styles.inviteExpiry}>
                        Expires {shortDate(i.expires_at)}
                      </span>
                    </span>
                    <span className="dashboard-row-actions" style={styles.teamRowActions}>
                      {!links[i.id] && (
                        <button
                          type="button"
                          aria-label={`New link for ${i.email}`}
                          disabled={pending}
                          className="dashboard-link-button" style={styles.linkButton}
                          onClick={() => void invite(i.email)}
                        >
                          New link
                        </button>
                      )}
                      <span className="dashboard-tooltip-anchor">
                      <button
                        type="button"
                        aria-label={`Revoke invite for ${i.email}`}
                        aria-describedby={`revoke-tip-${i.id}`}
                        disabled={pending}
                        style={styles.iconButton}
                        onClick={() =>
                          void run(() =>
                            organizationsRequest({
                              action: "revoke_invite",
                              invite_id: i.id,
                            }),
                          )
                        }
                      >
                        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                          <path d="M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4M6.75 6.5v4.5M9.25 6.5v4.5" />
                        </svg>
                      </button>
                      <span
                        role="tooltip"
                        id={`revoke-tip-${i.id}`}
                        className="dashboard-tooltip"
                      >
                        Revoke invite
                      </span>
                      </span>
                    </span>
                  </div>
                  {links[i.id] && (
                    <div className="dashboard-form" style={styles.teamLinkRow}>
                      <input
                        readOnly
                        aria-label={`Invite link for ${i.email}`}
                        value={links[i.id]}
                        style={styles.input}
                        onFocus={(e) => e.currentTarget.select()}
                      />
                      <button
                        type="button"
                        style={styles.secondaryButton}
                        onClick={() => void copyLink(i.id)}
                      >
                        {copied === i.id ? "Copied" : "Copy link"}
                      </button>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
          <form
            className="dashboard-form" style={styles.teamForm}
            onSubmit={(e) => {
              e.preventDefault();
              void invite(inviteEmail);
            }}
          >
            <input
              type="email"
              required
              aria-label="Email to invite"
              placeholder="teammate@example.com"
              value={inviteEmail}
              onChange={(e) => setInviteEmail(e.target.value)}
              style={styles.input}
            />
            <button type="submit" disabled={pending} style={styles.saveButton}>
              Generate invite link
            </button>
          </form>
          <p style={styles.inputHint}>
            Enter your teammate's email, generate a link, then copy it and
            send it to them. A link works once, for any Treq account, and
            expires after 24 hours. New link replaces the old one. Team includes{" "}
            {TEAM_INCLUDED_SEATS} members. Each member who joins beyond that
            adds US${TEAM_EXTRA_SEAT_USD} per month, prorated.
          </p>
        </>
      )}

      <div style={styles.teamSubheading}>GitHub App installations</div>
      {attached.length === 0 ? (
        <p style={styles.teamMuted}>No installation belongs to {org.name}.</p>
      ) : (
        <ul aria-label="GitHub App installations" style={styles.teamList}>
          {attached.map((i) => (
            <li key={i.id} className="dashboard-row" style={styles.teamRow}>
              <span style={styles.repoName}>{i.account_login}</span>
              <span style={styles.teamMuted}>{i.account_type}</span>
            </li>
          ))}
        </ul>
      )}
      {isOwner && attachable.length > 0 && (
        <div>
          <p style={styles.inputHint}>
            Attach an installation you linked. Members can then see its
            repositories, it runs on this organization&apos;s Team, and only owners of {org.name} can relink it.
          </p>
          <div style={styles.billingActions}>
            {attachable.map((i) => (
              <button
                key={i.id}
                type="button"
                disabled={pending}
                style={styles.secondaryButton}
                onClick={() =>
                  void run(() =>
                    organizationsRequest({
                      action: "attach_installation",
                      organization_id: org.id,
                      installation_id: i.id,
                    }),
                  )
                }
              >
                Attach {i.account_login}
              </button>
            ))}
          </div>
        </div>
      )}

      {isOwner && (
        <p style={styles.inputHint}>
          {ownerCount > 1
            ? "To transfer ownership, make another member an owner, then leave."
            : "You are the only owner. To transfer ownership, make another member an owner, then leave. To close the organization instead, delete it."}
        </p>
      )}
      <div style={styles.billingActions}>
        <button
          type="button"
          aria-label={`Leave ${org.name}`}
          disabled={pending}
          style={styles.dangerButton}
          onClick={() => {
            if (window.confirm(`Leave ${org.name}? You lose its Team at once.`)) {
              void run(() =>
                organizationsRequest({ action: "leave", organization_id: org.id }),
              );
            }
          }}
        >
          Leave
        </button>
        {isOwner && (
          <button
            type="button"
            aria-label={`Delete ${org.name}`}
            disabled={pending}
            style={styles.dangerButton}
            onClick={() => {
              if (
                window.confirm(
                  `Delete ${org.name}? Every member loses access at once, pending invites stop working, and its GitHub App installations become yours. This cannot be undone.`,
                )
              ) {
                void run(() =>
                  organizationsRequest({
                    action: "delete_organization",
                    organization_id: org.id,
                  }),
                );
              }
            }}
          >
            Delete organization
          </button>
        )}
      </div>
    </section>
  );
}

// Free and Pro users see this in place of the create form. Get Team creates
// the organization, then opens Team checkout for it, so nobody is left with
// an organization that has no Team.
function GetTeamCard({
  checkoutAvailable,
  onCreated,
}: {
  checkoutAvailable: boolean;
  onCreated: () => Promise<unknown>;
}) {
  const [name, setName] = useState("");
  const [orgId, setOrgId] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onCheckoutError = useCallback((message: string) => {
    setError(message);
  }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!checkoutAvailable) return;
    setPending(true);
    setError(null);
    try {
      const created = await organizationsRequest<{ organization: { id: string } }>({
        action: "create",
        name,
      });
      setOrgId(created.organization.id);
      await onCreated();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <section aria-label="Get Team" style={styles.orgCard}>
      <div style={styles.orgName}>Get Team</div>
      <p style={styles.billingNote}>
        Every member gets Pro, with shared billing and a shared GitHub App
        installation. US$199 per month for {TEAM_INCLUDED_SEATS} members, then
        US${TEAM_EXTRA_SEAT_USD} per extra member.
      </p>
      {orgId ? (
        <CheckoutRedirect organizationId={orgId} onError={onCheckoutError} />
      ) : (
        <form className="dashboard-form" style={styles.teamForm} onSubmit={submit}>
          <input
            required
            maxLength={100}
            aria-label="Organization name"
            placeholder="Acme"
            value={name}
            disabled={!checkoutAvailable}
            onChange={(e) => setName(e.target.value)}
            style={styles.input}
          />
          <button
            type="submit"
            disabled={!checkoutAvailable || pending}
            style={{
              ...styles.saveButton,
              ...(!checkoutAvailable ? styles.primaryButtonDisabled : {}),
            }}
          >
            <CreditCardIcon />
            Start a team
          </button>
        </form>
      )}
      {!checkoutAvailable && <div style={styles.comingSoonSubtext}>Coming Soon</div>}
      {error && (
        <div role="alert" style={styles.errorText}>
          {error}
        </div>
      )}
    </section>
  );
}

// "Nov 14, 2026": pending invites show a short expiry date.
function shortDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  });
}

function clearSearchParams(...names: string[]) {
  const url = new URL(window.location.href);
  for (const name of names) url.searchParams.delete(name);
  window.history.replaceState(null, "", url.toString());
}

function TeamTab({
  userId,
  reloadSubscription,
  stripePaymentsEnabled,
}: {
  userId: string;
  reloadSubscription: () => Promise<Subscription | null>;
  stripePaymentsEnabled: boolean;
}) {
  const checkoutAvailable = stripePaymentsEnabled && STRIPE_PUBLISHABLE_KEY !== "";
  const [data, setData] = useState<TeamData | null>(null);
  // From the invite link's #invite= fragment, or stashed before sign-in.
  const [inviteToken, setInviteToken] = useState<string | null>(() =>
    pendingInviteToken(invitePage()),
  );
  const [orgName, setOrgName] = useState("");
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checkoutReturn, setCheckoutReturn] = useState<"waiting" | "timed_out" | null>(
    () => (new URLSearchParams(window.location.search).has("session_id") ? "waiting" : null),
  );

  const reload = useCallback(async () => {
    const latest = await loadTeamData();
    setData(latest);
    return latest;
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Once read, the token leaves the address bar and the stash.
  useEffect(() => {
    forgetPendingInvite(invitePage());
  }, []);

  // After embedded Checkout returns, Team appears once the stripe-webhook
  // function has recorded the subscription. Poll until then.
  useEffect(() => {
    if (checkoutReturn !== "waiting") return;
    let polls = 0;
    let stopped = false;
    const ownsTeam = (latest: TeamData) =>
      latest.memberships.some(
        (m) =>
          m.user_id === userId &&
          (m.role === "owner" || m.role === "admin") &&
          latest.subscriptions.some((s) => s.owner_id === m.org_id),
      );
    const finish = (outcome: "timed_out" | null) => {
      stopped = true;
      clearSearchParams("session_id");
      setCheckoutReturn(outcome);
      if (outcome === null) void reloadSubscription();
    };
    const timer = window.setInterval(() => {
      if (stopped) return;
      polls += 1;
      void reload().then((latest) => {
        if (stopped) return;
        if (ownsTeam(latest)) finish(null);
        else if (polls >= CHECKOUT_RETURN_MAX_POLLS) finish("timed_out");
      });
    }, CHECKOUT_RETURN_POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [checkoutReturn, reload, reloadSubscription, userId]);

  const run = async (work: () => Promise<string>) => {
    setPending(true);
    setError(null);
    setNotice(null);
    try {
      setNotice(await work());
      await reload();
      void reloadSubscription();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  const accept = (token: string) =>
    run(async () => {
      const joined = await organizationsRequest<{
        organization: { name: string };
        result: string;
      }>({ action: "accept", token });
      setInviteToken(null);
      return joined.result === "already_member"
        ? `You are already a member of ${joined.organization.name}.`
        : `You joined ${joined.organization.name}. Every member of a team with Team has Pro.`;
    });

  // Free and Pro users get Team by creating an organization and paying in
  // one step. Members of a team that already has Team create more directly.
  const hasTeam = (data?.memberships ?? []).some(
    (m) =>
      m.user_id === userId &&
      (data?.subscriptions ?? []).some((s) => s.owner_id === m.org_id),
  );
  const orgs = new Map<string, { id: string; name: string }>();
  for (const m of data?.memberships ?? []) {
    if (m.user_id === userId && m.organizations) orgs.set(m.org_id, m.organizations);
  }

  return (
    <section aria-labelledby="team-heading">
      <h2 id="team-heading" style={styles.sectionTitle}>
        Teams
      </h2>

      {checkoutReturn === "waiting" && (
        <div role="status" style={styles.notice}>
          Payment received. Activating Team…
        </div>
      )}
      {checkoutReturn === "timed_out" && (
        <div role="status" style={styles.notice}>
          Payment received. Team will appear here within a few minutes.
        </div>
      )}
      {notice && (
        <div role="status" style={styles.notice}>
          {notice}
        </div>
      )}
      {error && (
        <div role="alert" style={{ ...styles.errorText, marginBottom: "0.75rem" }}>
          {error}
        </div>
      )}

      {inviteToken && (
        <div style={styles.orgCard}>
          <div style={styles.orgName}>You have an invite</div>
          <p style={styles.billingNote}>
            Accepting adds this account to the team. If the team has Team, you
            get Pro right away.
          </p>
          <div style={styles.billingActions}>
            <button
              type="button"
              disabled={pending}
              style={styles.primaryButton}
              onClick={() => void accept(inviteToken)}
            >
              Accept invite
            </button>
            <button
              type="button"
              style={styles.secondaryButton}
              onClick={() => setInviteToken(null)}
            >
              Not now
            </button>
          </div>
        </div>
      )}

      {data === null ? (
        <div style={styles.loadingText}>Loading…</div>
      ) : (
        [...orgs.values()].map((org) => (
          <OrganizationCard
            key={org.id}
            org={org}
            userId={userId}
            data={data}
            checkoutAvailable={checkoutAvailable}
            onChanged={async () => {
              await reload();
              void reloadSubscription();
            }}
          />
        ))
      )}

      {data !== null &&
        (hasTeam ? (
          <div style={styles.orgCard}>
            <div style={styles.orgName}>Create an organization</div>
            <p style={styles.billingNote}>
              An organization holds a Team subscription: US$199 per month for{" "}
              {TEAM_INCLUDED_SEATS} members, then US${TEAM_EXTRA_SEAT_USD} per extra
              member, each with everything in Pro. It can also own
              a shared GitHub App installation.
            </p>
            <form
              className="dashboard-form" style={styles.teamForm}
              onSubmit={(e) => {
                e.preventDefault();
                void run(async () => {
                  const created = await organizationsRequest<{
                    organization: { name: string };
                  }>({ action: "create", name: orgName });
                  setOrgName("");
                  return `Created ${created.organization.name}.`;
                });
              }}
            >
              <input
                required
                maxLength={100}
                aria-label="Organization name"
                placeholder="Acme"
                value={orgName}
                onChange={(e) => setOrgName(e.target.value)}
                style={styles.input}
              />
              <button type="submit" disabled={pending} style={styles.saveButton}>
                Create organization
              </button>
            </form>
          </div>
        ) : (
          <GetTeamCard
            checkoutAvailable={checkoutAvailable}
            onCreated={async () => {
              await reload();
            }}
          />
        ))}
    </section>
  );
}

// ── Delete account ───────────────────────────────────────────────────────────
//
// The delete-account Edge Function cancels the subscription, destroys the
// Treq Cloud machines and deletes the account. The user types their email
// first. A refusal (the user is the last owner of an organization) shows the
// server's sentence.

function DeleteAccountSection({ email }: { email: string }) {
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const confirmable = !pending && confirmsAccountDeletion(typed, email);

  const close = () => {
    setOpen(false);
    setTyped("");
    setError(null);
  };

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!confirmable) return;
    setPending(true);
    setError(null);
    try {
      await deleteAccount();
      await supabase.auth.signOut({ scope: "local" });
      window.location.href = "/";
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Your account could not be deleted.",
      );
      setPending(false);
    }
  };

  return (
    <section aria-labelledby="delete-account-heading" style={styles.dangerSection}>
      <h2 id="delete-account-heading" style={styles.sectionTitle}>
        Delete account
      </h2>
      <div style={{ ...styles.card, ...styles.dangerCard }}>
        <p style={styles.billingNote}>
          Your Pro subscription is cancelled at once, without a refund, and
          your Treq Cloud machines and their disks are deleted. Billing records
          are kept. Repositories and workspaces on your computer are not
          touched. If you are the last owner of an organization, make another
          member an owner or delete the organization first.
        </p>
        {!open ? (
          <div style={styles.billingActions}>
            <button
              type="button"
              onClick={() => setOpen(true)}
              style={styles.dangerButton}
            >
              Delete account…
            </button>
          </div>
        ) : (
          <form onSubmit={submit}>
            <label htmlFor="delete-account-email" style={styles.dangerLabel}>
              Type {email} to confirm
            </label>
            <input
              id="delete-account-email"
              type="email"
              autoComplete="off"
              value={typed}
              disabled={pending}
              placeholder={email}
              onChange={(event) => {
                setTyped(event.target.value);
                setError(null);
              }}
              style={styles.input}
            />
            <div style={styles.billingActions}>
              <button
                type="button"
                onClick={close}
                disabled={pending}
                style={styles.secondaryButton}
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={!confirmable}
                style={{
                  ...styles.dangerButton,
                  ...(!confirmable ? styles.primaryButtonDisabled : {}),
                }}
              >
                {pending ? "Deleting…" : "Delete account"}
              </button>
            </div>
          </form>
        )}
        {error && (
          <div role="alert" style={styles.errorText}>
            {error}
          </div>
        )}
      </div>
    </section>
  );
}

// ── More menu (phones) ───────────────────────────────────────────────────────
//
// Under 768px the sidebar is a top bar with no room for Sign Out, so it moves
// behind a ⋮ button. CSS shows this menu or the plain button, never both.

function MoreMenu({ onSignOut }: { onSignOut: () => void }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const onPointer = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onPointer);
    };
  }, [open]);

  return (
    <div ref={root} className="dashboard-more">
      <button
        type="button"
        aria-label="More options"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        style={styles.moreButton}
      >
        <svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
          <circle cx="8" cy="3" r="1.5" />
          <circle cx="8" cy="8" r="1.5" />
          <circle cx="8" cy="13" r="1.5" />
        </svg>
      </button>
      {open && (
        <div role="menu" style={styles.moreMenu}>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              onSignOut();
            }}
            style={styles.moreMenuItem}
          >
            Sign out
          </button>
        </div>
      )}
    </div>
  );
}

// ── Dashboard shell ──────────────────────────────────────────────────────────

function DashboardContent() {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as
    | { pro?: boolean; stripePayments?: boolean }
    | undefined;
  const stripePaymentsEnabled = Boolean(flags?.stripePayments);

  const [user, setUser] = useState<User | null>(null);
  const [session, setSession] = useState<Session | null>(null);
  const [loading, setLoading] = useState(true);
  const [subscription, setSubscription] = useState<Subscription | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>(() =>
    typeof window !== "undefined" ? tabFromSearch(window.location.search) : "account",
  );

  useEffect(() => {
    // Sign-in brings the user back here, so an invite link opened while
    // signed out still works. The redirect keeps the path and query; the
    // invite token in the fragment waits in sessionStorage.
    const signIn = () => {
      stashInviteForSignIn(invitePage());
      const here = `${window.location.pathname}${window.location.search}`;
      window.location.href = `/sign-in?redirect=${encodeURIComponent(here)}`;
    };
    supabase.auth.getSession().then(({ data: { session: s } }) => {
      if (!s) {
        signIn();
        return;
      }
      setSession(s);
      setUser(s.user);
      setLoading(false);
    });

    const {
      data: { subscription: authSub },
    } = supabase.auth.onAuthStateChange((_event, s) => {
      setSession(s);
      setUser(s?.user ?? null);
      if (!s) signIn();
    });

    return () => authSub.unsubscribe();
  }, []);

  const loadSubscription = useCallback(async () => {
    const { data } = await supabase.from("subscriptions").select("*").single();
    const latest = (data as Subscription | null) ?? null;
    if (latest) setSubscription(latest);
    return latest;
  }, []);

  useEffect(() => {
    if (!session) return;
    void loadSubscription();
  }, [session, loadSubscription]);

  const handleSignOut = async () => {
    await supabase.auth.signOut();
    window.location.href = "/";
  };

  if (loading) {
    return (
      <div style={styles.centerContainer}>
        <div style={styles.spinner} />
        <p>Loading…</p>
      </div>
    );
  }

  if (!user) return null;

  const avatarUrl = user.user_metadata?.avatar_url || user.user_metadata?.picture;
  const fullName =
    user.user_metadata?.full_name || user.user_metadata?.name || "User";
  return (
    <div className="dashboard-page" style={styles.container}>
      <div className="dashboard-sidebar" style={styles.sidebar}>
        <div className="dashboard-sidebar-header" style={styles.sidebarHeader}>
          {avatarUrl ? (
            <img src={avatarUrl} alt={fullName} style={styles.avatar} />
          ) : (
            <div style={styles.avatarPlaceholder}>{fullName[0]}</div>
          )}
          <div>
            <div style={styles.userName}>{fullName}</div>
            <div style={styles.userEmail}>{user.email}</div>
          </div>
        </div>

        <nav aria-label="Dashboard" className="dashboard-nav" style={styles.nav}>
          {(
            [
              ["account", "Account"],
              ["integrations", "Integrations"],
            ] as const
          ).map(([tab, label]) => (
            <button
              key={tab}
              type="button"
              aria-current={activeTab === tab ? "page" : undefined}
              onClick={() => setActiveTab(tab)}
              className="dashboard-nav-item"
              style={{
                ...styles.navItem,
                ...(activeTab === tab ? styles.navItemActive : {}),
              }}
            >
              {label}
            </button>
          ))}
        </nav>

        <button type="button" onClick={handleSignOut} className="dashboard-signout" style={styles.signOutButton}>
          Sign Out
        </button>

        <MoreMenu onSignOut={handleSignOut} />
      </div>

      <div className="dashboard-content" style={styles.content}>
        {activeTab === "account" && (
          <>
            <h1 style={styles.pageTitle}>Account</h1>
            <SubscriptionTab
              subscription={subscription}
              reloadSubscription={loadSubscription}
              stripePaymentsEnabled={stripePaymentsEnabled}
            />
            <div style={styles.accountSection}>
              <TeamTab
                userId={user.id}
                reloadSubscription={loadSubscription}
                stripePaymentsEnabled={stripePaymentsEnabled}
              />
            </div>
            <DeleteAccountSection email={user.email ?? ""} />
          </>
        )}

        {activeTab === "integrations" && (
          <IntegrationsTab onUpgrade={() => setActiveTab("account")} />
        )}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  centerContainer: {
    display: "flex",
    flexDirection: "column",
    justifyContent: "center",
    alignItems: "center",
    minHeight: "60vh",
  },
  spinner: {
    width: "32px",
    height: "32px",
    border: "3px solid var(--ifm-color-emphasis-200)",
    borderTopColor: "var(--ifm-color-primary)",
    borderRadius: "50%",
    animation: "spin 0.8s linear infinite",
    marginBottom: "1rem",
  },
  container: {
    display: "flex",
    width: "100%",
    maxWidth: "960px",
    margin: "0 auto",
    padding: "2rem 1rem",
    gap: "2rem",
    alignItems: "stretch",
    boxSizing: "border-box",
  },
  sidebar: {
    width: "240px",
    flexShrink: 0,
    display: "flex",
    flexDirection: "column",
  },
  sidebarHeader: {
    display: "flex",
    alignItems: "center",
    gap: "0.75rem",
    marginBottom: "1.5rem",
    paddingBottom: "1rem",
    borderBottom: "1px solid var(--ifm-color-emphasis-200)",
  },
  avatar: { width: "40px", height: "40px", borderRadius: "50%" },
  avatarPlaceholder: {
    width: "40px",
    height: "40px",
    borderRadius: "50%",
    backgroundColor: "var(--ifm-color-primary)",
    color: "#fff",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    fontWeight: 600,
    fontSize: "1.1rem",
  },
  userName: { fontWeight: 600, fontSize: "0.9rem" },
  userEmail: { fontSize: "0.75rem", color: "var(--ifm-color-emphasis-500)" },
  nav: { display: "flex", flexDirection: "column", gap: "0.25rem" },
  navItem: {
    display: "block",
    width: "100%",
    textAlign: "left",
    padding: "0.5rem 0.75rem",
    borderRadius: "6px",
    border: "none",
    backgroundColor: "transparent",
    color: "var(--ifm-font-color-base)",
    fontSize: "0.9rem",
    cursor: "pointer",
    transition: "all 0.15s",
  },
  navItemActive: {
    backgroundColor: "var(--ifm-color-emphasis-100)",
    fontWeight: 600,
  },
  signOutButton: {
    display: "block",
    width: "100%",
    textAlign: "left",
    padding: "0.5rem 0.75rem",
    borderRadius: "6px",
    border: "none",
    backgroundColor: "transparent",
    color: "#ef4444",
    fontSize: "0.9rem",
    cursor: "pointer",
    marginTop: "auto",
  },
  moreButton: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    width: "44px",
    height: "44px",
    borderRadius: "6px",
    border: "none",
    backgroundColor: "transparent",
    color: "var(--ifm-font-color-base)",
    cursor: "pointer",
  },
  moreMenu: {
    position: "absolute",
    right: 0,
    top: "calc(100% + 4px)",
    minWidth: "10rem",
    padding: "0.25rem",
    borderRadius: "8px",
    border: "1px solid var(--ifm-color-emphasis-200)",
    backgroundColor: "var(--ifm-background-color)",
    boxShadow: "0 8px 24px rgba(0,0,0,0.12)",
    zIndex: 10,
  },
  moreMenuItem: {
    display: "block",
    width: "100%",
    minHeight: "44px",
    padding: "0 0.75rem",
    textAlign: "left",
    border: "none",
    borderRadius: "6px",
    backgroundColor: "transparent",
    color: "#ef4444",
    fontSize: "0.9rem",
    cursor: "pointer",
  },
  content: { flex: 1, minWidth: 0, overflowWrap: "anywhere" },
  pageTitle: { fontSize: "1.5rem", fontWeight: 700, marginBottom: "1.25rem" },
  accountSection: { marginTop: "2.5rem" },
  sectionTitle: { fontSize: "1.25rem", fontWeight: 700, marginBottom: "1rem" },
  card: {
    padding: "1.5rem",
    borderRadius: "10px",
    border: "1px solid var(--ifm-color-emphasis-200)",
    backgroundColor: "var(--ifm-background-color)",
  },
  field: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    padding: "0.75rem 0",
    borderBottom: "1px solid var(--ifm-color-emphasis-100)",
  },
  fieldLabel: {
    fontSize: "0.85rem",
    color: "var(--ifm-color-emphasis-600)",
    fontWeight: 500,
  },
  fieldValue: { fontSize: "0.9rem", fontWeight: 500 },
  proBadge: {
    padding: "0.2rem 0.6rem",
    borderRadius: "12px",
    backgroundColor: "rgba(16,185,129,0.1)",
    color: "#10b981",
    fontSize: "0.8rem",
    fontWeight: 600,
  },
  freeBadge: {
    padding: "0.2rem 0.6rem",
    borderRadius: "12px",
    backgroundColor: "var(--ifm-color-emphasis-100)",
    color: "var(--ifm-color-emphasis-600)",
    fontSize: "0.8rem",
    fontWeight: 600,
  },
  primaryButton: {
    display: "inline-block",
    padding: "0.6rem 1.5rem",
    borderRadius: "8px",
    border: "none",
    background:
      "linear-gradient(135deg, var(--ifm-color-primary) 0%, var(--ifm-color-primary-dark) 100%)",
    color: "#fff",
    fontSize: "0.9rem",
    fontWeight: 600,
    cursor: "pointer",
    transition: "all 0.2s",
    textDecoration: "none",
  },
  primaryButtonDisabled: {
    opacity: 0.45,
    cursor: "not-allowed",
  },
  secondaryButton: {
    whiteSpace: "nowrap",
    display: "inline-block",
    padding: "0.6rem 1.25rem",
    borderRadius: "8px",
    border: "1px solid var(--ifm-color-emphasis-300)",
    backgroundColor: "transparent",
    color: "var(--ifm-font-color-base)",
    fontSize: "0.9rem",
    fontWeight: 600,
    cursor: "pointer",
  },
  billingActions: {
    display: "flex",
    alignItems: "flex-start",
    gap: "0.75rem",
    flexWrap: "wrap",
    marginTop: "1.5rem",
  },
  billingNote: {
    margin: "0.75rem 0 0",
    fontSize: "0.85rem",
    color: "var(--ifm-color-emphasis-700)",
  },
  notice: {
    marginBottom: "0.75rem",
    padding: "0.6rem 0.75rem",
    borderRadius: "6px",
    backgroundColor: "rgba(16,185,129,0.1)",
    color: "#059669",
    fontSize: "0.85rem",
  },
  errorText: { color: "#ef4444", fontSize: "0.85rem", marginTop: "0.75rem" },
  dangerSection: { marginTop: "2.5rem" },
  dangerCard: { borderColor: "rgba(239,68,68,0.4)" },
  dangerLabel: { display: "block", fontSize: "0.85rem", marginBottom: "0.4rem" },
  comingSoonSubtext: {
    marginTop: "0.5rem",
    fontSize: "0.8rem",
    color: "var(--ifm-color-emphasis-500)",
  },
  // Integrations tab
  integrationHeader: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "1rem",
  },
  integrationInfo: {
    display: "flex",
    alignItems: "flex-start",
    gap: "0.75rem",
    flex: 1,
    minWidth: 0,
  },
  integrationActions: { flexShrink: 0 },
  integrationIcon: {
    width: "36px",
    height: "36px",
    borderRadius: "8px",
    backgroundColor: "var(--ifm-color-emphasis-100)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  integrationName: { fontWeight: 600, fontSize: "0.9rem", marginBottom: "0.15rem" },
  integrationDesc: { fontSize: "0.8rem", color: "var(--ifm-color-emphasis-500)" },
  integrationFeatureList: {
    margin: "0.4rem 0 0",
    paddingLeft: "1.1rem",
    fontSize: "0.8rem",
    color: "var(--ifm-color-emphasis-600)",
    lineHeight: 1.55,
  },
  featureProBadge: {
    display: "inline-block",
    marginLeft: "0.25rem",
    padding: "0.05rem 0.35rem",
    borderRadius: "4px",
    backgroundColor: "rgba(16,185,129,0.12)",
    color: "#10b981",
    fontSize: "0.7rem",
    fontWeight: 700,
    letterSpacing: "0.04em",
    verticalAlign: "middle",
  },
  connectedBadge: {
    padding: "0.2rem 0.6rem",
    borderRadius: "12px",
    backgroundColor: "rgba(16,185,129,0.1)",
    color: "#10b981",
    fontSize: "0.8rem",
    fontWeight: 600,
  },
  repoSection: { marginTop: "1.25rem" },
  repoSectionHeader: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: "0.75rem",
  },
  repoSectionTitle: { fontSize: "0.85rem", fontWeight: 600 },
  manageLink: {
    fontSize: "0.8rem",
    color: "var(--ifm-color-primary)",
    textDecoration: "none",
  },
  loadingText: {
    fontSize: "0.85rem",
    color: "var(--ifm-color-emphasis-500)",
    padding: "0.5rem 0",
  },
  // Repo config panel
  repoCard: {
    border: "1px solid var(--ifm-color-emphasis-200)",
    borderRadius: "8px",
    marginBottom: "0.5rem",
    overflow: "hidden",
  },
  repoHeader: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    padding: "0.75rem 1rem",
    cursor: "pointer",
    userSelect: "none",
  },
  repoMeta: { display: "flex", alignItems: "center", gap: "0.5rem" },
  repoName: { fontSize: "0.875rem", fontWeight: 600, fontFamily: "monospace" },
  privateBadge: {
    fontSize: "0.75rem",
    padding: "0.1rem 0.4rem",
    borderRadius: "4px",
    backgroundColor: "var(--ifm-color-emphasis-100)",
    color: "var(--ifm-color-emphasis-600)",
  },
  input: {
    width: "100%",
    padding: "0.4rem 0.6rem",
    borderRadius: "6px",
    border: "1px solid var(--ifm-color-emphasis-200)",
    backgroundColor: "var(--ifm-background-color)",
    color: "var(--ifm-font-color-base)",
    fontSize: "0.85rem",
    outline: "none",
  },
  inputHint: {
    fontSize: "0.8rem",
    color: "var(--ifm-color-emphasis-600)",
    marginTop: "0.2rem",
  },
  // Team tab
  orgCard: {
    padding: "1.5rem",
    borderRadius: "10px",
    border: "1px solid var(--ifm-color-emphasis-200)",
    backgroundColor: "var(--ifm-background-color)",
    marginBottom: "1rem",
  },
  orgName: { fontWeight: 700, fontSize: "1rem", marginBottom: "0.25rem" },
  teamSubheading: {
    marginTop: "1.25rem",
    marginBottom: "0.4rem",
    fontSize: "0.78rem",
    fontWeight: 600,
    color: "var(--ifm-color-emphasis-600)",
    textTransform: "uppercase",
    letterSpacing: "0.03em",
  },
  teamList: { listStyle: "none", margin: 0, padding: 0 },
  teamRow: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    gap: "0.75rem",
    padding: "0.4rem 0",
    fontSize: "0.85rem",
  },
  teamRowActions: { display: "flex", alignItems: "center", gap: "0.75rem", flexWrap: "wrap" },
  teamInvite: { borderBottom: "1px solid var(--ifm-color-emphasis-100)" },
  teamLinkRow: { display: "flex", gap: "0.5rem", paddingBottom: "0.6rem" },
  teamMuted: { fontSize: "0.8rem", color: "var(--ifm-color-emphasis-500)" },
  teamForm: { display: "flex", gap: "0.5rem", marginTop: "0.75rem" },
  inviteEmail: { flex: 1, minWidth: 0, display: "flex", flexDirection: "column" },
  inviteExpiry: { fontSize: "0.8rem", color: "var(--ifm-color-emphasis-600)" },
  iconButton: {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
    width: "32px",
    height: "32px",
    padding: 0,
    borderRadius: "6px",
    border: "none",
    backgroundColor: "transparent",
    color: "#ef4444",
    cursor: "pointer",
  },
  linkButton: {
    border: "none",
    background: "none",
    padding: 0,
    color: "var(--ifm-color-primary)",
    fontSize: "0.8rem",
    cursor: "pointer",
  },
  dangerButton: {
    padding: "0.45rem 1rem",
    borderRadius: "6px",
    border: "1px solid rgba(239,68,68,0.4)",
    backgroundColor: "transparent",
    color: "#ef4444",
    fontSize: "0.85rem",
    fontWeight: 600,
    cursor: "pointer",
  },
  saveButton: {
    whiteSpace: "nowrap",
    padding: "0.45rem 1.2rem",
    borderRadius: "6px",
    border: "none",
    background:
      "linear-gradient(135deg, var(--ifm-color-primary) 0%, var(--ifm-color-primary-dark) 100%)",
    color: "#fff",
    fontSize: "0.85rem",
    fontWeight: 600,
    cursor: "pointer",
  },
};

export default function DashboardPage(): React.ReactNode {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as
    | { pro?: boolean; stripePayments?: boolean }
    | undefined;

  if (!flags?.pro) {
    return (
      <BrowserOnly>
        {() => {
          window.location.href = "/";
          return null;
        }}
      </BrowserOnly>
    );
  }

  return (
    <Layout title="Dashboard" description="Manage your Treq account">
      <Head>
        <meta name="robots" content="noindex, nofollow" />
      </Head>
      <BrowserOnly>{() => <DashboardContent />}</BrowserOnly>
    </Layout>
  );
}
