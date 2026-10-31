-- billing_record_subscription_event reports 'duplicate_subscription' when an
-- owner has more than one live subscription of the same plan, so the
-- webhook alerts on a double charge. The write itself is unchanged. Built on
-- 042_same_second_events.sql, whose same-second tie check it keeps.
--
-- Returns 'applied', 'duplicate', 'stale', 'tie' or 'duplicate_subscription'.

create or replace function public.billing_record_subscription_event(
  p_event_id text,
  p_event_type text,
  p_event_created timestamptz,
  p_subscription_id text,
  p_customer_id text,
  p_plan text,
  p_status text,
  p_current_period_end timestamptz,
  p_cancel_at_period_end boolean,
  p_trial_end timestamptz
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

  -- Stripe's event `created` has one-second resolution. Two events from the
  -- same second that disagree cannot be ordered, so neither is trusted: the
  -- event is not recorded and the webhook writes a fresh read from Stripe.
  -- A deletion is final whatever its order, so it is never a tie.
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
    status, current_period_end, cancel_at_period_end, trial_end,
    stripe_event_created
  )
  values (
    p_subscription_id, p_customer_id, v_owner_type, v_owner_id, p_plan,
    p_status, p_current_period_end, coalesce(p_cancel_at_period_end, false),
    p_trial_end, p_event_created
  )
  on conflict (stripe_subscription_id) do update set
    owner_type = coalesce(excluded.owner_type, s.owner_type),
    owner_id = coalesce(excluded.owner_id, s.owner_id),
    plan = excluded.plan,
    status = excluded.status,
    current_period_end = excluded.current_period_end,
    cancel_at_period_end = excluded.cancel_at_period_end,
    trial_end = excluded.trial_end,
    stripe_event_created = excluded.stripe_event_created,
    updated_at = now()
  -- Stripe never reactivates an ended subscription, so a late event cannot
  -- bring one back.
  where s.stripe_event_created <= excluded.stripe_event_created
    and s.status not in ('canceled', 'incomplete_expired');
  get diagnostics v_written = row_count;

  -- Any event that shows a Pro trial proves the trial was used, even a stale
  -- one.
  if p_plan = 'pro' and p_trial_end is not null then
    update public.billing_customers
    set trial_used_at = now(), updated_at = now()
    where stripe_customer_id = p_customer_id
      and trial_used_at is null;
  end if;

  -- Two live subscriptions of one plan for one owner mean a double charge
  -- (two checkouts completed before either webhook landed). Both are
  -- recorded, because Stripe bills both; the result makes the webhook alert.
  if v_written > 0
    and v_owner_id is not null
    and (
      select count(*)
      from public.billing_subscriptions s
      where s.owner_type = v_owner_type
        and s.owner_id = v_owner_id
        and s.plan = p_plan
        and s.status in ('trialing', 'active', 'past_due', 'incomplete', 'unpaid', 'paused')
    ) > 1 then
    return 'duplicate_subscription';
  end if;

  return case when v_written = 0 then 'stale' else 'applied' end;
end;
$$;
