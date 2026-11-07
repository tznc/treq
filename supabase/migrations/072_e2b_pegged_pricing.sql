-- E2B-pegged pricing. Running time bills at E2B's rate for the size tier,
-- whichever provider runs it: managed pays that rate plus the markup (20%),
-- BYOS pays the BYOS fee (5%) of it. Stopped disk keeps the provider's own
-- storage rate. usage_reports.provider_cost_nanousd stays the real provider
-- cost, so margin per provider stays visible.
--
-- The functions below are 068's (and 044/045's catalogs) with the running
-- rate read through list_running_nanousd_per_sec.

-- E2B's running rate for the machine type's size tier at p_at; the machine
-- type's own rate (p_fallback) when E2B has no price for that tier then.
create function treq_internal.list_running_nanousd_per_sec(
  p_machine_type_id text, p_at timestamptz, p_fallback bigint)
returns bigint
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (select e.running_nanousd_per_sec
       from public.provider_machine_types m
       join public.machine_type_prices e
         on e.id = treq_internal.current_price_id('e2b:' || m.tier_id, p_at)
      where m.id = p_machine_type_id),
    p_fallback);
$$;

revoke all on function treq_internal.list_running_nanousd_per_sec(text, timestamptz, bigint) from public, anon, authenticated;

CREATE OR REPLACE FUNCTION treq_internal.month_spend_nanousd(p_user_id uuid, p_now timestamp with time zone)
 RETURNS bigint
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  with bounds as (
    select treq_internal.spend_window_start(p_user_id, p_now) as month_start
  ),
  reported as (
    select coalesce(sum(r.billed_nanousd), 0) as billed,
           max(r.hour) + interval '1 hour' as reported_until
      from public.usage_reports r, bounds b
     where r.owner_user_id = p_user_id
       and r.billing_org_id is null
       and r.hour >= b.month_start and r.hour < p_now
  ),
  open as (
    select coalesce(sum(treq_internal.billed_nanousd(
             extract(epoch from
               least(coalesce(u.ended_at, p_now), p_now)
               - greatest(u.started_at, greatest(b.month_start, coalesce(rp.reported_until, b.month_start))))
             * case u.kind
                 when 'running' then treq_internal.list_running_nanousd_per_sec(p.machine_type_id, u.started_at, p.running_nanousd_per_sec)
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end,
             p.markup_bps, u.billing_mode)), 0) as billed
      from bounds b
     cross join reported rp
      join public.usage_intervals u
        on u.owner_user_id = p_user_id
       and u.billing_org_id is null
       and u.started_at < p_now
       and coalesce(u.ended_at, p_now) > greatest(b.month_start, coalesce(rp.reported_until, b.month_start))
      join public.machine_type_prices p on p.id = u.price_id
  )
  select (round((select billed from reported) + (select billed from open)))::bigint;
$function$;

CREATE OR REPLACE FUNCTION treq_internal.org_spend_nanousd(p_org_id uuid, p_user_id uuid, p_now timestamp with time zone)
 RETURNS bigint
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  with w as (select treq_internal.org_spend_window_start(p_org_id, p_now) as start)
  select coalesce(round(sum(treq_internal.billed_nanousd(
           extract(epoch from least(coalesce(u.ended_at, p_now), p_now) - greatest(u.started_at, w.start))
           * case u.kind
               when 'running' then treq_internal.list_running_nanousd_per_sec(p.machine_type_id, u.started_at, p.running_nanousd_per_sec)
               else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
             end,
           p.markup_bps, u.billing_mode))), 0)::bigint
    from w
    join public.usage_intervals u
      on u.billing_org_id = p_org_id
     and (p_user_id is null or u.owner_user_id = p_user_id)
     and u.started_at < p_now
     and coalesce(u.ended_at, p_now) > w.start
    join public.machine_type_prices p on p.id = u.price_id;
$function$;

CREATE OR REPLACE FUNCTION public.build_usage_reports(p_until timestamp with time zone)
 RETURNS integer
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
declare
  v_count int;
  v_cutoff timestamptz := date_trunc('hour', p_until - interval '5 minutes');
begin
  perform treq_internal.require_service_role();
  perform treq_internal.split_repriced_intervals(now());
  insert into public.usage_reports
    (identifier, owner_user_id, billing_org_id, hour, seconds, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd)
  select b.owner_user_id::text || coalesce(':' || b.billing_org_id::text, '') || ':' || to_char(b.hour at time zone 'UTC', 'YYYY-MM-DD"T"HH24'),
         b.owner_user_id, b.billing_org_id, b.hour,
         round(sum(b.seconds))::int,
         round(sum(b.cost))::bigint,
         round(sum(treq_internal.billed_nanousd(b.list_cost, b.markup_bps, b.billing_mode)))::bigint,
         round(sum(case when b.billing_mode = 'byos'
                        then treq_internal.billed_nanousd(b.list_cost, b.markup_bps, b.billing_mode)
                        else 0 end))::bigint
    from (
      select u.owner_user_id, u.billing_org_id, h.hour, p.markup_bps, u.billing_mode,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour)) as seconds,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour))
             * case u.kind
                 when 'running' then p.running_nanousd_per_sec
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end as cost,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour))
             * case u.kind
                 when 'running' then treq_internal.list_running_nanousd_per_sec(p.machine_type_id, h.hour, p.running_nanousd_per_sec)
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end as list_cost
        from public.usage_intervals u
        join public.machine_type_prices p on p.id = u.price_id
        cross join lateral generate_series(
          date_trunc('hour', u.started_at),
          least(coalesce(u.ended_at, v_cutoff), v_cutoff) - interval '1 microsecond',
          interval '1 hour') as h(hour)
       where u.started_at < v_cutoff
    ) b
   where b.hour < v_cutoff and b.seconds > 0
   group by b.owner_user_id, b.billing_org_id, b.hour
  on conflict (identifier) do nothing;
  get diagnostics v_count = row_count;
  return v_count;
end;
$function$;

CREATE OR REPLACE FUNCTION public.build_final_usage_reports(p_user_id uuid, p_until timestamp with time zone)
 RETURNS integer
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
declare
  v_count int;
  -- No hour rounding and no grace: the account is going away, and its
  -- machines were deleted (billing closed) before this runs.
  v_cutoff timestamptz := least(p_until, now());
begin
  perform treq_internal.require_service_role();
  perform treq_internal.split_repriced_intervals(now());
  insert into public.usage_reports
    (identifier, owner_user_id, billing_org_id, hour, seconds, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd)
  select b.owner_user_id::text || coalesce(':' || b.billing_org_id::text, '') || ':' || to_char(b.hour at time zone 'UTC', 'YYYY-MM-DD"T"HH24'),
         b.owner_user_id, b.billing_org_id, b.hour,
         round(sum(b.seconds))::int,
         round(sum(b.cost))::bigint,
         round(sum(treq_internal.billed_nanousd(b.list_cost, b.markup_bps, b.billing_mode)))::bigint,
         round(sum(case when b.billing_mode = 'byos'
                        then treq_internal.billed_nanousd(b.list_cost, b.markup_bps, b.billing_mode)
                        else 0 end))::bigint
    from (
      select u.owner_user_id, u.billing_org_id, h.hour, p.markup_bps, u.billing_mode,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour)) as seconds,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour))
             * case u.kind
                 when 'running' then p.running_nanousd_per_sec
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end as cost,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour))
             * case u.kind
                 when 'running' then treq_internal.list_running_nanousd_per_sec(p.machine_type_id, h.hour, p.running_nanousd_per_sec)
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end as list_cost
        from public.usage_intervals u
        join public.machine_type_prices p on p.id = u.price_id
        cross join lateral generate_series(
          date_trunc('hour', u.started_at),
          least(coalesce(u.ended_at, v_cutoff), v_cutoff) - interval '1 microsecond',
          interval '1 hour') as h(hour)
       where u.started_at < v_cutoff
         and u.owner_user_id = p_user_id
    ) b
   where b.hour < v_cutoff and b.seconds > 0
   group by b.owner_user_id, b.billing_org_id, b.hour
  on conflict (identifier) do nothing;
  get diagnostics v_count = row_count;
  return v_count;
end;
$function$;

CREATE OR REPLACE FUNCTION public.get_machine_catalog()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  select coalesce(jsonb_agg(tier order by sort), '[]'::jsonb)
    from (
      select st.sort, jsonb_build_object(
        'id', st.id,
        'label', st.label,
        'vcpu', st.vcpu,
        'ram_gib', st.ram_gib,
        'disk_gb', st.min_disk_gb,
        'regions', coalesce((
          select jsonb_agg(jsonb_build_object(
                   'region', d.region,
                   'machine_type_id', d.machine_type_id,
                   'hourly_usd', round(
                     treq_internal.list_running_nanousd_per_sec(d.machine_type_id, now(), p.running_nanousd_per_sec) * 3600 * (10000 + p.markup_bps) / 10000.0 / 1e9, 6)
                 ) order by d.region)
            from public.pro_provider_defaults d
            join public.machine_type_prices p
              on p.id = treq_internal.current_price_id(d.machine_type_id, now())
           where d.tier_id = st.id
        ), '[]'::jsonb)
      ) as tier
      from public.size_tiers st
    ) tiers;
$function$;

CREATE OR REPLACE FUNCTION public.get_byos_catalog()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object(
           'provider_kind', c.provider_kind,
           'provider_name', sp.display_name,
           'credential_valid', c.invalid_at is null,
           'tier_id', t.tier_id,
           'machine_type_id', t.id,
           'regions', to_jsonb(t.regions),
           'provider_hourly_usd', round(p.running_nanousd_per_sec * 3600 / 1e9, 6),
           'hourly_usd', round(treq_internal.billed_nanousd(
              treq_internal.list_running_nanousd_per_sec(t.id, now(), p.running_nanousd_per_sec) * 3600, p.markup_bps, 'byos') / 1e9, 6)
         ) order by c.provider_kind, st.sort), '[]'::jsonb)
    from public.remote_provider_credentials c
    join public.sandbox_providers sp on sp.kind = c.provider_kind
    join lateral (
      select distinct on (m.tier_id) m.*
        from public.provider_machine_types m
       where m.provider_kind = c.provider_kind
       order by m.tier_id, m.selectable desc, m.id
    ) t on true
    join public.size_tiers st on st.id = t.tier_id
    join public.machine_type_prices p on p.id = treq_internal.current_price_id(t.id, now())
   where c.owner_user_id = (select auth.uid())
     and c.owner_org_id is null;
$function$;
