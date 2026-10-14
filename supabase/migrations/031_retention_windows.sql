-- Retention windows promised by the Privacy Policy (web/src/pages/privacy.mdx,
-- "Retention") and the Terms ("Cloud Workspaces"):
--
-- * A lapsed cloud workspace's disk is deleted 7 days after its owner's Pro
--   ends, down from 30 (prds/billing-and-teams.md, open decision B02).
-- * Remote audit events are kept 7 days for members of an organization with
--   Team and 1 day for everyone else, down from 90 for all. The daily
--   `prune_audit_events` run in remote-maintenance.yml applies it.

-- ── Audit event retention ─────────────────────────────────────────────────

-- retention_days is now the window for anyone without Team. Team members get
-- team_retention_days.
alter table public.remote_audit_retention_config
  alter column retention_days set default 1,
  add column team_retention_days integer not null default 7 check (team_retention_days > 0);

update public.remote_audit_retention_config
set retention_days = 1, team_retention_days = 7
where id = true;

-- Deletes each owner's audit events older than their plan's window. Plan is
-- read at prune time, so an owner who leaves a Team drops to the shorter
-- window at the next run.
create or replace function public.prune_remote_audit_events()
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  deleted_count bigint;
  base_days integer;
  team_days integer;
begin
  select c.retention_days, c.team_retention_days
  into base_days, team_days
  from public.remote_audit_retention_config c
  where c.id = true;
  base_days := coalesce(base_days, 1);
  team_days := coalesce(team_days, 7);

  delete from public.remote_audit_events e
  where e.created_at < now() - make_interval(days => case
    when exists (
      select 1
      from public.organization_members m
      where m.user_id = e.owner_user_id
        and treq_internal.organization_has_team(m.org_id)
    ) then team_days
    else base_days
  end);

  get diagnostics deleted_count = row_count;
  return deleted_count;
end;
$$;

revoke all on function public.prune_remote_audit_events() from public, anon, authenticated;
grant execute on function public.prune_remote_audit_events() to service_role;

-- ── Lapsed cloud workspaces ───────────────────────────────────────────────

comment on column public.remote_instances.lapsed_at is
  'When the owner''s Pro entitlement was found to have ended. Cleared when Pro returns. The lapse sweep deletes the instance 7 days after this.';

-- Moves an instance to 'deleting' when its owner has had no Pro for 7 days.
-- Returns whether it did. The sweep deletes the provider resource only after
-- this returns true.
create or replace function public.remote_claim_lapsed_deletion(p_instance_id uuid)
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
      and i.lapsed_at <= now() - interval '7 days'
      and not treq_internal.user_has_pro(i.owner_user_id)
    returning 1
  )
  select exists (select 1 from claimed);
$$;
