-- Subscriptions the reconcile job re-reads from Stripe: still entitling, but
-- their period ended more than a day ago, so a renewal or end event should
-- have arrived. Service role only.
create function public.billing_stale_subscriptions(p_limit integer)
returns setof text
language sql
stable
security definer
set search_path = ''
as $$
  select s.stripe_subscription_id
  from public.billing_subscriptions s
  where s.status in ('trialing', 'active', 'past_due')
    and s.current_period_end < now() - interval '1 day'
  order by s.current_period_end
  limit greatest(p_limit, 0);
$$;

revoke all on function public.billing_stale_subscriptions(integer)
  from public, anon, authenticated;
grant execute on function public.billing_stale_subscriptions(integer) to service_role;
