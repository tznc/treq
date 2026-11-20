-- Scheduled remote-admin actions run from pg_cron (replaces
-- .github/workflows/remote-maintenance.yml). Each job POSTs
-- {"action": ...} to the remote-admin Edge Function via pg_net. The URL and
-- key live in Vault as `remote_admin_url` and `remote_admin_api_key`; until
-- both exist every job is a no-op.

create extension if not exists pg_cron;
create extension if not exists pg_net;

create or replace function public.invoke_remote_admin(p_action text)
returns bigint
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_url text;
  v_key text;
begin
  select decrypted_secret into v_url from vault.decrypted_secrets where name = 'remote_admin_url';
  select decrypted_secret into v_key from vault.decrypted_secrets where name = 'remote_admin_api_key';
  if v_url is null or v_key is null then
    return null;
  end if;
  return net.http_post(
    url := v_url,
    body := jsonb_build_object('action', p_action),
    headers := jsonb_build_object('content-type', 'application/json', 'x-admin-api-key', v_key),
    timeout_milliseconds := 300000
  );
end;
$$;

revoke all on function public.invoke_remote_admin(text) from public, anon, authenticated;
grant execute on function public.invoke_remote_admin(text) to service_role;

-- cron.schedule with a job name replaces an existing job of that name.
select cron.schedule('remote-admin-report-usage', '7 * * * *',
  'select public.invoke_remote_admin(''report_usage'')');
select cron.schedule('remote-admin-sweep-lapsed-instances', '17 * * * *',
  'select public.invoke_remote_admin(''sweep_lapsed_instances'')');
select cron.schedule('remote-admin-enforce-spend-caps', '2,17,32,47 * * * *',
  'select public.invoke_remote_admin(''enforce_spend_caps'')');
select cron.schedule('remote-admin-prune-audit-events', '23 3 * * *',
  'select public.invoke_remote_admin(''prune_audit_events'')');
select cron.schedule('remote-admin-advance-migrations', '41 * * * *',
  'select public.invoke_remote_admin(''advance_migrations'')');
select cron.schedule('remote-admin-destroy-retired-placements', '41 * * * *',
  'select public.invoke_remote_admin(''destroy_retired_placements'')');
select cron.schedule('remote-admin-enforce-idle', '*/5 * * * *',
  'select public.invoke_remote_admin(''enforce_idle'')');
