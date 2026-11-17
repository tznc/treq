-- Entitlement no longer depends only on the webhook ending a subscription.
-- A subscription stops entitling 30 days after its current_period_end, so a
-- lost customer.subscription.deleted event (or a past_due subscription Stripe
-- never moves on) cannot grant Pro or Team indefinitely. A null period end
-- keeps entitling, as before.

create function treq_internal.subscription_entitles(
  p_status text,
  p_current_period_end timestamptz
)
returns boolean
language sql
stable
set search_path = ''
as $$
  select p_status in ('trialing', 'active', 'past_due')
    and (p_current_period_end is null
      or p_current_period_end > now() - interval '30 days');
$$;

revoke all on function treq_internal.subscription_entitles(text, timestamptz)
  from public, anon, authenticated, service_role;

create or replace function treq_internal.organization_has_team(p_org_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1
    from public.billing_subscriptions s
    where s.owner_type = 'organization'
      and s.owner_id = p_org_id
      and s.plan = 'team'
      and treq_internal.subscription_entitles(s.status, s.current_period_end)
  );
$$;

create or replace function treq_internal.user_has_pro(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1
    from public.billing_subscriptions s
    where s.owner_type = 'user'
      and s.owner_id = p_user_id
      and s.plan = 'pro'
      and treq_internal.subscription_entitles(s.status, s.current_period_end)
  )
  or exists (
    select 1
    from public.organization_members m
    where m.user_id = p_user_id
      and treq_internal.organization_has_team(m.org_id)
  );
$$;

create or replace function public.get_current_user_subscription()
returns table (
  plan text,
  status text,
  current_period_end timestamp
)
language sql
stable
security definer
set search_path = ''
as $$
  with entitled_sub as (
    select s.status, s.current_period_end, s.cancel_at_period_end
    from public.billing_subscriptions s
    where treq_internal.subscription_entitles(s.status, s.current_period_end)
      and (
        (s.owner_type = 'user' and s.owner_id = (select auth.uid()) and s.plan = 'pro')
        or (
          s.owner_type = 'organization'
          and s.plan = 'team'
          and exists (
            select 1 from public.organization_members m
            where m.org_id = s.owner_id and m.user_id = (select auth.uid())
          )
        )
      )
    order by s.cancel_at_period_end, s.current_period_end desc nulls last
    limit 1
  )
  select
    case when public.has_pro((select auth.uid())) then 'pro' else 'free' end as plan,
    case
      when e.status is null then 'inactive'
      when e.cancel_at_period_end then 'canceled'
      else e.status
    end as status,
    (e.current_period_end at time zone 'utc') as current_period_end
  from (select 1) as _
  left join entitled_sub e on true;
$$;
