-- Integration proxying through the Data API (072_integration_proxy.sql):
-- who may call, sandbox JWTs, and the checks that run before any outbound
-- call. Outbound calls themselves are not exercised here.
begin;
select plan(25);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000e1', 'integrations@example.com'),
  ('00000000-0000-0000-0000-0000000000e2', 'other@example.com');

insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status,
   tier_id, machine_type_id)
values
  ('00000000-0000-0000-0000-00000000e0a1', '00000000-0000-0000-0000-0000000000e1',
   'local', 'dev-treq-e1', 'us_east', 'small', 'ready', 's', 'local:s');

insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region,
   provider_resource_id, state, role)
values
  ('00000000-0000-0000-0000-00000000e0b1', '00000000-0000-0000-0000-00000000e0a1',
   '00000000-0000-0000-0000-0000000000e1', 'local', 'local:s', 'us_east',
   'dev-treq-e1', 'ready', 'active');

select vault.create_secret('test-jwt-secret-with-at-least-32-characters', 'project_jwt_secret');

-- ── Privileges ──────────────────────────────────────────────────────────

select function_privs_are('public', 'linear_graphql', array['text', 'jsonb'], 'authenticated',
  array['EXECUTE'], 'authenticated can call linear_graphql');
select function_privs_are('public', 'github_repo_token', array['text'], 'authenticated',
  array['EXECUTE'], 'authenticated can call github_repo_token');
select function_privs_are('public', 'integration_sandbox_token', array['uuid'], 'authenticated',
  array['EXECUTE'], 'authenticated can call integration_sandbox_token');
select function_privs_are('public', 'linear_graphql', array['text', 'jsonb'], 'anon',
  array[]::text[], 'anon cannot call linear_graphql');
select function_privs_are('public', 'integration_vault', array['text'], 'authenticated',
  array[]::text[], 'authenticated cannot read Vault through the helper');
select function_privs_are('public', 'integration_linear_token', array['uuid'], 'authenticated',
  array[]::text[], 'authenticated cannot load Linear tokens through the helper');

-- ── Callers ─────────────────────────────────────────────────────────────

create function pg_temp.as_claims(p_claims jsonb) returns void language sql as $$
  select set_config('request.jwt.claims', p_claims::text, true);
$$;

select pg_temp.as_claims('{"sub":"00000000-0000-0000-0000-0000000000e1","role":"authenticated"}');
select is(public.integration_caller(), '00000000-0000-0000-0000-0000000000e1'::uuid,
  'a user JWT is its user');

select pg_temp.as_claims(jsonb_build_object(
  'sub', '00000000-0000-0000-0000-0000000000e1', 'role', 'authenticated',
  'iat', 1000, 'exp', 4600, 'treq_placement_id', '00000000-0000-0000-0000-00000000e0b1'));
select is(public.integration_caller(), '00000000-0000-0000-0000-0000000000e1'::uuid,
  'a sandbox JWT for a live placement is its owner');

select pg_temp.as_claims(jsonb_build_object(
  'sub', '00000000-0000-0000-0000-0000000000e1', 'role', 'authenticated',
  'iat', 1000, 'exp', 4601, 'treq_placement_id', '00000000-0000-0000-0000-00000000e0b1'));
select throws_ok($$ select public.integration_caller() $$, 'PGRST', null,
  'a sandbox JWT issued for over an hour is refused');

select pg_temp.as_claims(jsonb_build_object(
  'sub', '00000000-0000-0000-0000-0000000000e2', 'role', 'authenticated',
  'iat', 1000, 'exp', 4600, 'treq_placement_id', '00000000-0000-0000-0000-00000000e0b1'));
select throws_ok($$ select public.integration_caller() $$, 'PGRST', null,
  'a sandbox JWT for someone else''s placement is refused');

update public.remote_instance_placements set role = 'dead'
where id = '00000000-0000-0000-0000-00000000e0b1';
select pg_temp.as_claims(jsonb_build_object(
  'sub', '00000000-0000-0000-0000-0000000000e1', 'role', 'authenticated',
  'iat', 1000, 'exp', 4600, 'treq_placement_id', '00000000-0000-0000-0000-00000000e0b1'));
select throws_ok($$ select public.integration_caller() $$, 'PGRST', null,
  'a sandbox JWT dies with its placement');
update public.remote_instance_placements set role = 'active'
where id = '00000000-0000-0000-0000-00000000e0b1';

select pg_temp.as_claims('{}');
select throws_ok($$ select public.integration_caller() $$, 'PGRST', null,
  'no JWT is refused');

-- ── Sandbox JWT minting ─────────────────────────────────────────────────

select pg_temp.as_claims('{"sub":"00000000-0000-0000-0000-0000000000e1","role":"authenticated"}');
create temp table minted as
  select public.integration_sandbox_token('00000000-0000-0000-0000-00000000e0b1') ->> 'token' as token;

select is(
  split_part(token, '.', 3),
  public.integration_b64url(extensions.hmac(
    convert_to(split_part(token, '.', 1) || '.' || split_part(token, '.', 2), 'utf8'),
    convert_to('test-jwt-secret-with-at-least-32-characters', 'utf8'), 'sha256')),
  'the sandbox JWT is HS256 with the project JWT secret')
from minted;

select is(
  convert_from(decode(rpad(translate(split_part(token, '.', 2), '-_', '+/'),
    (length(split_part(token, '.', 2)) + 3) / 4 * 4, '='), 'base64'), 'utf8')::jsonb
    - 'iat' - 'exp',
  jsonb_build_object('sub', '00000000-0000-0000-0000-0000000000e1', 'role', 'authenticated',
    'aud', 'authenticated', 'is_anonymous', false,
    'treq_placement_id', '00000000-0000-0000-0000-00000000e0b1'),
  'the sandbox JWT carries the owner and placement')
from minted;

select throws_ok(
  $$ select public.integration_sandbox_token('00000000-0000-0000-0000-00000000ffff') $$,
  'PGRST', null, 'only the owner''s placements get a sandbox JWT');

select pg_temp.as_claims(jsonb_build_object(
  'sub', '00000000-0000-0000-0000-0000000000e1', 'role', 'authenticated',
  'iat', 1000, 'exp', 4600, 'treq_placement_id', '00000000-0000-0000-0000-00000000e0b1'));
select throws_ok(
  $$ select public.integration_sandbox_token('00000000-0000-0000-0000-00000000e0b1') $$,
  'PGRST', null, 'a sandbox JWT cannot mint another');

-- ── Checks before any outbound call ─────────────────────────────────────

select pg_temp.as_claims('{"sub":"00000000-0000-0000-0000-0000000000e1","role":"authenticated"}');
select throws_ok($$ select public.github_repo_token('not a repo') $$, 'PGRST', null,
  'github_repo_token refuses a malformed repository');
select throws_like($$ select public.github_repo_token('tznc/treq') $$, '%not_installed%',
  'github_repo_token refuses a repository without a linked installation');

-- ── Schedules ───────────────────────────────────────────────────────────

select results_eq(
  $$ select jobname::text, schedule::text from cron.job
     where jobname like 'integration-%' order by jobname $$,
  $$ values ('integration-refresh-github-app-jwt', '*/4 * * * *'),
            ('integration-refresh-linear-tokens', '* * * * *') $$,
  'pg_cron refreshes the GitHub App JWT and Linear tokens');

reset role;
select lives_ok($$ select public.integration_refresh_linear_tokens() $$,
  'the Linear refresh job runs with nothing to refresh');
select function_privs_are('public', 'integration_refresh_linear_tokens', array[]::text[],
  'authenticated', array[]::text[], 'authenticated cannot run the refresh job');

-- Use is recorded, so pg_cron only refreshes what is in use.
select public.linear_tokens_save('00000000-0000-0000-0000-0000000000e1', 'a1', 'r1',
  now() + interval '1 hour', 'ws', 'Workspace');
select public.integration_linear_token('00000000-0000-0000-0000-0000000000e1');
select isnt(
  (select last_used_at from public.linear_oauth_tokens
   where user_id = '00000000-0000-0000-0000-0000000000e1'),
  null, 'using a Linear token records last_used_at');

update public.linear_oauth_tokens set last_used_at = now() - interval '9 minutes'
where user_id = '00000000-0000-0000-0000-0000000000e1';
select public.integration_linear_token('00000000-0000-0000-0000-0000000000e1');
select is(
  (select last_used_at from public.linear_oauth_tokens
   where user_id = '00000000-0000-0000-0000-0000000000e1'),
  now() - interval '9 minutes', 'a use within 10 minutes of the last writes nothing');

update public.linear_oauth_tokens set last_used_at = now() - interval '11 minutes'
where user_id = '00000000-0000-0000-0000-0000000000e1';
select public.integration_linear_token('00000000-0000-0000-0000-0000000000e1');
select is(
  (select last_used_at from public.linear_oauth_tokens
   where user_id = '00000000-0000-0000-0000-0000000000e1'),
  now(), 'a use over 10 minutes after the last records it');

select matches(
  (select command from cron.job where jobname = 'integration-refresh-github-app-jwt'),
  'last_used_at > now\(\) - interval',
  'the App JWT refresh only runs while the JWT is in use');

select * from finish();
rollback;
