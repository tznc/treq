-- Fixed list prices, kept in admin_config: one price per size on every
-- provider, whichever provider runs the workspace. Managed and BYOS each have an hourly rate per size, and
-- stopped disk bills per GB-month wherever the provider keeps the disk
-- (stopKeepsDisk). The rates started from E2B (running) and Daytona (disk),
-- with the 20% markup and 5% BYOS fee, rounded:
--
--   size   managed   BYOS      stopped disk (all sizes)
--   s      $0.20/h   $0.0085/h  managed $0.10/GB-month
--   m      $0.40/h   $0.017/h   BYOS    $0.004/GB-month
--   l      $0.80/h   $0.034/h
--   xl     $1.60/h   $0.068/h
--
-- This replaces 072's rates, which followed E2B's running rate and Daytona's
-- storage rate times the markup. Prices no longer follow provider prices or
-- the markup settings: changing a price is an edit to the 'list_prices' row
-- of admin_config, in the dashboard, with no migration.
-- usage_reports.provider_cost_nanousd stays the real provider cost, so margin
-- per provider stays visible. If the 'list_prices' row is deleted, billing
-- falls back to provider cost with the markup or fee.
--
-- The functions below replace 072's, with the billed rate read through
-- billed_running_nanousd_per_sec / billed_stopped_nanousd_per_gb_sec.

-- Operator settings, one row per key, edited in the Supabase dashboard
-- (Table Editor -> admin_config). A key with a known shape is checked on write
-- by admin_config_validate, so a bad edit is refused instead of breaking
-- billing. Only the service role and the dashboard can read or write it.
create table public.admin_config (
  key text primary key,
  value jsonb not null,
  description text,
  updated_at timestamptz not null default now()
);
alter table public.admin_config enable row level security;
revoke all on public.admin_config from anon, authenticated;

create function treq_internal.admin_config_validate()
returns trigger
language plpgsql
set search_path = ''
as $$
declare
  v_path text[];
  v_paths text[][] := array[
    array['tiers', 's', 'managed_usd_per_hour'], array['tiers', 's', 'byos_usd_per_hour'],
    array['tiers', 'm', 'managed_usd_per_hour'], array['tiers', 'm', 'byos_usd_per_hour'],
    array['tiers', 'l', 'managed_usd_per_hour'], array['tiers', 'l', 'byos_usd_per_hour'],
    array['tiers', 'xl', 'managed_usd_per_hour'], array['tiers', 'xl', 'byos_usd_per_hour'],
    array['stopped_disk', 'managed_usd_per_gb_month', ''], array['stopped_disk', 'byos_usd_per_gb_month', '']];
begin
  new.updated_at := now();
  if new.key = 'list_prices' then
    foreach v_path slice 1 in array v_paths loop
      v_path := array_remove(v_path, '');
      if jsonb_typeof(new.value #> v_path) is distinct from 'number'
         or (new.value #>> v_path)::numeric < 0 then
        raise exception 'list_prices: % must be a number >= 0', array_to_string(v_path, '.');
      end if;
    end loop;
  end if;
  return new;
end;
$$;

create trigger admin_config_validate
  before insert or update on public.admin_config
  for each row execute function treq_internal.admin_config_validate();

insert into public.admin_config (key, value, description) values ('list_prices', '{
  "tiers": {
    "s":  {"managed_usd_per_hour": 0.20, "byos_usd_per_hour": 0.0085},
    "m":  {"managed_usd_per_hour": 0.40, "byos_usd_per_hour": 0.017},
    "l":  {"managed_usd_per_hour": 0.80, "byos_usd_per_hour": 0.034},
    "xl": {"managed_usd_per_hour": 1.60, "byos_usd_per_hour": 0.068}
  },
  "stopped_disk": {"managed_usd_per_gb_month": 0.10, "byos_usd_per_gb_month": 0.004}
}', 'Cloud workspace prices per size tier (US$/hour) and for stopped disk (US$/GB-month), managed and BYOS. Takes effect on the next hourly usage report.');

-- Billed nano-USD per running second for a machine type: its size's list
-- price for the billing mode, else p_fallback.
create function treq_internal.billed_running_nanousd_per_sec(
  p_machine_type_id text, p_billing_mode text, p_fallback numeric)
returns numeric
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (select (c.value #>> array['tiers', m.tier_id,
               case when p_billing_mode = 'byos' then 'byos_usd_per_hour' else 'managed_usd_per_hour' end])::numeric
            * 1e9 / 3600
       from public.provider_machine_types m, public.admin_config c
      where m.id = p_machine_type_id and c.key = 'list_prices'),
    p_fallback);
$$;

-- Billed nano-USD per GB-second of stopped disk (a month is 730 hours): 0
-- where the provider loses the disk when stopped (Cloudflare), else the list
-- price for the billing mode, else p_fallback.
create function treq_internal.billed_stopped_nanousd_per_gb_sec(
  p_machine_type_id text, p_billing_mode text, p_fallback numeric)
returns numeric
language sql
stable
security definer
set search_path = ''
as $$
  select case
    when (select (sp.capabilities->>'stopKeepsDisk')::boolean
            from public.provider_machine_types m
            join public.sandbox_providers sp on sp.kind = m.provider_kind
           where m.id = p_machine_type_id) is false then 0::numeric
    else coalesce(
      (select (c.value #>> array['stopped_disk',
                 case when p_billing_mode = 'byos' then 'byos_usd_per_gb_month' else 'managed_usd_per_gb_month' end])::numeric
              * 1e9 / (730 * 3600)
         from public.admin_config c
        where c.key = 'list_prices'),
      p_fallback)
  end;
$$;

revoke all on function treq_internal.admin_config_validate() from public, anon, authenticated;
revoke all on function treq_internal.billed_running_nanousd_per_sec(text, text, numeric) from public, anon, authenticated;
revoke all on function treq_internal.billed_stopped_nanousd_per_gb_sec(text, text, numeric) from public, anon, authenticated;

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
    select coalesce(sum(
             extract(epoch from
               least(coalesce(u.ended_at, p_now), p_now)
               - greatest(u.started_at, greatest(b.month_start, coalesce(rp.reported_until, b.month_start))))
             * case u.kind
                 when 'running' then treq_internal.billed_running_nanousd_per_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.running_nanousd_per_sec, p.markup_bps, u.billing_mode))
                 else treq_internal.billed_stopped_nanousd_per_gb_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.stopped_nanousd_per_gb_sec, p.markup_bps, u.billing_mode)) * coalesce(u.disk_gb, 0)
               end), 0) as billed
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
  select coalesce(round(sum(
           extract(epoch from least(coalesce(u.ended_at, p_now), p_now) - greatest(u.started_at, w.start))
           * case u.kind
               when 'running' then treq_internal.billed_running_nanousd_per_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.running_nanousd_per_sec, p.markup_bps, u.billing_mode))
               else treq_internal.billed_stopped_nanousd_per_gb_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.stopped_nanousd_per_gb_sec, p.markup_bps, u.billing_mode)) * coalesce(u.disk_gb, 0)
             end)), 0)::bigint
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
         round(sum(b.billed))::bigint,
         round(sum(case when b.billing_mode = 'byos'
                        then b.billed
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
                 when 'running' then treq_internal.billed_running_nanousd_per_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.running_nanousd_per_sec, p.markup_bps, u.billing_mode))
                 else treq_internal.billed_stopped_nanousd_per_gb_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.stopped_nanousd_per_gb_sec, p.markup_bps, u.billing_mode)) * coalesce(u.disk_gb, 0)
               end as billed
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
         round(sum(b.billed))::bigint,
         round(sum(case when b.billing_mode = 'byos'
                        then b.billed
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
                 when 'running' then treq_internal.billed_running_nanousd_per_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.running_nanousd_per_sec, p.markup_bps, u.billing_mode))
                 else treq_internal.billed_stopped_nanousd_per_gb_sec(p.machine_type_id, u.billing_mode, treq_internal.billed_nanousd(p.stopped_nanousd_per_gb_sec, p.markup_bps, u.billing_mode)) * coalesce(u.disk_gb, 0)
               end as billed
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
                     treq_internal.billed_running_nanousd_per_sec(d.machine_type_id, 'managed', p.running_nanousd_per_sec * (10000 + p.markup_bps) / 10000.0) * 3600 / 1e9, 6)
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
           'hourly_usd', round(treq_internal.billed_running_nanousd_per_sec(t.id, 'byos', treq_internal.billed_nanousd(p.running_nanousd_per_sec, p.markup_bps, 'byos')) * 3600 / 1e9, 6)
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

CREATE OR REPLACE FUNCTION public.set_placement_state(p_placement_id uuid, p_state text, p_disk_gb integer DEFAULT NULL::integer, p_at timestamp with time zone DEFAULT NULL::timestamp with time zone)
 RETURNS void
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
declare
  v_placement public.remote_instance_placements;
  v_price public.machine_type_prices;
  v_want text;
  v_now timestamptz := now();
  v_at timestamptz := least(coalesce(p_at, now()), now());
  v_start timestamptz;
begin
  perform treq_internal.require_service_role();
  -- Split first, so an interval closed below is priced correctly.
  perform treq_internal.split_repriced_intervals(v_now);

  update public.remote_instance_placements
     set state = p_state
   where id = p_placement_id
  returning * into v_placement;
  if not found then
    raise exception 'placement_not_found';
  end if;

  select greatest(v_at, coalesce(max(coalesce(u.ended_at, u.started_at)), v_at)) into v_at
    from public.usage_intervals u
   where u.placement_id = p_placement_id;

  select * into v_price
    from public.machine_type_prices
   where id = treq_internal.current_price_id(v_placement.machine_type_id, v_at);

  v_want := case
    when v_placement.role <> 'active' then null
    -- The machine bills until the provider confirms the delete.
    when p_state = 'deleting' then (
      select u.kind from public.usage_intervals u
       where u.placement_id = p_placement_id and u.ended_at is null
       order by u.started_at desc limit 1)
    when p_state in ('ready', 'waking', 'degraded', 'bootstrapping', 'installing_access', 'verifying')
      then 'running'
    when p_state = 'suspended' and treq_internal.billed_stopped_nanousd_per_gb_sec(v_price.machine_type_id, 'managed', v_price.stopped_nanousd_per_gb_sec) > 0
      then 'stopped_storage'
    else null
  end;

  update public.usage_intervals
     set ended_at = greatest(started_at, v_at)
   where placement_id = p_placement_id
     and ended_at is null
     and kind is distinct from v_want;

  if v_want is not null and not exists (
    select 1 from public.usage_intervals
     where placement_id = p_placement_id and ended_at is null and kind = v_want
  ) then
    select greatest(v_at, coalesce(max(u.ended_at), v_at)) into v_start
      from public.usage_intervals u
     where u.placement_id = p_placement_id;
    select * into v_price
      from public.machine_type_prices
     where id = treq_internal.current_price_id(v_placement.machine_type_id, v_start);
    if v_price.id is not null then
      insert into public.usage_intervals
        (placement_id, owner_user_id, kind, price_id, disk_gb, started_at, billing_mode)
      values
        (p_placement_id, v_placement.owner_user_id, v_want, v_price.id,
         case when v_want = 'stopped_storage' then coalesce(
           p_disk_gb,
           (select m.disk_gb from public.provider_machine_types m where m.id = v_placement.machine_type_id),
           0) end,
         v_start, v_placement.credential_source);
    end if;
  end if;

  -- A backdated start may span a price change.
  perform treq_internal.split_repriced_intervals(v_now);

  if v_placement.role = 'active' then
    update public.remote_instances
       set status = p_state,
           credential_source = v_placement.credential_source,
           credential_id = v_placement.credential_id,
           updated_at = v_now
     where id = v_placement.instance_id
       and (status is distinct from p_state
            or credential_source is distinct from v_placement.credential_source
            or credential_id is distinct from v_placement.credential_id);
  end if;
end;
$function$;

-- 072's pegged-rate helpers have no callers left.
drop function treq_internal.list_running_nanousd_per_sec(text, timestamptz, bigint);
drop function treq_internal.list_stopped_nanousd_per_gb_sec(text, timestamptz, bigint);
