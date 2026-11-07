-- Volume pricing. The markup falls as usage in a billing period grows,
-- graduated (each slice of usage pays its own tier's rate):
--
--   sandbox-months this period   managed markup   BYOS fee
--   0 - 1                         20%              5%
--   1 - 2                         18%              4%
--   2 +                           16%              3%
--
-- A sandbox-month is 730 hours of a Medium (4 vCPU) sandbox, counted in
-- vCPU-seconds (10,512,000), so a larger machine moves through the tiers
-- faster. Stopped disk adds no vCPU-seconds. Managed and BYOS usage count
-- separately. The tiers apply to whoever receives the invoice: the user, or
-- the organization for usage on an organization profile. The period follows
-- that owner's subscription (current_period_start), else the calendar month.
--
-- Rates are multipliers on the base rate (machine_type_prices.markup_bps and
-- byos_markup_bps()), so changing a base rate keeps the curve.

alter table public.usage_reports
  add column managed_vcpu_seconds bigint not null default 0,
  add column byos_vcpu_seconds bigint not null default 0;

-- Base rate times the tier multipliers, averaged over the slice of usage
-- [p_prior, p_prior + p_units) in vCPU-seconds. An empty slice pays the rate
-- of the tier it sits in.
create function treq_internal.volume_rate_bps(
  p_billing_mode text, p_base_bps int, p_prior numeric, p_units numeric)
returns numeric
language plpgsql
immutable
set search_path = ''
as $$
declare
  v_tier constant numeric := 10512000;
  v_mult numeric[] := case when p_billing_mode = 'byos'
                           then array[1, 0.8, 0.6] else array[1, 0.9, 0.8] end;
  v_lo numeric[] := array[0, v_tier, 2 * v_tier];
  v_hi numeric[] := array[v_tier, 2 * v_tier, 'Infinity'::numeric];
  v_prior numeric := greatest(coalesce(p_prior, 0), 0);
  v_units numeric := greatest(coalesce(p_units, 0), 0);
  v_sum numeric := 0;
begin
  if v_units = 0 then
    for i in 1..3 loop
      if v_prior < v_hi[i] then
        return p_base_bps * v_mult[i];
      end if;
    end loop;
  end if;
  for i in 1..3 loop
    v_sum := v_sum + greatest(least(v_prior + v_units, v_hi[i]) - greatest(v_prior, v_lo[i]), 0) * v_mult[i];
  end loop;
  return round(p_base_bps * v_sum / v_units, 6);
end;
$$;

-- Start of the billing period that holds p_hour for this invoice owner.
create function treq_internal.usage_period_start(p_user_id uuid, p_org_id uuid, p_hour timestamptz)
returns timestamptz
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  v_anchor timestamptz;
  v_n int := 0;
begin
  select s.current_period_start into v_anchor
    from public.billing_subscriptions s
   where s.current_period_start is not null
     and ((p_org_id is null and s.owner_type = 'user' and s.owner_id = p_user_id)
       or (p_org_id is not null and s.owner_type = 'organization' and s.owner_id = p_org_id))
   order by s.current_period_start desc
   limit 1;
  if v_anchor is null then
    return date_trunc('month', p_hour);
  end if;
  while v_anchor + make_interval(months => v_n) > p_hour loop
    v_n := v_n - 1;
  end loop;
  while v_anchor + make_interval(months => v_n + 1) <= p_hour loop
    v_n := v_n + 1;
  end loop;
  return v_anchor + make_interval(months => v_n);
end;
$$;

-- Shared by build_usage_reports and build_final_usage_reports: one row per
-- (owner, billing org, hour) before p_cutoff, oldest first, so each row's
-- tier sees the rows before it.
create function treq_internal.write_usage_reports(p_cutoff timestamptz, p_user_id uuid)
returns int
language plpgsql
security definer
set search_path = ''
as $$
declare
  r record;
  v_count int := 0;
  v_start timestamptz;
  v_prior_m numeric;
  v_prior_b numeric;
  v_managed numeric;
  v_byos numeric;
begin
  for r in
    select b.owner_user_id::text || coalesce(':' || b.billing_org_id::text, '') || ':' || to_char(b.hour at time zone 'UTC', 'YYYY-MM-DD"T"HH24') as identifier,
           b.owner_user_id, b.billing_org_id, b.hour,
           sum(b.seconds) as seconds,
           sum(b.cost) as cost,
           sum(case when b.billing_mode = 'managed' then b.cost else 0 end) as cost_m,
           sum(case when b.billing_mode = 'managed' then b.cost * b.markup_bps else 0 end) as markup_m,
           sum(case when b.billing_mode = 'byos' then b.cost else 0 end) as cost_b,
           sum(case when b.billing_mode = 'managed' then b.vcpu_seconds else 0 end) as vcpu_m,
           sum(case when b.billing_mode = 'byos' then b.vcpu_seconds else 0 end) as vcpu_b
      from (
        select u.owner_user_id, u.billing_org_id, h.hour, p.markup_bps, u.billing_mode,
               s.seconds,
               s.seconds * case u.kind
                             when 'running' then p.running_nanousd_per_sec
                             else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
                           end as cost,
               case u.kind when 'running' then s.seconds * mt.vcpu else 0 end as vcpu_seconds
          from public.usage_intervals u
          join public.machine_type_prices p on p.id = u.price_id
          join public.provider_machine_types mt on mt.id = p.machine_type_id
          cross join lateral generate_series(
            date_trunc('hour', u.started_at),
            least(coalesce(u.ended_at, p_cutoff), p_cutoff) - interval '1 microsecond',
            interval '1 hour') as h(hour)
          cross join lateral (select extract(epoch from
              least(coalesce(u.ended_at, p_cutoff), h.hour + interval '1 hour', p_cutoff)
              - greatest(u.started_at, h.hour)) as seconds) s
         where u.started_at < p_cutoff
           and (p_user_id is null or u.owner_user_id = p_user_id)
      ) b
     where b.hour < p_cutoff and b.seconds > 0
     group by b.owner_user_id, b.billing_org_id, b.hour
     order by b.hour, 1
  loop
    continue when exists (select 1 from public.usage_reports x where x.identifier = r.identifier);

    v_start := treq_internal.usage_period_start(r.owner_user_id, r.billing_org_id, r.hour);
    select coalesce(sum(x.managed_vcpu_seconds), 0), coalesce(sum(x.byos_vcpu_seconds), 0)
      into v_prior_m, v_prior_b
      from public.usage_reports x
     where x.hour >= v_start and x.hour <= r.hour
       and (case when r.billing_org_id is null
                 then x.owner_user_id = r.owner_user_id and x.billing_org_id is null
                 else x.billing_org_id = r.billing_org_id end);

    v_managed := r.cost_m + r.markup_m / 10000.0
                 * treq_internal.volume_rate_bps('managed', 10000, v_prior_m, r.vcpu_m) / 10000.0;
    v_byos := r.cost_b * treq_internal.volume_rate_bps('byos', treq_internal.byos_markup_bps(), v_prior_b, r.vcpu_b) / 10000.0;

    insert into public.usage_reports
      (identifier, owner_user_id, billing_org_id, hour, seconds, provider_cost_nanousd,
       billed_nanousd, byos_billed_nanousd, managed_vcpu_seconds, byos_vcpu_seconds)
    values (r.identifier, r.owner_user_id, r.billing_org_id, r.hour, round(r.seconds)::int, round(r.cost)::bigint,
            round(v_managed + v_byos)::bigint, round(v_byos)::bigint, round(r.vcpu_m)::bigint, round(r.vcpu_b)::bigint)
    on conflict (identifier) do nothing;
    v_count := v_count + 1;
  end loop;
  return v_count;
end;
$$;

create or replace function public.build_usage_reports(p_until timestamptz)
returns int
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  perform treq_internal.split_repriced_intervals(now());
  return treq_internal.write_usage_reports(date_trunc('hour', p_until - interval '5 minutes'), null);
end;
$$;

-- No hour rounding and no grace: the account is going away (067).
create or replace function public.build_final_usage_reports(p_user_id uuid, p_until timestamptz)
returns int
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  perform treq_internal.split_repriced_intervals(now());
  return treq_internal.write_usage_reports(least(p_until, now()), p_user_id);
end;
$$;

revoke all on function treq_internal.volume_rate_bps(text, int, numeric, numeric) from public, anon, authenticated;
revoke all on function treq_internal.usage_period_start(uuid, uuid, timestamptz) from public, anon, authenticated;
revoke all on function treq_internal.write_usage_reports(timestamptz, uuid) from public, anon, authenticated;
