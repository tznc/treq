-- Cloud workspace catalog: providers, size tiers, machine types, versioned
-- prices and the cheapest default per tier and region
-- (docs/design/sandbox-provider-adapter.md).

create extension if not exists btree_gist with schema extensions;

create table public.sandbox_providers (
  kind text primary key check (kind in ('boat', 'local', 'daytona', 'fly_machines', 'e2b')),
  display_name text not null,
  status text not null default 'active' check (status in ('active', 'draining', 'retiring', 'retired')),
  billing_granularity_sec int not null check (billing_granularity_sec > 0),
  capabilities jsonb not null default '{}'::jsonb,
  health text not null default 'ok' check (health in ('ok', 'degraded', 'down')),
  updated_at timestamptz not null default now()
);

create table public.size_tiers (
  id text primary key,
  label text not null,
  vcpu numeric not null check (vcpu > 0),
  ram_gib numeric not null check (ram_gib > 0),
  min_disk_gb int not null check (min_disk_gb > 0),
  sort int not null unique
);

create table public.provider_machine_types (
  id text primary key,
  provider_kind text not null references public.sandbox_providers (kind),
  vendor_type text not null,
  tier_id text not null references public.size_tiers (id),
  vcpu numeric not null,
  ram_gib numeric not null,
  disk_gb int not null,
  regions text[] not null,
  selectable boolean not null default true
);

-- Never updated in place: a price change ends the old row and adds a new
-- one, so an interval keeps the rate it started with.
create table public.machine_type_prices (
  id uuid primary key default gen_random_uuid(),
  machine_type_id text not null references public.provider_machine_types (id),
  running_nanousd_per_sec bigint not null check (running_nanousd_per_sec >= 0),
  stopped_nanousd_per_gb_sec bigint not null default 0 check (stopped_nanousd_per_gb_sec >= 0),
  markup_bps int not null default 2000 check (markup_bps >= 0),
  effective_from timestamptz not null default now(),
  effective_to timestamptz,
  exclude using gist (
    machine_type_id with =,
    tstzrange(effective_from, effective_to) with &&
  )
);

create table public.pro_provider_defaults (
  tier_id text not null references public.size_tiers (id),
  region text not null,
  machine_type_id text not null references public.provider_machine_types (id),
  computed_at timestamptz not null default now(),
  primary key (tier_id, region)
);

alter table public.sandbox_providers enable row level security;
alter table public.size_tiers enable row level security;
alter table public.provider_machine_types enable row level security;
alter table public.machine_type_prices enable row level security;
alter table public.pro_provider_defaults enable row level security;

-- Customers read prices only through get_machine_catalog().
create policy "Anyone signed in can read size tiers"
  on public.size_tiers for select to authenticated using (true);

revoke all on public.sandbox_providers, public.provider_machine_types,
  public.machine_type_prices, public.pro_provider_defaults
  from anon, authenticated;
revoke all on public.size_tiers from anon;
revoke insert, update, delete on public.size_tiers from authenticated;

-- ── Seed data ─────────────────────────────────────────────────────────
-- Prices are nano-USD per second ($/h * 1e9 / 3600). Sources and the
-- unverified figures are listed in the design doc.

insert into public.sandbox_providers (kind, display_name, billing_granularity_sec, capabilities) values
  ('boat', 'Boat', 1,
   '{"stopKeepsDisk":true,"execApi":true,"serviceSupervisor":false,"tcpProxy":true}'),
  -- In-process fake; its machine types are selectable only when a developer
  -- turns them on (scripts/service-qa).
  ('local', 'Local stub', 1,
   '{"stopKeepsDisk":true,"execApi":true,"serviceSupervisor":true,"tcpProxy":false}'),
  ('daytona', 'Daytona', 1,
   '{"stopKeepsDisk":true,"execApi":true,"serviceSupervisor":false,"tcpProxy":true}'),
  ('fly_machines', 'Fly Machines', 1,
   '{"stopKeepsDisk":true,"execApi":true,"serviceSupervisor":false,"tcpProxy":true}'),
  ('e2b', 'E2B', 1,
   '{"stopKeepsDisk":true,"execApi":true,"serviceSupervisor":false,"tcpProxy":true}');

insert into public.size_tiers (id, label, vcpu, ram_gib, min_disk_gb, sort) values
  ('s', 'Small', 2, 4, 10, 1),
  ('m', 'Medium', 4, 8, 40, 2),
  ('l', 'Large', 8, 16, 100, 3),
  ('xl', 'Extra large', 16, 32, 200, 4);

-- vendor_type: Boat's size name; the tier id for Daytona and E2B (E2B starts
-- template `<TREQ_E2B_TEMPLATE>-<tier>`); the Fly guest for Fly Machines.
-- Fly is not selectable: all customers would share one Fly app and its
-- private network until it uses an app per owner.
insert into public.provider_machine_types
  (id, provider_kind, vendor_type, tier_id, vcpu, ram_gib, disk_gb, regions, selectable)
values
  ('boat:small', 'boat', 'small', 's', 2, 4, 12, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('boat:default', 'boat', 'default', 'm', 4, 8, 50, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('boat:large', 'boat', 'large', 'l', 8, 16, 125, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('boat:xlarge', 'boat', 'xlarge', 'xl', 16, 32, 251, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('local:s', 'local', 'local', 's', 2, 4, 10, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false),
  ('local:m', 'local', 'local', 'm', 4, 8, 40, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false),
  ('daytona:s', 'daytona', 's', 's', 2, 4, 10, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('daytona:m', 'daytona', 'm', 'm', 4, 8, 40, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('daytona:l', 'daytona', 'l', 'l', 8, 16, 100, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('daytona:xl', 'daytona', 'xl', 'xl', 16, 32, 200, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('fly_machines:s', 'fly_machines', 'shared-cpu-2x:4096', 's', 2, 4, 10, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false),
  ('fly_machines:m', 'fly_machines', 'performance-4x:8192', 'm', 4, 8, 40, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false),
  ('fly_machines:l', 'fly_machines', 'performance-8x:16384', 'l', 8, 16, 100, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false),
  ('fly_machines:xl', 'fly_machines', 'performance-16x:32768', 'xl', 16, 32, 200, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], false),
  ('e2b:s', 'e2b', 's', 's', 2, 4, 10, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('e2b:m', 'e2b', 'm', 'm', 4, 8, 40, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('e2b:l', 'e2b', 'l', 'l', 8, 16, 100, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true),
  ('e2b:xl', 'e2b', 'xl', 'xl', 16, 32, 200, array['us_east', 'us_west', 'eu_west', 'ap_southeast'], true);

insert into public.machine_type_prices
  (machine_type_id, running_nanousd_per_sec, stopped_nanousd_per_gb_sec)
values
  ('boat:small', 5000, 0),
  ('boat:default', 10000, 0),
  ('boat:large', 20000, 0),
  ('boat:xlarge', 55556, 0),
  ('local:s', 1000, 0),
  ('local:m', 2000, 0),
  ('daytona:s', 46000, 30),
  ('daytona:m', 92000, 30),
  ('daytona:l', 184000, 30),
  ('daytona:xl', 368000, 30),
  ('fly_machines:s', 9661, 57),
  ('fly_machines:m', 50228, 57),
  ('fly_machines:l', 100457, 57),
  ('fly_machines:xl', 200913, 57),
  ('e2b:s', 46000, 0),
  ('e2b:m', 92000, 0),
  ('e2b:l', 184000, 0),
  ('e2b:xl', 368000, 0);

-- ── Functions ─────────────────────────────────────────────────────────

create function treq_internal.require_service_role()
returns void
language plpgsql
set search_path = ''
as $$
begin
  if coalesce(current_setting('role', true), '') <> 'service_role'
     and current_user not in ('postgres', 'supabase_admin') then
    raise exception 'service role required' using errcode = '42501';
  end if;
end;
$$;

create function treq_internal.current_price_id(p_machine_type_id text, p_at timestamptz)
returns uuid
language sql
stable
set search_path = ''
as $$
  select p.id
    from public.machine_type_prices p
   where p.machine_type_id = p_machine_type_id
     and p.effective_from <= p_at
     and (p.effective_to is null or p.effective_to > p_at)
   order by p.effective_from desc
   limit 1;
$$;

-- Cheapest active, healthy, selectable machine type per tier and region;
-- ties break by machine type id.
create function public.recompute_pro_defaults()
returns int
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_count int;
begin
  perform treq_internal.require_service_role();
  -- pg-safeupdate (loaded for PostgREST) rejects a DELETE with no WHERE.
  delete from public.pro_provider_defaults where true;
  insert into public.pro_provider_defaults (tier_id, region, machine_type_id)
  select distinct on (t.tier_id, r.region) t.tier_id, r.region, t.id
    from public.provider_machine_types t
    join public.sandbox_providers sp on sp.kind = t.provider_kind
    cross join lateral unnest(t.regions) as r(region)
    join public.machine_type_prices p
      on p.id = treq_internal.current_price_id(t.id, now())
   where t.selectable
     and sp.status = 'active'
     and sp.health <> 'down'
   order by t.tier_id, r.region, p.running_nanousd_per_sec, t.id;
  get diagnostics v_count = row_count;
  return v_count;
end;
$$;

-- Tiers with the default machine type and billed hourly price per region.
-- Provider cost and markup are never returned.
create function public.get_machine_catalog()
returns jsonb
language sql
stable
security definer
set search_path = ''
as $$
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
                     p.running_nanousd_per_sec * 3600 * (10000 + p.markup_bps) / 10000.0 / 1e9, 6)
                 ) order by d.region)
            from public.pro_provider_defaults d
            join public.machine_type_prices p
              on p.id = treq_internal.current_price_id(d.machine_type_id, now())
           where d.tier_id = st.id
        ), '[]'::jsonb)
      ) as tier
      from public.size_tiers st
    ) tiers;
$$;

revoke all on function public.recompute_pro_defaults() from public, anon, authenticated;
grant execute on function public.recompute_pro_defaults() to service_role;
revoke all on function public.get_machine_catalog() from public, anon;
grant execute on function public.get_machine_catalog() to authenticated, service_role;

select public.recompute_pro_defaults();
