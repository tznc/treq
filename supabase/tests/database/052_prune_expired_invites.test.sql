-- prune_expired_organization_invites from 074_prune_expired_invites.sql: the
-- daily remote-maintenance run deletes invites that expired without being
-- accepted. Live invites and accepted ones stay.
begin;
select plan(4);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000d1', 'prune-invites-owner@example.com');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select set_config('test.org',
  public.organization_create('00000000-0000-0000-0000-0000000000d1', 'Prune Invites')->>'id', true);
reset role;

insert into public.organization_invites (org_id, email, token_hash, expires_at, accepted_at, revoked_at) values
  (current_setting('test.org')::uuid, 'live@example.com', 'h1', now() + interval '1 hour', null, null),
  (current_setting('test.org')::uuid, 'expired@example.com', 'h2', now() - interval '1 hour', null, null),
  (current_setting('test.org')::uuid, 'revoked-expired@example.com', 'h3', now() - interval '2 days', null, now() - interval '3 days'),
  (current_setting('test.org')::uuid, 'accepted@example.com', 'h4', now() - interval '2 days', now() - interval '3 days', null);

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.prune_expired_organization_invites(), 2::bigint,
  'prune deletes the two invites that expired unaccepted');
reset role;

select results_eq(
  $$select email from public.organization_invites
    where org_id = current_setting('test.org')::uuid order by email$$,
  $$values ('accepted@example.com'::text), ('live@example.com'::text)$$,
  'live and accepted invites stay');

select function_privs_are('public', 'prune_expired_organization_invites', array[]::text[], 'authenticated',
  array[]::text[], 'authenticated cannot prune invites');
select function_privs_are('public', 'prune_expired_organization_invites', array[]::text[], 'anon',
  array[]::text[], 'anon cannot prune invites');

select * from finish();
rollback;
