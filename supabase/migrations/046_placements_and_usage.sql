-- Placements and the usage ledger.
--
-- remote_instances stays the customer-facing identity; each machine behind it
-- is a placement, exactly one active, mirrored onto the instance row.
-- usage_intervals is the billing ledger: set_placement_state() and
-- cutover_placement() change it in the same transaction as the state, so
-- billing cannot drift. usage_reports holds one row per (owner, hour) for
-- the Stripe meter.

-- ── Instances and placements ──────────────────────────────────────────

alter table public.remote_instances
  drop constraint remote_instances_provider_kind_check;
alter table public.remote_instances
  drop constraint if exists remote_instances_disk_quota_gb_check;
alter table public.remote_instances
  alter column provider_kind drop default;
update public.remote_instances set provider_kind = 'boat';
alter table public.remote_instances
  add constraint remote_instances_provider_kind_check
  check (provider_kind in ('boat', 'local', 'daytona', 'fly_machines', 'e2b'));

alter table public.remote_instances
  add column tier_id text references public.size_tiers (id),
  add column machine_type_id text references public.provider_machine_types (id),
  add column active_placement_id uuid,
  add column fence bigint not null default 0,
  add column credential_source text not null default 'managed'
    check (credential_source in ('managed', 'byos')),
  add column credential_id uuid references public.remote_provider_credentials (id) on delete set null;

create table public.remote_instance_placements (
  id uuid primary key default gen_random_uuid(),
  instance_id uuid not null references public.remote_instances (id) on delete cascade,
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  provider_kind text not null references public.sandbox_providers (kind),
  machine_type_id text not null references public.provider_machine_types (id),
  region text not null,
  provider_resource_id text,
  state text not null default 'provisioning',
  role text not null check (role in ('active', 'candidate', 'retiring', 'dead')),
  created_at timestamptz not null default now(),
  retired_at timestamptz,
  credential_source text not null default 'managed'
    check (credential_source in ('managed', 'byos')),
  credential_id uuid references public.remote_provider_credentials (id) on delete set null,
  constraint remote_instance_placements_managed_no_credential
    check (credential_source = 'byos' or credential_id is null)
);

create index remote_instance_placements_owner_user_id_idx
  on public.remote_instance_placements (owner_user_id);

create unique index remote_instance_placements_one_active
  on public.remote_instance_placements (instance_id)
  where role = 'active';
-- Two workers running target_provisioning cannot both make a target.
create unique index remote_instance_placements_one_candidate
  on public.remote_instance_placements (instance_id)
  where role = 'candidate';

alter table public.remote_instances
  add constraint remote_instances_active_placement_fkey
  foreign key (active_placement_id) references public.remote_instance_placements (id)
  on delete set null deferrable initially deferred;

-- A BYOS placement must use its owner's credential for its own kind, and a
-- BYOS workspace never moves to a treq-managed account.
create function treq_internal.placements_byos_guard()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  -- On update a null credential is allowed: deleting a credential sets it
  -- null on dead placements (on delete set null).
  if tg_op = 'INSERT' and new.credential_source = 'byos' and new.credential_id is null then
    raise exception 'byos_credential_required';
  end if;
  if new.credential_source = 'byos' and new.credential_id is not null then
    -- Pairs with the row lock in delete_provider_credential().
    perform 1 from public.remote_provider_credentials c
      where c.id = new.credential_id
        and c.owner_user_id = new.owner_user_id
        and c.provider_kind = new.provider_kind
      for key share;
    if not found then
      raise exception 'byos_credential_mismatch';
    end if;
  end if;
  if new.credential_source = 'managed' and exists (
    select 1 from public.remote_instance_placements p
     where p.instance_id = new.instance_id
       and p.role = 'active'
       and p.credential_source = 'byos'
  ) then
    raise exception 'byos_placement_must_stay_byos';
  end if;
  return new;
end;
$$;

create trigger remote_instance_placements_byos_guard
  before insert or update of credential_source, credential_id, owner_user_id, provider_kind
  on public.remote_instance_placements
  for each row execute function treq_internal.placements_byos_guard();

-- The disk quota is the machine type's disk.
create function treq_internal.remote_instance_disk_quota()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if new.machine_type_id is not null then
    select m.disk_gb into new.disk_quota_gb
      from public.provider_machine_types m
     where m.id = new.machine_type_id;
  end if;
  return new;
end;
$$;

create trigger remote_instances_disk_quota
  before insert or update of machine_type_id on public.remote_instances
  for each row execute function treq_internal.remote_instance_disk_quota();

-- Existing instances get tier s and one placement mirroring the row.
update public.remote_instances
   set tier_id = 's', machine_type_id = 'boat:small';

with created as (
  insert into public.remote_instance_placements
    (instance_id, owner_user_id, provider_kind, machine_type_id, region,
     provider_resource_id, state, role)
  select i.id, i.owner_user_id, i.provider_kind, 'boat:small', i.region,
         i.provider_resource_id, i.status,
         case when i.status = 'deleted' then 'dead' else 'active' end
    from public.remote_instances i
  returning id, instance_id, role
)
update public.remote_instances i
   set active_placement_id = c.id
  from created c
 where c.instance_id = i.id and c.role = 'active';

-- ── Usage ledger ──────────────────────────────────────────────────────

create table public.usage_intervals (
  id uuid primary key default gen_random_uuid(),
  placement_id uuid not null references public.remote_instance_placements (id) on delete cascade,
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  kind text not null check (kind in ('running', 'stopped_storage')),
  price_id uuid not null references public.machine_type_prices (id),
  disk_gb int,
  started_at timestamptz not null default now(),
  ended_at timestamptz,
  -- Copied from the placement when the interval opens.
  billing_mode text not null default 'managed' check (billing_mode in ('managed', 'byos')),
  check (ended_at is null or ended_at >= started_at),
  exclude using gist (
    placement_id with =,
    kind with =,
    tstzrange(started_at, ended_at) with &&
  )
);

create index usage_intervals_open on public.usage_intervals (placement_id) where ended_at is null;
create index usage_intervals_owner_time on public.usage_intervals (owner_user_id, started_at);

-- Append and close only: an interval keeps its start, price, owner and
-- mode, and a closed interval stays closed.
create function treq_internal.usage_intervals_guard()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if new.started_at <> old.started_at
     or new.price_id <> old.price_id
     or new.owner_user_id <> old.owner_user_id
     or new.placement_id <> old.placement_id
     or new.kind <> old.kind
     or new.billing_mode <> old.billing_mode
     or (old.ended_at is not null and new.ended_at is distinct from old.ended_at) then
    raise exception 'usage_intervals is append and close only';
  end if;
  return new;
end;
$$;

create trigger usage_intervals_guard
  before update on public.usage_intervals
  for each row execute function treq_internal.usage_intervals_guard();

create table public.usage_reports (
  identifier text primary key,
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  hour timestamptz not null,
  seconds int not null,
  provider_cost_nanousd bigint not null,
  billed_nanousd bigint not null,
  created_at timestamptz not null default now(),
  sent_at timestamptz,
  -- 'no_customer' and 'dead' are terminal (usage-report.ts), so a row that
  -- cannot be sent stops blocking newer ones.
  stripe_status text check (stripe_status in ('accepted', 'zero', 'error', 'no_customer', 'dead')),
  error text,
  -- The BYOS part of billed_nanousd.
  byos_billed_nanousd bigint not null default 0,
  attempts int not null default 0 check (attempts >= 0)
);

create index usage_reports_unsent on public.usage_reports (created_at)
  where sent_at is null and (stripe_status is null or stripe_status = 'error');

alter table public.remote_instance_placements enable row level security;
alter table public.usage_intervals enable row level security;
alter table public.usage_reports enable row level security;
revoke all on public.remote_instance_placements, public.usage_intervals, public.usage_reports
  from anon, authenticated;

-- ── Functions ─────────────────────────────────────────────────────────

-- Closes every open interval whose price ended at or before p_now at that
-- price's effective_to and continues it on the next price, following a
-- chain of changes to the current one.
create function treq_internal.split_repriced_intervals(p_now timestamptz)
returns int
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_id uuid;
  v_end timestamptz;
  v_machine_type text;
  v_next uuid;
  v_count int := 0;
begin
  loop
    select u.id, p.effective_to, p.machine_type_id
      into v_id, v_end, v_machine_type
      from public.usage_intervals u
      join public.machine_type_prices p on p.id = u.price_id
     where u.ended_at is null
       and p.effective_to is not null
       and p.effective_to <= p_now
       and u.started_at < p.effective_to
     order by u.started_at
     limit 1
     for update of u;
    exit when not found;

    v_next := treq_internal.current_price_id(v_machine_type, v_end);
    update public.usage_intervals set ended_at = v_end where id = v_id;
    if v_next is not null then
      insert into public.usage_intervals
        (placement_id, owner_user_id, kind, price_id, disk_gb, started_at, billing_mode)
      select u.placement_id, u.owner_user_id, u.kind, v_next, u.disk_gb, v_end, u.billing_mode
        from public.usage_intervals u
       where u.id = v_id;
    end if;
    v_count := v_count + 1;
  end loop;
  return v_count;
end;
$$;

-- Closes the open price row and adds the new one; a price already in effect
-- splits the open intervals on the old one.
create function public.set_machine_type_price(
  p_machine_type_id text,
  p_running_nanousd_per_sec bigint,
  p_stopped_nanousd_per_gb_sec bigint default 0,
  p_effective_from timestamptz default now())
returns uuid
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_open public.machine_type_prices;
  v_id uuid;
begin
  perform treq_internal.require_service_role();
  select * into v_open
    from public.machine_type_prices
   where machine_type_id = p_machine_type_id and effective_to is null
   for update;
  if found then
    if v_open.effective_from >= p_effective_from then
      raise exception 'effective_from must be after the current price';
    end if;
    update public.machine_type_prices set effective_to = p_effective_from where id = v_open.id;
  end if;
  insert into public.machine_type_prices
    (machine_type_id, running_nanousd_per_sec, stopped_nanousd_per_gb_sec, markup_bps, effective_from)
  values
    (p_machine_type_id, p_running_nanousd_per_sec, coalesce(p_stopped_nanousd_per_gb_sec, 0),
     coalesce(v_open.markup_bps, 2000), p_effective_from)
  returning id into v_id;
  perform treq_internal.split_repriced_intervals(now());
  perform public.recompute_pro_defaults();
  return v_id;
end;
$$;

-- Records a placement's state and keeps the ledger in step. An active
-- running placement has one open 'running' interval; a suspended one has a
-- 'stopped_storage' interval when its price bills stopped disk; others are
-- not billed. p_at is the provider's transition time, clamped to
-- [last ledger entry, now()].
create function public.set_placement_state(
  p_placement_id uuid,
  p_state text,
  p_disk_gb int default null,
  p_at timestamptz default null)
returns void
language plpgsql
security definer
set search_path = ''
as $$
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
    when p_state = 'suspended' and coalesce(v_price.stopped_nanousd_per_gb_sec, 0) > 0
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
$$;

-- Makes the candidate active in one transaction: checks the fence, swaps
-- roles, moves billing and mirrors the placement onto the instance row.
create function public.cutover_placement(p_instance_id uuid, p_target_placement_id uuid, p_expected_fence bigint)
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_instance public.remote_instances;
  v_target public.remote_instance_placements;
  v_now timestamptz := now();
begin
  perform treq_internal.require_service_role();
  select * into v_instance from public.remote_instances where id = p_instance_id for update;
  if not found then
    raise exception 'instance_not_found';
  end if;
  if v_instance.fence <> p_expected_fence then
    raise exception 'fence_mismatch';
  end if;
  select * into v_target from public.remote_instance_placements
   where id = p_target_placement_id and instance_id = p_instance_id for update;
  if not found or v_target.role <> 'candidate' then
    raise exception 'target_not_candidate';
  end if;

  update public.usage_intervals u
     set ended_at = v_now
    from public.remote_instance_placements p
   where u.placement_id = p.id
     and p.instance_id = p_instance_id
     and p.role = 'active'
     and u.ended_at is null;

  update public.remote_instance_placements
     set role = 'retiring', retired_at = v_now
   where instance_id = p_instance_id and role = 'active';
  update public.remote_instance_placements
     set role = 'active'
   where id = p_target_placement_id;

  update public.remote_instances
     set active_placement_id = v_target.id,
         provider_kind = v_target.provider_kind,
         provider_resource_id = v_target.provider_resource_id,
         machine_type_id = v_target.machine_type_id,
         tier_id = (select tier_id from public.provider_machine_types where id = v_target.machine_type_id),
         fence = fence + 1,
         updated_at = v_now
   where id = p_instance_id;

  perform public.set_placement_state(v_target.id, v_target.state);

  return v_instance.fence + 1;
end;
$$;

-- Buckets usage into (owner, hour) rows for each hour that ended at least
-- 5 minutes before p_until, so an interval that commits late is not lost
-- behind a row already written. Cost is summed in nano-USD and rounded once
-- per row; an hour is reported once.
create function public.build_usage_reports(p_until timestamptz)
returns int
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_count int;
  v_cutoff timestamptz := date_trunc('hour', p_until - interval '5 minutes');
begin
  perform treq_internal.require_service_role();
  perform treq_internal.split_repriced_intervals(now());
  insert into public.usage_reports
    (identifier, owner_user_id, hour, seconds, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd)
  select b.owner_user_id::text || ':' || to_char(b.hour at time zone 'UTC', 'YYYY-MM-DD"T"HH24'),
         b.owner_user_id, b.hour,
         round(sum(b.seconds))::int,
         round(sum(b.cost))::bigint,
         round(sum(treq_internal.billed_nanousd(b.cost, b.markup_bps, b.billing_mode)))::bigint,
         round(sum(case when b.billing_mode = 'byos'
                        then treq_internal.billed_nanousd(b.cost, b.markup_bps, b.billing_mode)
                        else 0 end))::bigint
    from (
      select u.owner_user_id, h.hour, p.markup_bps, u.billing_mode,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour)) as seconds,
             extract(epoch from
               least(coalesce(u.ended_at, v_cutoff), h.hour + interval '1 hour', v_cutoff)
               - greatest(u.started_at, h.hour))
             * case u.kind
                 when 'running' then p.running_nanousd_per_sec
                 else p.stopped_nanousd_per_gb_sec * coalesce(u.disk_gb, 0)
               end as cost
        from public.usage_intervals u
        join public.machine_type_prices p on p.id = u.price_id
        cross join lateral generate_series(
          date_trunc('hour', u.started_at),
          least(coalesce(u.ended_at, v_cutoff), v_cutoff) - interval '1 microsecond',
          interval '1 hour') as h(hour)
       where u.started_at < v_cutoff
    ) b
   where b.hour < v_cutoff and b.seconds > 0
   group by b.owner_user_id, b.hour
  on conflict (identifier) do nothing;
  get diagnostics v_count = row_count;
  return v_count;
end;
$$;

create function public.get_usage_summary(p_from timestamptz, p_to timestamptz)
returns table (hour timestamptz, seconds int, billed_usd numeric, byos_billed_usd numeric)
language sql
stable
security definer
set search_path = ''
as $$
  select r.hour, r.seconds, round(r.billed_nanousd / 1e9, 6), round(r.byos_billed_nanousd / 1e9, 6)
    from public.usage_reports r
   where r.owner_user_id = (select auth.uid())
     and r.hour >= p_from and r.hour < p_to
   order by r.hour;
$$;

revoke all on function treq_internal.remote_instance_disk_quota() from public, anon, authenticated;
revoke all on function treq_internal.placements_byos_guard() from public, anon, authenticated;
revoke all on function treq_internal.split_repriced_intervals(timestamptz) from public, anon, authenticated;
revoke all on function public.set_machine_type_price(text, bigint, bigint, timestamptz) from public, anon, authenticated;
revoke all on function public.set_placement_state(uuid, text, int, timestamptz) from public, anon, authenticated;
revoke all on function public.cutover_placement(uuid, uuid, bigint) from public, anon, authenticated;
revoke all on function public.build_usage_reports(timestamptz) from public, anon, authenticated;
grant execute on function public.set_machine_type_price(text, bigint, bigint, timestamptz) to service_role;
grant execute on function public.set_placement_state(uuid, text, int, timestamptz) to service_role;
grant execute on function public.cutover_placement(uuid, uuid, bigint) to service_role;
grant execute on function public.build_usage_reports(timestamptz) to service_role;
revoke all on function public.get_usage_summary(timestamptz, timestamptz) from public, anon;
grant execute on function public.get_usage_summary(timestamptz, timestamptz) to authenticated;
