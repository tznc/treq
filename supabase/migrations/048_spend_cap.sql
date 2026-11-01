-- Spend cap and in-app notifications.
--
-- A user's cap defaults to US$50 (minimum US$5, maximum the
-- `treq.spend_cap_max_usd` setting, default US$1000). Spend is what treq
-- bills in the user's window: their live Pro billing period from its first
-- full hour, else the UTC calendar month. The 80% and 100% notices are
-- stored once per (user, kind, period).

-- ── Stripe billing period ─────────────────────────────────────────────

alter table public.billing_subscriptions
  add column current_period_start timestamptz;

drop function public.billing_record_subscription_event(
  text, text, timestamptz, text, text, text, text, timestamptz, boolean, timestamptz);

-- 042's function plus p_current_period_start.
create function public.billing_record_subscription_event(
  p_event_id text,
  p_event_type text,
  p_event_created timestamptz,
  p_subscription_id text,
  p_customer_id text,
  p_plan text,
  p_status text,
  p_current_period_end timestamptz,
  p_cancel_at_period_end boolean,
  p_trial_end timestamptz,
  p_current_period_start timestamptz default null
)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_owner_type text;
  v_owner_id uuid;
  v_written int;
begin
  if p_event_type not in (
    'customer.subscription.created',
    'customer.subscription.updated',
    'customer.subscription.deleted'
  ) then
    raise exception 'unsupported subscription event type %', p_event_type
      using errcode = 'invalid_parameter_value';
  end if;

  -- 042's same-second tie: two events from one second that disagree cannot
  -- be ordered, so the webhook writes a fresh read from Stripe instead.
  if p_event_type <> 'customer.subscription.deleted' and exists (
    select 1
    from public.billing_subscriptions s
    where s.stripe_subscription_id = p_subscription_id
      and s.stripe_event_created = p_event_created
      and s.status not in ('canceled', 'incomplete_expired')
      and (
        s.plan is distinct from p_plan
        or s.status is distinct from p_status
        or s.current_period_end is distinct from p_current_period_end
        or s.cancel_at_period_end is distinct from coalesce(p_cancel_at_period_end, false)
        or s.trial_end is distinct from p_trial_end
      )
  ) and not exists (
    select 1 from public.billing_events e where e.stripe_event_id = p_event_id
  ) then
    return 'tie';
  end if;

  insert into public.billing_events (stripe_event_id, type)
  values (p_event_id, p_event_type)
  on conflict (stripe_event_id) do nothing;
  if not found then
    return 'duplicate';
  end if;

  select c.owner_type, c.owner_id
    into v_owner_type, v_owner_id
  from public.billing_customers c
  where c.stripe_customer_id = p_customer_id;

  insert into public.billing_subscriptions as s (
    stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan,
    status, current_period_start, current_period_end, cancel_at_period_end, trial_end,
    stripe_event_created
  )
  values (
    p_subscription_id, p_customer_id, v_owner_type, v_owner_id, p_plan,
    p_status, p_current_period_start, p_current_period_end, coalesce(p_cancel_at_period_end, false),
    p_trial_end, p_event_created
  )
  on conflict (stripe_subscription_id) do update set
    owner_type = coalesce(excluded.owner_type, s.owner_type),
    owner_id = coalesce(excluded.owner_id, s.owner_id),
    plan = excluded.plan,
    status = excluded.status,
    current_period_start = excluded.current_period_start,
    current_period_end = excluded.current_period_end,
    cancel_at_period_end = excluded.cancel_at_period_end,
    trial_end = excluded.trial_end,
    stripe_event_created = excluded.stripe_event_created,
    updated_at = now()
  where s.stripe_event_created <= excluded.stripe_event_created
    and s.status not in ('canceled', 'incomplete_expired');
  get diagnostics v_written = row_count;

  if p_plan = 'pro' and p_trial_end is not null then
    update public.billing_customers
    set trial_used_at = now(), updated_at = now()
    where stripe_customer_id = p_customer_id
      and trial_used_at is null;
  end if;

  return case when v_written = 0 then 'stale' else 'applied' end;
end;
$$;

revoke all on function public.billing_record_subscription_event(
  text, text, timestamptz, text, text, text, text, timestamptz, boolean, timestamptz, timestamptz)
  from public, anon, authenticated;
grant execute on function public.billing_record_subscription_event(
  text, text, timestamptz, text, text, text, text, timestamptz, boolean, timestamptz, timestamptz)
  to service_role;

-- 042's snapshot write plus p_current_period_start.
drop function public.billing_record_subscription_snapshot(
  text, text, text, text, timestamptz, boolean, timestamptz, timestamptz);

create function public.billing_record_subscription_snapshot(
  p_subscription_id text,
  p_customer_id text,
  p_plan text,
  p_status text,
  p_current_period_end timestamptz,
  p_cancel_at_period_end boolean,
  p_trial_end timestamptz,
  p_fetched_at timestamptz,
  p_current_period_start timestamptz default null
)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_owner_type text;
  v_owner_id uuid;
  v_written int;
begin
  select c.owner_type, c.owner_id
    into v_owner_type, v_owner_id
  from public.billing_customers c
  where c.stripe_customer_id = p_customer_id;

  insert into public.billing_subscriptions as s (
    stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan,
    status, current_period_start, current_period_end, cancel_at_period_end, trial_end,
    stripe_event_created
  )
  values (
    p_subscription_id, p_customer_id, v_owner_type, v_owner_id, p_plan,
    p_status, p_current_period_start, p_current_period_end, coalesce(p_cancel_at_period_end, false),
    p_trial_end, p_fetched_at
  )
  on conflict (stripe_subscription_id) do update set
    owner_type = coalesce(excluded.owner_type, s.owner_type),
    owner_id = coalesce(excluded.owner_id, s.owner_id),
    plan = excluded.plan,
    status = excluded.status,
    current_period_start = excluded.current_period_start,
    current_period_end = excluded.current_period_end,
    cancel_at_period_end = excluded.cancel_at_period_end,
    trial_end = excluded.trial_end,
    stripe_event_created = excluded.stripe_event_created,
    updated_at = now()
  where s.stripe_event_created <= excluded.stripe_event_created
    and s.status not in ('canceled', 'incomplete_expired');
  get diagnostics v_written = row_count;

  return case when v_written = 0 then 'stale' else 'applied' end;
end;
$$;
revoke all on function public.billing_record_subscription_snapshot(
  text, text, text, text, timestamptz, boolean, timestamptz, timestamptz, timestamptz)
  from public, anon, authenticated;
grant execute on function public.billing_record_subscription_snapshot(
  text, text, text, text, timestamptz, boolean, timestamptz, timestamptz, timestamptz)
  to service_role;

-- ── Tables ────────────────────────────────────────────────────────────

create table public.spend_caps (
  user_id uuid primary key references auth.users (id) on delete cascade,
  cap_usd numeric(10, 2) not null default 50 check (cap_usd >= 5),
  updated_at timestamptz not null default now()
);
alter table public.spend_caps enable row level security;
revoke all on public.spend_caps from public, anon, authenticated;

create table public.user_notifications (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null references auth.users (id) on delete cascade,
  kind text not null,
  period text not null,
  title text not null,
  body text not null,
  created_at timestamptz not null default now(),
  dismissed_at timestamptz,
  unique (user_id, kind, period)
);
alter table public.user_notifications enable row level security;
revoke all on public.user_notifications from public, anon, authenticated;
grant select on public.user_notifications to authenticated;
create policy user_notifications_owner_read on public.user_notifications
  for select to authenticated using (user_id = (select auth.uid()));

-- ── Spend ─────────────────────────────────────────────────────────────

create function treq_internal.spend_cap_max_usd()
returns numeric
language sql
stable
set search_path = ''
as $$
  select coalesce(nullif(current_setting('treq.spend_cap_max_usd', true), '')::numeric, 1000);
$$;

-- Rounded up to the hour: meter events are stamped with their hour, so
-- Stripe puts a partial first hour on the previous invoice.
create function treq_internal.spend_window_start(p_user_id uuid, p_now timestamptz)
returns timestamptz
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (select date_trunc('hour', s.current_period_start + interval '1 hour' - interval '1 microsecond')
       from public.billing_subscriptions s
      where s.owner_type = 'user' and s.owner_id = p_user_id
        and s.plan = 'pro'
        and s.status in ('trialing', 'active', 'past_due')
        and s.current_period_start <= p_now
        and s.current_period_end > p_now
      order by s.current_period_start desc
      limit 1),
    date_trunc('month', p_now, 'UTC'));
$$;

-- Billed spend in the window: finished usage_reports plus the hours not
-- yet reported, priced from the ledger.
create function treq_internal.month_spend_nanousd(p_user_id uuid, p_now timestamptz)
returns bigint
language sql
stable
security definer
set search_path = ''
as $$
  with bounds as (
    select treq_internal.spend_window_start(p_user_id, p_now) as month_start
  ),
  reported as (
    select coalesce(sum(r.billed_nanousd), 0) as billed,
           max(r.hour) + interval '1 hour' as reported_until
      from public.usage_reports r, bounds b
     where r.owner_user_id = p_user_id
       and r.hour >= b.month_start and r.hour < p_now
  ),
  open as (
    select coalesce(sum(treq_internal.billed_nanousd(
             extract(epoch from
               least(coalesce(u.ended_at, p_now), p_now)
               - greatest(u.started_at, greatest(b.month_start, coalesce(rp.reported_until, b.month_start))))
             * case u.kind
                 when 'running' then p.running_nanousd_per_sec
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end,
             p.markup_bps, u.billing_mode)), 0) as billed
      from bounds b
     cross join reported rp
      join public.usage_intervals u
        on u.owner_user_id = p_user_id
       and u.started_at < p_now
       and coalesce(u.ended_at, p_now) > greatest(b.month_start, coalesce(rp.reported_until, b.month_start))
      join public.machine_type_prices p on p.id = u.price_id
  )
  select (round((select billed from reported) + (select billed from open)))::bigint;
$$;

create function treq_internal.spend_status(p_user_id uuid, p_now timestamptz)
returns jsonb
language sql
stable
security definer
set search_path = ''
as $$
  with s as (
    select coalesce((select c.cap_usd from public.spend_caps c where c.user_id = p_user_id), 50)::numeric as cap,
           round(treq_internal.month_spend_nanousd(p_user_id, p_now) / 1e9, 6) as spent
  )
  select jsonb_build_object(
    'cap_usd', s.cap,
    'spent_usd', s.spent,
    'month_start', treq_internal.spend_window_start(p_user_id, p_now),
    'state', case when s.spent >= s.cap then 'capped'
                  when s.spent >= s.cap * 0.8 then 'warning'
                  else 'ok' end)
    from s;
$$;

create function public.get_spend_status()
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  if (select auth.uid()) is null then
    raise exception 'not_authenticated' using errcode = '42501';
  end if;
  return treq_internal.spend_status((select auth.uid()), now());
end;
$$;

create function public.set_spend_cap(p_cap_usd numeric)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_user uuid := (select auth.uid());
begin
  if v_user is null then
    raise exception 'not_authenticated' using errcode = '42501';
  end if;
  if p_cap_usd is null or p_cap_usd < 5 then
    raise exception 'spend_cap_too_low';
  end if;
  if p_cap_usd > treq_internal.spend_cap_max_usd() then
    raise exception 'spend_cap_too_high';
  end if;
  insert into public.spend_caps (user_id, cap_usd, updated_at)
  values (v_user, round(p_cap_usd, 2), now())
  on conflict (user_id) do update set cap_usd = excluded.cap_usd, updated_at = excluded.updated_at;
  return treq_internal.spend_status(v_user, now());
end;
$$;

create function public.get_spend_status_for(p_user_id uuid)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return treq_internal.spend_status(p_user_id, now());
end;
$$;

-- Users at 80% of their cap or more. A Stripe period is at most 31 days, so
-- anyone with usage in the last 32 days is checked against their own window.
create function public.spend_cap_candidates(p_now timestamptz)
returns table (user_id uuid, cap_usd numeric, spent_usd numeric, month_start timestamptz, state text)
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return query
  select o.owner_user_id,
         (st->>'cap_usd')::numeric, (st->>'spent_usd')::numeric,
         (st->>'month_start')::timestamptz, st->>'state'
    from (
      select distinct u.owner_user_id
        from public.usage_intervals u
       where coalesce(u.ended_at, p_now) >= least(date_trunc('month', p_now, 'UTC'), p_now - interval '32 days')
    ) o
   cross join lateral treq_internal.spend_status(o.owner_user_id, p_now) st
   where st->>'state' <> 'ok';
end;
$$;

-- ── Notifications ─────────────────────────────────────────────────────

-- True when stored, false when this (user, kind, period) already exists.
create function public.record_user_notification(
  p_user_id uuid, p_kind text, p_period text, p_title text, p_body text)
returns boolean
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_count int;
begin
  perform treq_internal.require_service_role();
  insert into public.user_notifications (user_id, kind, period, title, body)
  values (p_user_id, p_kind, p_period, p_title, p_body)
  on conflict (user_id, kind, period) do nothing;
  get diagnostics v_count = row_count;
  return v_count > 0;
end;
$$;

create function public.list_notifications()
returns table (id uuid, kind text, period text, title text, body text, created_at timestamptz)
language sql
stable
security definer
set search_path = ''
as $$
  select n.id, n.kind, n.period, n.title, n.body, n.created_at
    from public.user_notifications n
   where n.user_id = (select auth.uid()) and n.dismissed_at is null
   order by n.created_at desc
   limit 50;
$$;

create function public.dismiss_notification(p_id uuid)
returns void
language sql
security definer
set search_path = ''
as $$
  update public.user_notifications
     set dismissed_at = now()
   where id = p_id and user_id = (select auth.uid()) and dismissed_at is null;
$$;

revoke all on function treq_internal.spend_cap_max_usd() from public, anon, authenticated;
revoke all on function treq_internal.spend_window_start(uuid, timestamptz) from public, anon, authenticated;
revoke all on function treq_internal.month_spend_nanousd(uuid, timestamptz) from public, anon, authenticated;
revoke all on function treq_internal.spend_status(uuid, timestamptz) from public, anon, authenticated;
revoke all on function public.get_spend_status() from public, anon;
revoke all on function public.set_spend_cap(numeric) from public, anon;
revoke all on function public.list_notifications() from public, anon;
revoke all on function public.dismiss_notification(uuid) from public, anon;
grant execute on function public.get_spend_status() to authenticated;
grant execute on function public.set_spend_cap(numeric) to authenticated;
grant execute on function public.list_notifications() to authenticated;
grant execute on function public.dismiss_notification(uuid) to authenticated;
revoke all on function public.get_spend_status_for(uuid) from public, anon, authenticated;
revoke all on function public.spend_cap_candidates(timestamptz) from public, anon, authenticated;
revoke all on function public.record_user_notification(uuid, text, text, text, text) from public, anon, authenticated;
grant execute on function public.get_spend_status_for(uuid) to service_role;
grant execute on function public.spend_cap_candidates(timestamptz) to service_role;
grant execute on function public.record_user_notification(uuid, text, text, text, text) to service_role;
