-- Owners from 029_organization_owners.sql: promoting a member, demoting an
-- owner (never the last one), deleting an organization (refused while it has
-- Team), and the account-deletion trigger that keeps every organization
-- with an owner or deletes it when nobody is left.
begin;
select plan(51);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000a1', 'own-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000a2', 'own-first-member@example.com'),
  ('00000000-0000-0000-0000-0000000000a3', 'own-second-member@example.com'),
  ('00000000-0000-0000-0000-0000000000a4', 'own-outsider@example.com'),
  ('00000000-0000-0000-0000-0000000000b1', 'team-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000b2', 'team-co-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000b3', 'team-member@example.com'),
  ('00000000-0000-0000-0000-0000000000c1', 'leaving-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000c2', 'longest-member@example.com'),
  ('00000000-0000-0000-0000-0000000000c3', 'newer-member@example.com'),
  ('00000000-0000-0000-0000-0000000000d1', 'alone-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000e1', 'pair-owner-1@example.com'),
  ('00000000-0000-0000-0000-0000000000e2', 'pair-owner-2@example.com'),
  ('00000000-0000-0000-0000-0000000000e3', 'pair-member@example.com');

-- ── Shape and grants ──────────────────────────────────────────────────────
select is(
  (select count(*)::int from unnest(array[
    'public.organization_promote_member(uuid,uuid,uuid)',
    'public.organization_demote_owner(uuid,uuid,uuid)',
    'public.organization_delete(uuid,uuid)'
  ]::regprocedure[]) f
   where has_function_privilege('anon', f, 'execute')
      or has_function_privilege('authenticated', f, 'execute')),
  0, 'no client role can promote, demote or delete');
select is(
  (select count(*)::int from unnest(array[
    'public.organization_promote_member(uuid,uuid,uuid)',
    'public.organization_demote_owner(uuid,uuid,uuid)',
    'public.organization_delete(uuid,uuid)'
  ]::regprocedure[]) f
   where has_function_privilege('service_role', f, 'execute')),
  3, 'the service role can promote, demote and delete');
select is(
  (select count(*)::int from unnest(array[
    'treq_internal.organizations_on_account_delete()'
  ]::regprocedure[]) f
   where has_function_privilege('anon', f, 'execute')
      or has_function_privilege('authenticated', f, 'execute')
      or has_function_privilege('service_role', f, 'execute')),
  0, 'nobody can call the account-deletion trigger function directly');
select trigger_is('auth', 'users', 'on_auth_user_deleted_organizations',
  'treq_internal', 'organizations_on_account_delete',
  'deleting an account runs the organization hand-over');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

-- ── Promote ───────────────────────────────────────────────────────────────
select set_config('test.org',
  public.organization_create('00000000-0000-0000-0000-0000000000a1', 'Owners Co')->>'id', true);
insert into public.organization_members (org_id, user_id, role, created_at) values
  (current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a2', 'member', now() + interval '1 minute'),
  (current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a3', 'member', now() + interval '2 minutes');

select throws_ok(
  $$select public.organization_promote_member('00000000-0000-0000-0000-0000000000a2',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a3')$$,
  'PT403', null, 'a member cannot promote another member');
select throws_ok(
  $$select public.organization_promote_member('00000000-0000-0000-0000-0000000000a2',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a2')$$,
  'PT403', null, 'a member cannot promote themselves');
select throws_ok(
  $$select public.organization_promote_member('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a4')$$,
  'PT404', null, 'an owner cannot promote someone outside the organization');
select is(
  public.organization_promote_member('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a2'),
  'promoted', 'an owner promotes a member');
select is((select role from public.organization_members
           where org_id = current_setting('test.org')::uuid
             and user_id = '00000000-0000-0000-0000-0000000000a2'),
  'owner', 'the promoted member is an owner');
select is(
  public.organization_promote_member('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a2'),
  'already_owner', 'promoting an owner changes nothing');

-- ── Demote ────────────────────────────────────────────────────────────────
-- a2 linked an installation the organization owns. Writes to it follow
-- linked_user_id, so it must stay with an owner.
insert into public.github_app_installations
  (id, account_login, account_type, app_id, linked_user_id, organization_id) values
  (9701, 'owners-gh', 'Organization', 1, '00000000-0000-0000-0000-0000000000a2',
   current_setting('test.org')::uuid);

select throws_ok(
  $$select public.organization_demote_owner('00000000-0000-0000-0000-0000000000a3',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a1')$$,
  'PT403', null, 'a member cannot demote an owner');
select is(
  public.organization_demote_owner('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a2'),
  'demoted', 'an owner demotes another owner');
select results_eq(
  $$select user_id, role from public.organization_members
    where org_id = current_setting('test.org')::uuid order by created_at$$,
  $$values ('00000000-0000-0000-0000-0000000000a1'::uuid, 'owner'::text),
           ('00000000-0000-0000-0000-0000000000a2'::uuid, 'member'::text),
           ('00000000-0000-0000-0000-0000000000a3'::uuid, 'member'::text)$$,
  'the demoted owner is a member again');
select is((select linked_user_id from public.github_app_installations where id = 9701),
  '00000000-0000-0000-0000-0000000000a1'::uuid,
  'an installation the demoted owner linked passes to the owner who demoted them');
select is(
  public.organization_demote_owner('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a3'),
  'already_member', 'demoting a member changes nothing');
select throws_ok(
  $$select public.organization_demote_owner('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a1')$$,
  'PT409', null, 'the last owner cannot demote themselves');
select throws_ok(
  $$select public.organization_demote_owner('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a4')$$,
  'PT404', null, 'demoting someone outside the organization is refused');
select is((select count(*)::int from public.organization_members
           where org_id = current_setting('test.org')::uuid and role = 'owner'),
  1, 'the organization keeps its one owner');

-- Transferring ownership: promote, then step down (or leave).
select is(
  public.organization_promote_member('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a3'),
  'promoted', 'the owner promotes a successor');
select is(
  public.organization_demote_owner('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a1'),
  'demoted', 'with another owner in place, an owner can step down');
select is((select linked_user_id from public.github_app_installations where id = 9701),
  '00000000-0000-0000-0000-0000000000a3'::uuid,
  'stepping down hands their installations to the remaining owner');
select throws_ok(
  $$select public.organization_demote_owner('00000000-0000-0000-0000-0000000000a1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a3')$$,
  'PT403', null, 'a former owner can no longer demote anyone');
select throws_ok(
  $$select public.organization_demote_owner('00000000-0000-0000-0000-0000000000a3',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000a3')$$,
  'PT409', null, 'the new sole owner cannot demote themselves either');

-- ── Delete ────────────────────────────────────────────────────────────────
select set_config('test.team',
  public.organization_create('00000000-0000-0000-0000-0000000000b1', 'Team Co')->>'id', true);
insert into public.organization_members (org_id, user_id, role) values
  (current_setting('test.team')::uuid, '00000000-0000-0000-0000-0000000000b2', 'owner'),
  (current_setting('test.team')::uuid, '00000000-0000-0000-0000-0000000000b3', 'member');
select public.organization_invite('00000000-0000-0000-0000-0000000000b1',
  current_setting('test.team')::uuid, 'pending@example.com');
-- b2 linked the organization's installation; a personal one of b1 stays put.
insert into public.github_app_installations
  (id, account_login, account_type, app_id, linked_user_id, organization_id) values
  (9702, 'team-gh', 'Organization', 1, '00000000-0000-0000-0000-0000000000b2',
   current_setting('test.team')::uuid),
  (9703, 'b1-personal', 'User', 1, '00000000-0000-0000-0000-0000000000b1', null);
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status,
   current_period_end, cancel_at_period_end, stripe_event_created)
values
  ('sub_owners_team', 'cus_owners_team', 'organization', current_setting('test.team')::uuid,
   'team', 'active', now() + interval '20 days', false, now());

select throws_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000b3',
    current_setting('test.team')::uuid)$$,
  'PT403', null, 'a member cannot delete the organization');
select throws_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000a4',
    current_setting('test.team')::uuid)$$,
  'PT403', null, 'an outsider cannot delete the organization');
select throws_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000b1',
    current_setting('test.team')::uuid)$$,
  'PT409',
  'This organization has a Team subscription. Cancel it with Manage billing, then delete the organization once the subscription has ended.',
  'an owner cannot delete an organization with an active Team');
update public.billing_subscriptions set cancel_at_period_end = true
where stripe_subscription_id = 'sub_owners_team';
select throws_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000b1',
    current_setting('test.team')::uuid)$$,
  'PT409', null, 'a Team set to cancel still counts until it ends');
update public.billing_subscriptions set status = 'past_due', cancel_at_period_end = false
where stripe_subscription_id = 'sub_owners_team';
select throws_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000b1',
    current_setting('test.team')::uuid)$$,
  'PT409', null, 'a past_due Team counts too');
update public.billing_subscriptions set status = 'trialing'
where stripe_subscription_id = 'sub_owners_team';
select throws_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000b1',
    current_setting('test.team')::uuid)$$,
  'PT409', null, 'a trialing Team counts too');
select is((select count(*)::int from public.organizations where id = current_setting('test.team')::uuid),
  1, 'the refused organization is still there');

update public.billing_subscriptions set status = 'canceled'
where stripe_subscription_id = 'sub_owners_team';
select lives_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000b1',
    current_setting('test.team')::uuid)$$,
  'once the Team has ended, an owner deletes the organization');
select is((select count(*)::int from public.organizations where id = current_setting('test.team')::uuid),
  0, 'the organization is gone');
select is((select count(*)::int from public.organization_members
           where org_id = current_setting('test.team')::uuid),
  0, 'its memberships are gone');
select is((select count(*)::int from public.organization_invites
           where org_id = current_setting('test.team')::uuid),
  0, 'its invites are gone');
select results_eq(
  $$select organization_id, linked_user_id from public.github_app_installations where id = 9702$$,
  $$values (null::uuid, '00000000-0000-0000-0000-0000000000b1'::uuid)$$,
  'its installation is detached and linked to the owner who deleted it');
select is((select linked_user_id from public.github_app_installations where id = 9703),
  '00000000-0000-0000-0000-0000000000b1'::uuid,
  'installations outside the organization are untouched');
select throws_ok(
  $$select public.organization_delete('00000000-0000-0000-0000-0000000000b1',
    current_setting('test.team')::uuid)$$,
  'PT403', null, 'a deleted organization cannot be deleted again');

-- ── Account deletion: the longest-standing member takes over ─────────────
select set_config('test.handover',
  public.organization_create('00000000-0000-0000-0000-0000000000c1', 'Handover Co')->>'id', true);
insert into public.organization_members (org_id, user_id, role, created_at) values
  (current_setting('test.handover')::uuid, '00000000-0000-0000-0000-0000000000c3', 'member', now() + interval '2 days'),
  (current_setting('test.handover')::uuid, '00000000-0000-0000-0000-0000000000c2', 'member', now() + interval '1 day');
insert into public.github_app_installations
  (id, account_login, account_type, app_id, linked_user_id, organization_id) values
  (9704, 'handover-gh', 'Organization', 1, '00000000-0000-0000-0000-0000000000c1',
   current_setting('test.handover')::uuid);
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status,
   current_period_end, cancel_at_period_end, stripe_event_created)
values
  ('sub_handover_team', 'cus_handover_team', 'organization', current_setting('test.handover')::uuid,
   'team', 'active', now() + interval '20 days', false, now());

-- Alone in its organization, with a pending invite and an installation.
select set_config('test.alone',
  public.organization_create('00000000-0000-0000-0000-0000000000d1', 'Alone Co')->>'id', true);
select public.organization_invite('00000000-0000-0000-0000-0000000000d1',
  current_setting('test.alone')::uuid, 'never-joined@example.com');
insert into public.github_app_installations
  (id, account_login, account_type, app_id, linked_user_id, organization_id) values
  (9705, 'alone-gh', 'Organization', 1, '00000000-0000-0000-0000-0000000000d1',
   current_setting('test.alone')::uuid);

-- Two owners and a member.
select set_config('test.pair',
  public.organization_create('00000000-0000-0000-0000-0000000000e1', 'Pair Co')->>'id', true);
insert into public.organization_members (org_id, user_id, role, created_at) values
  (current_setting('test.pair')::uuid, '00000000-0000-0000-0000-0000000000e3', 'member', now() + interval '1 day'),
  (current_setting('test.pair')::uuid, '00000000-0000-0000-0000-0000000000e2', 'owner', now() + interval '2 days');

reset role;
delete from auth.users where id in (
  '00000000-0000-0000-0000-0000000000c1',
  '00000000-0000-0000-0000-0000000000d1',
  '00000000-0000-0000-0000-0000000000e1'
);
set local role service_role;

select results_eq(
  $$select user_id, role from public.organization_members
    where org_id = current_setting('test.handover')::uuid order by created_at$$,
  $$values ('00000000-0000-0000-0000-0000000000c2'::uuid, 'owner'::text),
           ('00000000-0000-0000-0000-0000000000c3'::uuid, 'member'::text)$$,
  'when the last owner deletes their account, the longest-standing member becomes owner');
select results_eq(
  $$select organization_id, linked_user_id from public.github_app_installations where id = 9704$$,
  $$values (current_setting('test.handover')::uuid, '00000000-0000-0000-0000-0000000000c2'::uuid)$$,
  'the installation the deleted owner linked stays with the organization, linked to the new owner');
select ok(public.has_pro('00000000-0000-0000-0000-0000000000c3'),
  'the remaining members keep Team');
select is(
  public.organization_invite('00000000-0000-0000-0000-0000000000c2',
    current_setting('test.handover')::uuid, 'after-handover@example.com')->>'email',
  'after-handover@example.com', 'the new owner can run the organization');

select is((select count(*)::int from public.organizations where id = current_setting('test.alone')::uuid),
  0, 'an organization left with no members is deleted');
select is((select count(*)::int from public.organization_invites
           where org_id = current_setting('test.alone')::uuid),
  0, 'its pending invites go with it');
select results_eq(
  $$select organization_id, linked_user_id from public.github_app_installations where id = 9705$$,
  $$values (null::uuid, null::uuid)$$,
  'its installation is detached and no longer linked to anyone');

select results_eq(
  $$select user_id, role from public.organization_members
    where org_id = current_setting('test.pair')::uuid order by created_at$$,
  $$values ('00000000-0000-0000-0000-0000000000e3'::uuid, 'member'::text),
           ('00000000-0000-0000-0000-0000000000e2'::uuid, 'owner'::text)$$,
  'with another owner left, nobody is promoted');
select is((select count(*)::int from public.organizations
           where id in (current_setting('test.handover')::uuid, current_setting('test.pair')::uuid)),
  2, 'organizations with members left survive');

-- A member deleting their account changes no roles.
reset role;
delete from auth.users where id = '00000000-0000-0000-0000-0000000000e3';
set local role service_role;
select results_eq(
  $$select user_id, role from public.organization_members
    where org_id = current_setting('test.pair')::uuid$$,
  $$values ('00000000-0000-0000-0000-0000000000e2'::uuid, 'owner'::text)$$,
  'a member deleting their account just leaves');

-- Deleting the last member of an organization that still has Team deletes it
-- too; the Stripe subscription is not touched here.
reset role;
delete from auth.users where id in (
  '00000000-0000-0000-0000-0000000000c2',
  '00000000-0000-0000-0000-0000000000c3'
);
set local role service_role;
select is((select count(*)::int from public.organizations where id = current_setting('test.handover')::uuid),
  0, 'when every member is gone, the organization is deleted');
select results_eq(
  $$select organization_id, linked_user_id from public.github_app_installations where id = 9704$$,
  $$values (null::uuid, null::uuid)$$,
  'and its installation is detached');

-- Deleting an account in one statement with every other member works too.
select set_config('test.together',
  public.organization_create('00000000-0000-0000-0000-0000000000a4', 'Together Co')->>'id', true);
insert into public.organization_members (org_id, user_id, role) values
  (current_setting('test.together')::uuid, '00000000-0000-0000-0000-0000000000b3', 'member');
reset role;
select lives_ok(
  $$delete from auth.users where id in (
    '00000000-0000-0000-0000-0000000000a4', '00000000-0000-0000-0000-0000000000b3')$$,
  'deleting the owner and the member together succeeds');
set local role service_role;
select is((select count(*)::int from public.organizations where id = current_setting('test.together')::uuid),
  0, 'and leaves no organization behind');

select * from finish();
rollback;
