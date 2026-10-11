-- Server-side Pro enforcement (prds/billing-and-teams.md, "Enforcement",
-- rollout step 3).
--
-- Every cloud feature asks Postgres whether its owner is entitled to Pro:
--
-- * treq_internal.user_has_pro holds the entitlement rule. It answers about
--   any user and needs no JWT, so pg_cron jobs and the functions below can
--   use it. Only the database owner can call it: treq_internal is not
--   exposed through PostgREST and no API role holds EXECUTE.
-- * public.has_pro keeps its contract: a signed-in user may ask about
--   themselves, the service role about anyone.
-- * public.installation_has_pro answers for a GitHub App installation. Today
--   that is the user who linked it. Organizations (rollout step 4) extend
--   this one function with the organization's Team subscription.
--
-- The merge queue opt-in and the cloud workspace lapse sweep build on them.

-- ── Entitlement ───────────────────────────────────────────────────────────

create function treq_internal.user_has_pro(p_user_id uuid)
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
      and s.status in ('trialing', 'active', 'past_due')
  );
$$;

revoke all on function treq_internal.user_has_pro(uuid)
  from public, anon, authenticated, service_role;

-- Same answers as 025_billing_entitlement.sql. The rule now lives in
-- treq_internal.user_has_pro, so there is still one place that reads
-- subscription rows to decide access.
create or replace function public.has_pro(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (
      p_user_id = (select auth.uid())
      or current_setting('role', true) = 'service_role'
    )
    and treq_internal.user_has_pro(p_user_id),
    false
  );
$$;

-- Is the owner of this GitHub App installation entitled to Pro? The merge
-- queue worker calls it with the service role. Signed-in users cannot call
-- it, so it cannot reveal whether another user pays.
create function public.installation_has_pro(p_installation_id bigint)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(
    (
      select treq_internal.user_has_pro(i.linked_user_id)
      from public.github_app_installations i
      where i.id = p_installation_id
        and i.linked_user_id is not null
    ),
    false
  );
$$;

revoke all on function public.installation_has_pro(bigint) from public, anon, authenticated;
grant execute on function public.installation_has_pro(bigint) to service_role;

-- ── Merge queue opt-in ────────────────────────────────────────────────────
--
-- Turning the queue on needs Pro. Turning it off always works, and a queue
-- that is already on stays on when Pro ends: the worker pauses it instead
-- (_shared/merge-queue/entitlement-pause.ts), so it resumes when Pro returns.
-- SQLSTATE PT402 makes PostgREST answer HTTP 402. The hint carries the code
-- clients match on.

create or replace function public.set_merge_queue_enabled(
  p_repo_full_name text,
  p_enabled boolean
)
returns boolean
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_repo_id bigint;
  v_default_branch text;
  v_installation_id bigint;
begin
  select r.id, coalesce(r.default_branch, 'main'), r.installation_id
    into v_repo_id, v_default_branch, v_installation_id
  from public.github_repositories r
  join public.github_app_installations i
    on i.id = r.installation_id
  where r.full_name = p_repo_full_name
    and i.linked_user_id = (select auth.uid())
  limit 1;

  if v_repo_id is null then
    raise exception 'Repository % is not linked to a GitHub App installation for this account', p_repo_full_name
      using errcode = 'no_data_found';
  end if;

  if p_enabled and not public.installation_has_pro(v_installation_id) then
    raise exception using
      message = 'The merge queue needs Pro',
      errcode = 'PT402',
      hint = 'pro_required';
  end if;

  insert into public.merge_queue_configs (repo_id, target_branch, enabled)
  values (v_repo_id, v_default_branch, p_enabled)
  on conflict (repo_id, target_branch)
  do update set enabled = excluded.enabled, updated_at = now();

  return p_enabled;
end;
$$;

-- The linking user can also write merge_queue_configs through RLS (the web
-- dashboard saves settings that way), so the same rule applies to the rows.
-- Only a change from off to on counts, and moving an enabled row to another
-- repo or branch counts as turning that one on. An upsert of a queue that is
-- already on passes the BEFORE INSERT check here and then the BEFORE UPDATE
-- check.
--
-- BEFORE triggers run before the RLS WITH CHECK, so for a signed-in caller
-- the check runs only on the caller's own installation. Anyone else's row
-- falls through to RLS and fails the same way whether its owner pays or not.
create function treq_internal.merge_queue_configs_require_pro()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_installation_id bigint;
  v_linked_user_id uuid;
begin
  if not new.enabled then
    return new;
  end if;
  if tg_op = 'UPDATE'
    and old.enabled
    and old.repo_id = new.repo_id
    and old.target_branch = new.target_branch then
    return new;
  end if;
  if tg_op = 'INSERT' and exists (
    select 1
    from public.merge_queue_configs c
    where c.repo_id = new.repo_id
      and c.target_branch = new.target_branch
      and c.enabled
  ) then
    return new;
  end if;

  select r.installation_id, i.linked_user_id
    into v_installation_id, v_linked_user_id
  from public.github_repositories r
  left join public.github_app_installations i on i.id = r.installation_id
  where r.id = new.repo_id;

  if current_setting('role', true) in ('anon', 'authenticated')
    and v_linked_user_id is distinct from (select auth.uid()) then
    return new;
  end if;

  if not public.installation_has_pro(v_installation_id) then
    raise exception using
      message = 'The merge queue needs Pro',
      errcode = 'PT402',
      hint = 'pro_required';
  end if;
  return new;
end;
$$;

revoke all on function treq_internal.merge_queue_configs_require_pro()
  from public, anon, authenticated, service_role;

create trigger merge_queue_configs_require_pro
  before insert or update of enabled, repo_id, target_branch on public.merge_queue_configs
  for each row execute function treq_internal.merge_queue_configs_require_pro();

-- ── Cloud workspace lapse (open decision B02) ─────────────────────────────
--
-- When the owner's entitlement ends, the sweep (remote-admin
-- `sweep_lapsed_instances`) stops the instance and records lapsed_at. After
-- 30 days without Pro it deletes the instance. The functions below make
-- each step re-check entitlement against database time at the moment it
-- writes, so neither a clock-skewed caller nor a stale read can delete the
-- instance of someone who pays.

alter table public.remote_instances add column lapsed_at timestamptz;

comment on column public.remote_instances.lapsed_at is
  'When the owner''s Pro entitlement was found to have ended. Cleared when Pro returns. The lapse sweep deletes the instance 30 days after this.';

-- Live instances whose owner has no Pro, plus any that still carry a lapse.
create function public.remote_lapse_candidates()
returns table (
  instance_id uuid,
  owner_user_id uuid,
  status text,
  provider_resource_id text,
  lapsed_at timestamptz,
  owner_has_pro boolean
)
language sql
stable
security definer
set search_path = ''
as $$
  select i.id, i.owner_user_id, i.status, i.provider_resource_id, i.lapsed_at, e.has_pro
  from public.remote_instances i
  cross join lateral (
    select treq_internal.user_has_pro(i.owner_user_id) as has_pro
  ) e
  where i.status <> 'deleted'
    and (i.lapsed_at is not null or not e.has_pro)
  order by i.lapsed_at nulls first, i.id;
$$;

-- Records the lapse of an instance whose owner has no Pro (keeping the first
-- lapse time) or clears it when the owner has Pro. Returns lapsed_at after
-- the write: null means the owner is entitled.
create function public.remote_sync_instance_lapse(p_instance_id uuid)
returns timestamptz
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_lapsed_at timestamptz;
begin
  update public.remote_instances i
  set lapsed_at = case
        when treq_internal.user_has_pro(i.owner_user_id) then null
        else coalesce(i.lapsed_at, now())
      end,
      updated_at = now()
  where i.id = p_instance_id
    and i.status <> 'deleted'
  returning i.lapsed_at into v_lapsed_at;
  return v_lapsed_at;
end;
$$;

-- Moves an instance to 'deleting' when its owner has had no Pro for 30 days.
-- Returns whether it did. The sweep deletes the provider resource only after
-- this returns true.
create function public.remote_claim_lapsed_deletion(p_instance_id uuid)
returns boolean
language sql
security definer
set search_path = ''
as $$
  with claimed as (
    update public.remote_instances i
    set status = 'deleting', updated_at = now()
    where i.id = p_instance_id
      and i.status <> 'deleted'
      and i.lapsed_at is not null
      and i.lapsed_at <= now() - interval '30 days'
      and not treq_internal.user_has_pro(i.owner_user_id)
    returning 1
  )
  select exists (select 1 from claimed);
$$;

revoke all on function public.remote_lapse_candidates() from public, anon, authenticated;
revoke all on function public.remote_sync_instance_lapse(uuid) from public, anon, authenticated;
revoke all on function public.remote_claim_lapsed_deletion(uuid) from public, anon, authenticated;
grant execute on function public.remote_lapse_candidates() to service_role;
grant execute on function public.remote_sync_instance_lapse(uuid) to service_role;
grant execute on function public.remote_claim_lapsed_deletion(uuid) to service_role;

-- A subscription that makes its owner entitled clears the owner's lapse in
-- the same transaction as the webhook's write. Without this, a user who
-- lapsed, subscribed again, and lapsed again before the next sweep would be
-- measured from the first lapse.
create function treq_internal.clear_remote_lapse_on_entitlement()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  if new.owner_type = 'user'
    and new.owner_id is not null
    and treq_internal.user_has_pro(new.owner_id) then
    update public.remote_instances
    set lapsed_at = null, updated_at = now()
    where owner_user_id = new.owner_id
      and lapsed_at is not null
      and status <> 'deleted';
  end if;
  return null;
end;
$$;

revoke all on function treq_internal.clear_remote_lapse_on_entitlement()
  from public, anon, authenticated, service_role;

create trigger billing_subscriptions_clear_remote_lapse
  after insert or update on public.billing_subscriptions
  for each row execute function treq_internal.clear_remote_lapse_on_entitlement();
