-- Admins from 069_organization_admins.sql: an admin manages the team's
-- billing account and invites, and can make members admins. Only owners
-- make owners, and an admin cannot change an owner's role.
begin;
select plan(18);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-00000000ad01', 'adm-owner@example.com'),
  ('00000000-0000-0000-0000-00000000ad02', 'adm-admin@example.com'),
  ('00000000-0000-0000-0000-00000000ad03', 'adm-member@example.com'),
  ('00000000-0000-0000-0000-00000000ad04', 'adm-member2@example.com');

select is(
  (select count(*)::int from unnest(array[
    'public.organization_set_role(uuid,uuid,uuid,text)'
  ]::regprocedure[]) f
   where has_function_privilege('anon', f, 'execute')
      or has_function_privilege('authenticated', f, 'execute')),
  0, 'no client role can set a role');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select set_config('test.org',
  public.organization_create('00000000-0000-0000-0000-00000000ad01', 'Admins Co')->>'id', true);
insert into public.organization_members (org_id, user_id, role) values
  (current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad02', 'member'),
  (current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad03', 'member'),
  (current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad04', 'member');

-- ── Setting roles ────────────────────────────────────────────────────────
select is(
  public.organization_set_role('00000000-0000-0000-0000-00000000ad01',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad02', 'admin'),
  'updated', 'an owner makes a member an admin');
select is(
  public.organization_set_role('00000000-0000-0000-0000-00000000ad01',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad02', 'admin'),
  'unchanged', 'setting the same role again changes nothing');
select is(
  public.organization_set_role('00000000-0000-0000-0000-00000000ad02',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad03', 'admin'),
  'updated', 'an admin makes a member an admin');
select is(
  public.organization_set_role('00000000-0000-0000-0000-00000000ad02',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad03', 'member'),
  'updated', 'an admin makes an admin a member');
select throws_ok(
  $$ select public.organization_set_role('00000000-0000-0000-0000-00000000ad03',
       current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad04', 'admin') $$,
  'PT403', null, 'a member cannot set roles');
select throws_ok(
  $$ select public.organization_set_role('00000000-0000-0000-0000-00000000ad02',
       current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad01', 'member') $$,
  'PT403', null, 'an admin cannot change an owner');
select throws_ok(
  $$ select public.organization_set_role('00000000-0000-0000-0000-00000000ad02',
       current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad04', 'owner') $$,
  'PT400', null, 'set_role never makes an owner: owners use promote');
select throws_ok(
  $$ select public.organization_set_role('00000000-0000-0000-0000-00000000ad01',
       current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad01', 'admin') $$,
  'PT403', null, 'an owner steps down with demote_owner, not set_role');
select lives_ok(
  $$ select public.organization_promote_member('00000000-0000-0000-0000-00000000ad01',
       current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad02') $$,
  'an owner can still promote an admin to owner');
select is(
  public.organization_demote_owner('00000000-0000-0000-0000-00000000ad01',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad02'),
  'demoted', 'and demote them again');
select is(
  public.organization_set_role('00000000-0000-0000-0000-00000000ad01',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-00000000ad02', 'admin'),
  'updated', 'the owner makes them admin again');

-- ── Invites ──────────────────────────────────────────────────────────────
select set_config('test.invite',
  public.organization_invite('00000000-0000-0000-0000-00000000ad02',
    current_setting('test.org')::uuid, 'adm-invitee@example.com')->>'invite_id', true);
select isnt(current_setting('test.invite'), '', 'an admin invites');
select lives_ok(
  $$ select public.organization_revoke_invite('00000000-0000-0000-0000-00000000ad02',
       current_setting('test.invite')::uuid) $$,
  'an admin revokes an invite');
select throws_ok(
  $$ select public.organization_invite('00000000-0000-0000-0000-00000000ad03',
       current_setting('test.org')::uuid, 'nope@example.com') $$,
  'PT403', null, 'a member cannot invite');

-- ── Billing account ──────────────────────────────────────────────────────
reset role;
insert into public.billing_customers (owner_type, owner_id, stripe_customer_id)
values ('organization', current_setting('test.org')::uuid, 'cus_admins_co');

set local role authenticated;
select set_config('request.jwt.claims',
  '{"role":"authenticated","sub":"00000000-0000-0000-0000-00000000ad02"}', true);
select ok(public.can_manage_organization_billing(current_setting('test.org')::uuid),
  'an admin manages the billing account');
select is((select count(*)::int from public.billing_customers
  where owner_id = current_setting('test.org')::uuid), 1,
  'an admin sees the team''s billing customer');

select set_config('request.jwt.claims',
  '{"role":"authenticated","sub":"00000000-0000-0000-0000-00000000ad03"}', true);
select ok(not public.can_manage_organization_billing(current_setting('test.org')::uuid),
  'a member does not manage billing');

select * from finish();
rollback;
