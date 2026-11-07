-- GitHub identities (072_github_user_identities.sql) are service_role only.
begin;
select plan(3);

select ok(
  (select relrowsecurity from pg_class where oid = 'public.github_user_identities'::regclass),
  'github_user_identities has RLS on');
select table_privs_are('public', 'github_user_identities', 'authenticated', array[]::text[],
  'authenticated cannot touch GitHub identities');
select table_privs_are('public', 'github_user_identities', 'anon', array[]::text[],
  'anon cannot touch GitHub identities');

select * from finish();
rollback;
