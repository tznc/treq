// Display-side reading of `public.subscriptions` (prds/billing-and-teams.md,
// "Clients"). The server decides entitlement with has_pro. This helper only
// decides what the dashboard shows. Keep it in sync with
// src/lib/subscription.ts in the desktop app.

export interface Subscription {
  plan: string;
  status: string;
  current_period_end: string | null;
}

/**
 * Statuses that still carry Pro. `canceled` here means the subscription is
 * set to cancel at the period end: a subscription that has ended reads
 * plan `free`.
 */
const PRO_STATUSES: ReadonlySet<string> = new Set([
  "trialing",
  "active",
  "past_due",
  "canceled",
]);

export function isProSubscription(
  subscription: { plan: string; status: string } | null | undefined,
): boolean {
  return subscription?.plan === "pro" && PRO_STATUSES.has(subscription.status);
}

const STATUS_LABELS: Readonly<Record<string, string>> = {
  trialing: "Trial",
  active: "Active",
  past_due: "Past due",
  canceled: "Canceling",
  inactive: "Inactive",
};

export function subscriptionStatusLabel(status: string): string {
  return STATUS_LABELS[status] ?? status;
}

/** What `current_period_end` means for this status. */
export function subscriptionPeriodLabel(status: string): string {
  switch (status) {
    case "trialing":
      return "Trial ends";
    case "active":
    case "past_due":
      return "Renews on";
    case "canceled":
      return "Pro until";
    default:
      return "Period ends";
  }
}

/**
 * `current_period_end` is a UTC timestamp without a zone suffix. Read it as
 * UTC so the date shown is the right local date.
 */
export function formatPeriodEnd(value: string): string {
  const hasZone = /(?:Z|[+-]\d{2}:?\d{2})$/.test(value);
  return new Date(hasZone ? value : `${value}Z`).toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}
