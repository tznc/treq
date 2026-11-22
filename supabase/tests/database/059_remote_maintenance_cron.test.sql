-- 082_remote_maintenance_cron.sql: pg_cron + pg_net call remote-admin on the
-- schedules .github/workflows/remote-maintenance.yml used to run.
begin;
select plan(10);

select results_eq(
  $$select jobname, schedule, command from cron.job
    where jobname like 'remote-admin-%' order by jobname$$,
  $$values
    ('remote-admin-advance-migrations'::text, '41 * * * *'::text, 'select public.invoke_remote_admin(''advance_migrations'')'::text),
    ('remote-admin-destroy-retired-placements', '41 * * * *', 'select public.invoke_remote_admin(''destroy_retired_placements'')'),
    ('remote-admin-enforce-idle', '*/5 * * * *', 'select public.invoke_remote_admin(''enforce_idle'')'),
    ('remote-admin-enforce-spend-caps', '2,17,32,47 * * * *', 'select public.invoke_remote_admin(''enforce_spend_caps'')'),
    ('remote-admin-prune-audit-events', '23 3 * * *', 'select public.invoke_remote_admin(''prune_audit_events'')'),
    ('remote-admin-report-usage', '7 * * * *', 'select public.invoke_remote_admin(''report_usage'')'),
    ('remote-admin-sweep-lapsed-instances', '17 * * * *', 'select public.invoke_remote_admin(''sweep_lapsed_instances'')')$$,
  'one cron job per remote-admin action, on the old workflow schedules');

select function_privs_are('public', 'invoke_remote_admin', array['text'], 'authenticated',
  array[]::text[], 'authenticated cannot invoke remote-admin');
select function_privs_are('public', 'invoke_remote_admin', array['text'], 'anon',
  array[]::text[], 'anon cannot invoke remote-admin');

-- No Vault secrets: no request is queued.
select is(public.invoke_remote_admin('enforce_idle'), null::bigint, 'no-op without vault secrets');

select vault.create_secret('https://example.test/functions/v1/remote-admin', 'remote_admin_url');
select is(public.invoke_remote_admin('enforce_idle'), null::bigint, 'no-op with only the url secret');
select vault.create_secret('k3y', 'remote_admin_api_key');

select isnt(public.invoke_remote_admin('enforce_idle'), null::bigint, 'queues a request once both secrets exist');
select is((select method from net.http_request_queue order by id desc limit 1), 'POST', 'POST');
select is((select url from net.http_request_queue order by id desc limit 1),
  'https://example.test/functions/v1/remote-admin', 'to the vault url');
select is((select headers->>'x-admin-api-key' from net.http_request_queue order by id desc limit 1),
  'k3y', 'with the admin key header');
select is((select convert_from(body, 'utf8')::jsonb from net.http_request_queue order by id desc limit 1),
  '{"action":"enforce_idle"}'::jsonb, 'with the action body');

select * from finish();
rollback;
