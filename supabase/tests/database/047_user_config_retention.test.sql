-- Synced config keeps the last 3 versions of each scope (063).
begin;
select plan(4);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000046a1', 'config-retention@example.com');
set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000046a1","role":"authenticated"}', true);

select public.put_user_config('app', v - 1, jsonb_build_object('v', v::text)) from generate_series(1, 5) v;
select public.put_user_config('repo:a', 0, '{"a":"1"}'::jsonb);
reset role;

select results_eq(
  $$ select version from public.user_config_snapshots
      where owner_user_id = '00000000-0000-0000-0000-0000000046a1' and scope = 'app' order by version $$,
  $$ values (3::bigint), (4::bigint), (5::bigint) $$,
  'five saves of a scope keep its last 3 versions');
select is((select count(*)::int from public.user_config_snapshots
            where owner_user_id = '00000000-0000-0000-0000-0000000046a1' and scope = 'repo:a'),
  1, 'other scopes are untouched');

set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000046a1","role":"authenticated"}', true);
select is((public.get_user_config('app'))->'files'->>'v', '5', 'the latest version is still read');
select is(public.put_user_config('app', 5, '{"v":"6"}'::jsonb), 6::bigint, 'versions keep counting after pruning');
reset role;

select * from finish();
rollback;
