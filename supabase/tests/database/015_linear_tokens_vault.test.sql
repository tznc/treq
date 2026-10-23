-- Linear OAuth tokens live in Vault (032_linear_tokens_vault.sql): the table
-- holds no plaintext, only service_role can read or write them, and a
-- deleted connection takes its secrets along.
begin;
select plan(14);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000d1', 'linear-vault@example.com');

select hasnt_column('public', 'linear_oauth_tokens', 'access_token', 'no plaintext access_token column');
select hasnt_column('public', 'linear_oauth_tokens', 'refresh_token', 'no plaintext refresh_token column');

select function_privs_are('public', 'linear_tokens_load', array['uuid'], 'authenticated', array[]::text[],
  'authenticated cannot load Linear tokens');
select function_privs_are('public', 'linear_tokens_save', array['uuid', 'text', 'text', 'timestamptz', 'text', 'text'],
  'authenticated', array[]::text[], 'authenticated cannot save Linear tokens');
select function_privs_are('public', 'linear_tokens_load', array['uuid'], 'service_role', array['EXECUTE'],
  'service_role can load Linear tokens');

set local role service_role;

select throws_ok(
  $$ select public.linear_tokens_save('00000000-0000-0000-0000-0000000000d1', 'a0', 'r0', null) $$,
  null, null, 'a first save needs the workspace');

select lives_ok(
  $$ select public.linear_tokens_save('00000000-0000-0000-0000-0000000000d1', 'access-1', 'refresh-1',
       now() + interval '1 hour', 'ws', 'Workspace') $$,
  'a new connection saves');

select results_eq(
  $$ select access_token, refresh_token from public.linear_tokens_load('00000000-0000-0000-0000-0000000000d1') $$,
  $$ values ('access-1'::text, 'refresh-1'::text) $$,
  'load decrypts the saved tokens');

select lives_ok(
  $$ select public.linear_tokens_save('00000000-0000-0000-0000-0000000000d1', 'access-2', 'refresh-2', null) $$,
  'a refresh saves without the workspace');

select results_eq(
  $$ select access_token, refresh_token, expires_at from public.linear_tokens_load('00000000-0000-0000-0000-0000000000d1') $$,
  $$ values ('access-2'::text, 'refresh-2'::text, null::timestamptz) $$,
  'a refresh replaces the tokens');

select is(
  (select linear_workspace_name from public.linear_oauth_tokens
   where user_id = '00000000-0000-0000-0000-0000000000d1'),
  'Workspace', 'a refresh keeps the workspace');

select lives_ok(
  $$ select public.linear_tokens_save('00000000-0000-0000-0000-0000000000d1', 'access-3', null, null) $$,
  'a save can drop the refresh token');

reset role;

select set_config('test.secrets',
  (select access_token_secret_id::text from public.linear_oauth_tokens
   where user_id = '00000000-0000-0000-0000-0000000000d1'), true);

select is(
  (select count(*)::int from vault.secrets where description like 'Linear refresh token for 00000000-0000-0000-0000-0000000000d1'),
  0, 'dropping the refresh token deletes its secret');

delete from auth.users where id = '00000000-0000-0000-0000-0000000000d1';

select is(
  (select count(*)::int from vault.secrets where id = current_setting('test.secrets')::uuid),
  0, 'deleting the user deletes the access token secret');

select * from finish();
rollback;
