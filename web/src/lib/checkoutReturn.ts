// Stripe returns to /dashboard?tab=<subscription|team>&session_id=… (see
// checkoutReturnUrl in supabase/functions/_shared/billing/checkout.ts). The
// tab says which plan was bought, so only that section shows the notice.
export function checkoutReturnPlan(search: string): 'pro' | 'team' | null {
  const params = new URLSearchParams(search);
  if (!params.has('session_id')) return null;
  switch (params.get('tab')) {
    case 'subscription':
      return 'pro';
    case 'team':
      return 'team';
    default:
      return null;
  }
}
