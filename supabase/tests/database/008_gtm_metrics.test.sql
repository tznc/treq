-- 024_gtm_metrics.sql: public.gtm_metrics() returns aggregate counts for
-- the gtm-metrics Edge Function. It reads auth.users, so it runs as a
-- security definer, and only the service role may call it.
begin;
select plan(14);

select is_definer('public', 'gtm_metrics', array['date', 'date'],
  'gtm_metrics(date, date) is security definer');
select is(
  (select r.rolname from pg_proc p join pg_roles r on r.oid = p.proowner
   where p.oid = 'public.gtm_metrics(date, date)'::regprocedure),
  'postgres',
  'gtm_metrics(date, date) is owned by postgres');
select function_privs_are('public', 'gtm_metrics', array['date', 'date'], 'anon', array[]::text[],
  'anon cannot execute gtm_metrics');
select function_privs_are('public', 'gtm_metrics', array['date', 'date'], 'authenticated', array[]::text[],
  'authenticated cannot execute gtm_metrics');
select function_privs_are('public', 'gtm_metrics', array['date', 'date'], 'service_role', array['EXECUTE'],
  'service_role can execute gtm_metrics');

-- Fixtures in 2001, long before any real data. Window: 2001-01-01..2001-01-07.
insert into auth.users (id, email, created_at) values
  ('00000000-0000-0000-0000-0000000000d1', 'gtm-before@example.com', '2000-12-31 23:59:59+00'),
  ('00000000-0000-0000-0000-0000000000d2', 'gtm-first@example.com',  '2001-01-01 00:00:00+00'),
  ('00000000-0000-0000-0000-0000000000d3', 'gtm-last@example.com',   '2001-01-07 23:59:59+00'),
  ('00000000-0000-0000-0000-0000000000d4', 'gtm-after@example.com',  '2001-01-08 00:00:00+00');

insert into public.github_app_installations (id, account_login, account_type, app_id, linked_user_id, created_at) values
  (990000001, 'gtm-a', 'User', 1, '00000000-0000-0000-0000-0000000000d1', '2001-01-02'),
  (990000002, 'gtm-a-org', 'Organization', 1, '00000000-0000-0000-0000-0000000000d1', '2001-01-03'),
  (990000003, 'gtm-unlinked', 'Organization', 1, null, '2001-01-04'),
  (990000004, 'gtm-later', 'User', 1, '00000000-0000-0000-0000-0000000000d2', '2001-01-09');

select is(
  (public.gtm_metrics('2001-01-01', '2001-01-07') ->> 'accounts_created')::int, 2,
  'accounts_created counts accounts created from the start of from to the end of to (UTC)');
select is(
  (public.gtm_metrics('2001-01-01', '2001-01-07') ->> 'accounts_total')::int,
  (select count(*)::int from auth.users where created_at < '2001-01-08 00:00:00+00'),
  'accounts_total counts every account created before the end of the window');
select is(
  (public.gtm_metrics('2001-01-01', '2001-01-07') ->> 'installations_total')::int,
  (select count(*)::int from public.github_app_installations where created_at < '2001-01-08 00:00:00+00'),
  'installations_total counts installations created before the end of the window');
select is(
  (public.gtm_metrics('2001-01-01', '2001-01-07') ->> 'github_app_installations_linked')::int,
  (select count(distinct linked_user_id)::int from public.github_app_installations
   where created_at < '2001-01-08 00:00:00+00'),
  'github_app_installations_linked counts accounts, not installations');
select is(
  (public.gtm_metrics('2001-01-01', '2001-01-07') ->> 'github_app_installations_linked')::int, 1,
  'two installations linked to one account count once, and an unlinked one not at all');

-- Before 031_alpha_waitlist.sql exists the waitlist counts are null; after,
-- they are zero for this window, since no waitlist row dates from 2001.
select is(
  public.gtm_metrics('2001-01-01', '2001-01-07')
    -> 'alpha_waitlist_joined',
  case when to_regclass('public.alpha_waitlist') is null then 'null'::jsonb else '0'::jsonb end,
  'alpha_waitlist_joined is null without the waitlist table, else a count');

select is(
  (select array_agg(k order by k) from jsonb_object_keys(public.gtm_metrics('2001-01-01', '2001-01-07')) k),
  array['accounts_created', 'accounts_total', 'alpha_waitlist_active', 'alpha_waitlist_joined',
        'alpha_waitlist_unsubscribed', 'from', 'github_app_installations_linked',
        'installations_total', 'to'],
  'the result has only the documented aggregate keys');

select throws_ok(
  $$select public.gtm_metrics('2001-01-07', '2001-01-01')$$,
  '22023', null,
  'from after to is rejected');

select throws_ok(
  $$select public.gtm_metrics(null, '2001-01-01')$$,
  '22023', null,
  'a missing bound is rejected');

select * from finish();
rollback;
