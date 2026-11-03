-- Synced config storage is bounded per user (059): at most 100 scopes, each
-- name at most 256 characters, so a signed-in user cannot grow storage
-- without limit (1 MiB x 20 versions per scope).
begin;
select plan(5);

insert into auth.users (id, email) values ('00000000-0000-0000-0000-0000000039a1', 'config-limits@example.com');
set local role authenticated;
select set_config('request.jwt.claims', '{"sub":"00000000-0000-0000-0000-0000000039a1","role":"authenticated"}', true);

select lives_ok(
  $$ select public.put_user_config('repo:' || g, 0, '{"a":"eA=="}'::jsonb) from generate_series(1, 99) g $$,
  'up to 100 scopes are accepted');
select lives_ok($$ select public.put_user_config('app', 0, '{}'::jsonb) $$, 'the 100th scope is accepted');
select throws_ok($$ select public.put_user_config('repo:one-too-many', 0, '{}'::jsonb) $$,
  'P0001', 'config_scope_limit', 'a 101st scope is refused');
select lives_ok($$ select public.put_user_config('app', 1, '{"b":"eQ=="}'::jsonb) $$,
  'an existing scope can still be updated at the limit');
select throws_ok($$ select public.put_user_config('repo:' || repeat('x', 300), 0, '{}'::jsonb) $$,
  'P0001', 'config_scope_too_long', 'a scope name over 256 characters is refused');

reset role;
select * from finish();
rollback;
