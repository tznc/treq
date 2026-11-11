-- GitHub App user tokens (077_github_user_tokens.sql) live in Vault, only
-- service_role can read or write them, and a deleted row takes its secrets.
begin;
select plan(9);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000a1', 'gh-tokens@example.com');

select hasnt_column('public', 'github_user_tokens', 'access_token', 'no plaintext access token column');
select function_privs_are('public', 'github_user_tokens_load', array['uuid'], 'authenticated',
  array[]::text[], 'authenticated cannot load GitHub user tokens');
select function_privs_are('public', 'github_user_tokens_save',
  array['uuid', 'text', 'text', 'timestamptz', 'timestamptz'], 'authenticated',
  array[]::text[], 'authenticated cannot save GitHub user tokens');
select table_privs_are('public', 'github_user_tokens', 'authenticated', array[]::text[],
  'authenticated cannot read the token table');

set local role service_role;

select lives_ok(
  $$ select public.github_user_tokens_save('00000000-0000-0000-0000-0000000000a1',
       'ghu_1', 'ghr_1', now() + interval '8 hours', now() + interval '6 months') $$,
  'a first save stores the grant');
select results_eq(
  $$ select access_token, refresh_token
     from public.github_user_tokens_load('00000000-0000-0000-0000-0000000000a1') $$,
  $$ values ('ghu_1'::text, 'ghr_1'::text) $$,
  'load returns the decrypted grant');

select public.github_user_tokens_save('00000000-0000-0000-0000-0000000000a1',
  'ghu_2', 'ghr_2', now() + interval '8 hours', now() + interval '6 months');
select results_eq(
  $$ select access_token, refresh_token
     from public.github_user_tokens_load('00000000-0000-0000-0000-0000000000a1') $$,
  $$ values ('ghu_2'::text, 'ghr_2'::text) $$,
  'a later save replaces both tokens (GitHub rotates refresh tokens)');

reset role;
create temp table secret_ids as
  select access_token_secret_id as id from public.github_user_tokens
  union all select refresh_token_secret_id from public.github_user_tokens;
delete from public.github_user_tokens where user_id = '00000000-0000-0000-0000-0000000000a1';
select is((select count(*)::int from vault.secrets where id in (select id from secret_ids)), 0,
  'deleting the row deletes its Vault secrets');
select is((select count(*)::int from secret_ids), 2, 'both tokens had secrets');

select * from finish();
rollback;
