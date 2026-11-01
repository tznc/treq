-- Provider switches, resizes and drains as a compare-and-swap state machine,
-- run under a lease by the hourly admin job or an inline resize request.

create table public.sandbox_migrations (
  id uuid primary key default gen_random_uuid(),
  instance_id uuid not null references public.remote_instances (id) on delete cascade,
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  source_placement_id uuid not null references public.remote_instance_placements (id),
  target_placement_id uuid references public.remote_instance_placements (id),
  target_machine_type_id text not null references public.provider_machine_types (id),
  reason text not null check (reason in ('user_resize', 'drain', 'retire', 'cheaper_default', 'restart')),
  state text not null default 'requested',
  preflight jsonb,
  acknowledged_data_loss boolean not null default false,
  deadline timestamptz,
  lease_owner text,
  lease_expires_at timestamptz,
  attempt int not null default 0,
  requested_by uuid,
  error jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  -- The instance fence at creation; cutover_migration() needs it unchanged.
  expected_fence bigint
);

create unique index sandbox_migrations_one_open
  on public.sandbox_migrations (instance_id)
  where state not in ('completed', 'rolled_back', 'failed', 'cancelled');

create table public.sandbox_migration_steps (
  id uuid primary key default gen_random_uuid(),
  migration_id uuid not null references public.sandbox_migrations (id) on delete cascade,
  from_state text not null,
  to_state text not null,
  detail jsonb,
  created_at timestamptz not null default now()
);

alter table public.sandbox_migrations enable row level security;
alter table public.sandbox_migration_steps enable row level security;
revoke all on public.sandbox_migrations, public.sandbox_migration_steps from anon, authenticated;

create function treq_internal.capture_migration_fence()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if new.expected_fence is null then
    select fence into new.expected_fence from public.remote_instances where id = new.instance_id;
  end if;
  return new;
end;
$$;

create trigger sandbox_migrations_capture_fence
  before insert on public.sandbox_migrations
  for each row execute function treq_internal.capture_migration_fence();

-- Where a fresh machine re-clones the repo from; repos without one are
-- flagged needs_attention on a switch.
alter table public.remote_repositories
  add column remote_url text;

alter table public.remote_instance_operations
  drop constraint remote_instance_operations_operation_type_check;
alter table public.remote_instance_operations
  add constraint remote_instance_operations_operation_type_check
  check (operation_type in (
    'provision', 'wake', 'reprovision', 'delete', 'resize',
    'register_client_key', 'revoke_client_key', 'issue_certificate',
    'register_endpoint', 'register_repository',
    'install_authorized_key', 'remove_authorized_key', 'keyscan_host_key',
    'admin_revoke_client_key', 'admin_force_recover_instance'
  ));

-- Operator actions that touch no one user's instance (price changes,
-- drains) are audited with no owner.
alter table public.remote_audit_events
  alter column owner_user_id drop not null;

-- ── State changes ─────────────────────────────────────────────────────

create function public.advance_migration(
  p_migration_id uuid, p_expected_state text, p_next_state text, p_detail jsonb default null)
returns boolean
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  update public.sandbox_migrations
     set state = p_next_state,
         updated_at = now(),
         error = case when p_next_state in ('failed', 'rolled_back') then p_detail else error end
   where id = p_migration_id and state = p_expected_state;
  if not found then
    return false;
  end if;
  insert into public.sandbox_migration_steps (migration_id, from_state, to_state, detail)
  values (p_migration_id, p_expected_state, p_next_state, p_detail);
  return true;
end;
$$;

-- advance_migration, only for the worker holding the lease.
create function public.advance_migration_leased(
  p_migration_id uuid, p_expected_state text, p_next_state text, p_detail jsonb, p_owner text)
returns boolean
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  perform 1 from public.sandbox_migrations
   where id = p_migration_id and lease_owner = p_owner
   for update;
  if not found then
    return false;
  end if;
  return public.advance_migration(p_migration_id, p_expected_state, p_next_state, p_detail);
end;
$$;

-- Cancels the instance's open migration from whatever state it is in, in
-- one statement, so a delete or lapse sweep never loses to a worker.
-- Returns its id, or null when none is open.
create function public.cancel_open_migration(p_instance_id uuid, p_detail jsonb default null)
returns uuid
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_id uuid;
  v_state text;
begin
  perform treq_internal.require_service_role();
  select id, state into v_id, v_state
    from public.sandbox_migrations
   where instance_id = p_instance_id
     and state not in ('completed', 'rolled_back', 'failed', 'cancelled')
   for update;
  if not found then
    return null;
  end if;
  update public.sandbox_migrations
     set state = 'cancelled', updated_at = now(), error = coalesce(p_detail, error)
   where id = v_id;
  insert into public.sandbox_migration_steps (migration_id, from_state, to_state, detail)
  values (v_id, v_state, 'cancelled', p_detail);
  return v_id;
end;
$$;

-- Sends the user's open resizes that have not reached cutover to
-- rolling_back (spend cap); the engine then deletes any candidate.
create function public.rollback_user_resizes(p_user_id uuid)
returns table (migration_id uuid)
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_m record;
begin
  perform treq_internal.require_service_role();
  for v_m in
    select id, state from public.sandbox_migrations
     where owner_user_id = p_user_id
       and reason = 'user_resize'
       and state in ('requested', 'preflight', 'awaiting_ack', 'target_provisioning',
                     'target_bootstrapped', 'verifying')
     for update
  loop
    update public.sandbox_migrations
       set state = 'rolling_back', updated_at = now()
     where id = v_m.id;
    insert into public.sandbox_migration_steps (migration_id, from_state, to_state, detail)
    values (v_m.id, v_m.state, 'rolling_back', '{"reason":"spend_cap"}');
    migration_id := v_m.id;
    return next;
  end loop;
end;
$$;

-- ── Leases ────────────────────────────────────────────────────────────

-- Claims up to p_limit runnable migrations (or just p_migration_id). An
-- expired lease, or one p_owner already holds, can be taken.
create function public.claim_sandbox_migrations(
  p_owner text, p_lease_sec int, p_limit int, p_migration_id uuid default null)
returns table (migration_id uuid)
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return query
  with picked as (
    select m.id
      from public.sandbox_migrations m
     where m.state in ('requested', 'preflight', 'target_provisioning', 'target_bootstrapped',
                       'verifying', 'cutover', 'source_retiring', 'rolling_back')
       and (p_migration_id is null or m.id = p_migration_id)
       and (m.lease_owner is null or m.lease_owner = p_owner or m.lease_expires_at < now())
     order by m.created_at
     limit greatest(p_limit, 0)
     for update skip locked
  )
  update public.sandbox_migrations m
     set lease_owner = p_owner,
         lease_expires_at = now() + make_interval(secs => p_lease_sec)
    from picked
   where m.id = picked.id
  returning m.id;
end;
$$;

create function public.renew_sandbox_migration_lease(p_migration_id uuid, p_owner text, p_lease_sec int)
returns boolean
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  update public.sandbox_migrations
     set lease_expires_at = now() + make_interval(secs => p_lease_sec)
   where id = p_migration_id and lease_owner = p_owner;
  return found;
end;
$$;

create function public.release_sandbox_migration(p_migration_id uuid, p_owner text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  update public.sandbox_migrations
     set lease_owner = null, lease_expires_at = null
   where id = p_migration_id and lease_owner = p_owner;
end;
$$;

-- ── Cutover ───────────────────────────────────────────────────────────

-- cutover_placement for a migration still in `cutover`, whose source is
-- still active and whose fence is the one captured at creation.
create function public.cutover_migration(p_migration_id uuid, p_target_placement_id uuid)
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_m public.sandbox_migrations;
  v_instance public.remote_instances;
begin
  perform treq_internal.require_service_role();
  select * into v_m from public.sandbox_migrations where id = p_migration_id for update;
  if not found then
    raise exception 'migration_not_found';
  end if;
  if v_m.state <> 'cutover' then
    raise exception 'migration_not_in_cutover';
  end if;
  select * into v_instance from public.remote_instances where id = v_m.instance_id for update;
  if v_instance.fence <> coalesce(v_m.expected_fence, v_instance.fence) then
    raise exception 'fence_mismatch';
  end if;
  if v_instance.active_placement_id is distinct from v_m.source_placement_id then
    raise exception 'source_not_active';
  end if;
  return public.cutover_placement(v_m.instance_id, p_target_placement_id, v_instance.fence);
end;
$$;

-- Moves a completed migration back to its source in one transaction:
-- refused while another migration is open or once the source is no longer
-- retiring.
create function public.reverse_cutover(p_instance_id uuid, p_migration_id uuid, p_detail jsonb default null)
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_instance public.remote_instances;
  v_m public.sandbox_migrations;
  v_source public.remote_instance_placements;
  v_fence bigint;
begin
  perform treq_internal.require_service_role();
  select * into v_instance from public.remote_instances where id = p_instance_id for update;
  if not found then
    raise exception 'instance_not_found';
  end if;
  select * into v_m from public.sandbox_migrations
   where id = p_migration_id and instance_id = p_instance_id
   for update;
  if not found or v_m.state <> 'completed' then
    raise exception 'migration_not_completed';
  end if;
  if exists (
    select 1 from public.sandbox_migrations
     where instance_id = p_instance_id
       and state not in ('completed', 'rolled_back', 'failed', 'cancelled')
  ) then
    raise exception 'migration_open';
  end if;
  -- Locks the source against destroy_retired_placements' claim.
  select * into v_source from public.remote_instance_placements
   where id = v_m.source_placement_id
   for update;
  if not found or v_source.role <> 'retiring' then
    raise exception 'source_not_retiring';
  end if;
  if v_instance.active_placement_id is distinct from v_m.target_placement_id then
    raise exception 'target_not_active';
  end if;

  update public.remote_instance_placements
     set role = 'candidate', retired_at = null
   where id = v_source.id;
  v_fence := public.cutover_placement(p_instance_id, v_source.id, v_instance.fence);
  perform public.set_placement_state(v_source.id, 'ready');

  update public.sandbox_migrations
     set state = 'rolled_back', updated_at = now(), error = p_detail
   where id = p_migration_id;
  insert into public.sandbox_migration_steps (migration_id, from_state, to_state, detail)
  values (p_migration_id, 'completed', 'rolled_back', p_detail);
  return v_fence;
end;
$$;

-- Placements still holding a machine nobody uses, for the operator's
-- destroy_retired_placements sweep: dead ones whose machine was never
-- deleted, and candidates no open migration owns, an hour after creation.
create function public.orphaned_placements(p_limit int)
returns setof public.remote_instance_placements
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return query
  select p.*
    from public.remote_instance_placements p
   where p.provider_resource_id is not null
     and ((p.role = 'dead' and p.state <> 'deleted')
          or (p.role = 'candidate'
              and p.created_at < now() - interval '1 hour'
              and not exists (
                select 1 from public.sandbox_migrations m
                 where m.instance_id = p.instance_id
                   and m.state not in ('completed', 'rolled_back', 'failed', 'cancelled'))))
   order by p.created_at
   limit greatest(p_limit, 0);
end;
$$;

revoke all on function treq_internal.capture_migration_fence() from public, anon, authenticated;
revoke all on function public.advance_migration(uuid, text, text, jsonb) from public, anon, authenticated;
revoke all on function public.advance_migration_leased(uuid, text, text, jsonb, text) from public, anon, authenticated;
revoke all on function public.cancel_open_migration(uuid, jsonb) from public, anon, authenticated;
revoke all on function public.rollback_user_resizes(uuid) from public, anon, authenticated;
revoke all on function public.claim_sandbox_migrations(text, int, int, uuid) from public, anon, authenticated;
revoke all on function public.renew_sandbox_migration_lease(uuid, text, int) from public, anon, authenticated;
revoke all on function public.release_sandbox_migration(uuid, text) from public, anon, authenticated;
revoke all on function public.cutover_migration(uuid, uuid) from public, anon, authenticated;
revoke all on function public.reverse_cutover(uuid, uuid, jsonb) from public, anon, authenticated;
revoke all on function public.orphaned_placements(int) from public, anon, authenticated;
grant execute on function public.advance_migration(uuid, text, text, jsonb) to service_role;
grant execute on function public.advance_migration_leased(uuid, text, text, jsonb, text) to service_role;
grant execute on function public.cancel_open_migration(uuid, jsonb) to service_role;
grant execute on function public.rollback_user_resizes(uuid) to service_role;
grant execute on function public.claim_sandbox_migrations(text, int, int, uuid) to service_role;
grant execute on function public.renew_sandbox_migration_lease(uuid, text, int) to service_role;
grant execute on function public.release_sandbox_migration(uuid, text) to service_role;
grant execute on function public.cutover_migration(uuid, uuid) to service_role;
grant execute on function public.reverse_cutover(uuid, uuid, jsonb) to service_role;
grant execute on function public.orphaned_placements(int) to service_role;
