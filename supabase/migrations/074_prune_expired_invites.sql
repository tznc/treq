-- Expired organization invites are deleted once a day by a pg_cron job
-- (supabase/postgres preloads pg_cron, hosted and self-hosted). An invite
-- that expired without being accepted (revoked or not) goes; accepted
-- invites stay as the record of who joined.

create or replace function public.prune_expired_organization_invites()
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  deleted_count bigint;
begin
  delete from public.organization_invites v
  where v.accepted_at is null
    and v.expires_at < now();
  get diagnostics deleted_count = row_count;
  return deleted_count;
end;
$$;

revoke all on function public.prune_expired_organization_invites() from public, anon, authenticated;
grant execute on function public.prune_expired_organization_invites() to service_role;

create extension if not exists pg_cron;

-- cron.schedule with a job name replaces an existing job of that name, so
-- re-running this migration does not add a second job.
select cron.schedule(
  'prune-expired-organization-invites',
  '23 3 * * *',
  'select public.prune_expired_organization_invites()'
);
