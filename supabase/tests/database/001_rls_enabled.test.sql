-- Every app table must have RLS enabled, so a blanket grant like
-- 008_public_grants.sql (select/insert/update/delete to anon, authenticated)
-- can never be the only thing standing between a client and the data.
begin;
select plan(6);

select ok((select relrowsecurity from pg_class where oid = 'public.profiles'::regclass),
  'RLS enabled on public.profiles');
select ok((select relrowsecurity from pg_class where oid = 'public.desktop_auth_tokens'::regclass),
  'RLS enabled on public.desktop_auth_tokens');
select ok((select relrowsecurity from pg_class where oid = 'public.github_app_installations'::regclass),
  'RLS enabled on public.github_app_installations');
select ok((select relrowsecurity from pg_class where oid = 'public.github_repositories'::regclass),
  'RLS enabled on public.github_repositories');
select ok((select relrowsecurity from pg_class where oid = 'public.github_webhook_receipts'::regclass),
  'RLS enabled on public.github_webhook_receipts');
select ok((select relrowsecurity from pg_class where oid = 'public.github_install_intents'::regclass),
  'RLS enabled on public.github_install_intents');

select * from finish();
rollback;
