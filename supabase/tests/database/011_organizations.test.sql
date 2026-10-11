-- Organizations from 028_organizations_team.sql: creating one, invites
-- matched by a single-use token that expires after 7 days, no member cap
-- (030_team_seat_billing.sql), last-owner protection, and row-level
-- security on the new tables.
begin;
select plan(77);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000e1', 'org-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000e2', 'org-member@example.com'),
  ('00000000-0000-0000-0000-0000000000e3', 'org-outsider@example.com'),
  ('00000000-0000-0000-0000-0000000000e4', 'org-late@example.com'),
  ('00000000-0000-0000-0000-0000000000e5', 'org-second-owner@example.com');

-- ── Shape and grants ──────────────────────────────────────────────────────
select has_table('public', 'organizations', 'organizations exists');
select has_table('public', 'organization_members', 'organization_members exists');
select has_table('public', 'organization_invites', 'organization_invites exists');
select ok((select relrowsecurity from pg_class where oid = 'public.organizations'::regclass),
  'RLS enabled on public.organizations');
select ok((select relrowsecurity from pg_class where oid = 'public.organization_members'::regclass),
  'RLS enabled on public.organization_members');
select ok((select relrowsecurity from pg_class where oid = 'public.organization_invites'::regclass),
  'RLS enabled on public.organization_invites');
select has_column('public', 'github_app_installations', 'organization_id',
  'an installation can belong to an organization');
select hasnt_column('public', 'organization_invites', 'token',
  'invites store no plaintext token');

select is(
  (select count(*)::int from unnest(array[
    'public.organization_create(uuid,text)',
    'public.organization_invite(uuid,uuid,text)',
    'public.organization_accept_invite(uuid,text)',
    'public.organization_revoke_invite(uuid,uuid)',
    'public.organization_remove_member(uuid,uuid,uuid)',
    'public.organization_leave(uuid,uuid)',
    'public.organization_attach_installation(uuid,uuid,bigint)',
    'public.github_link_installation(uuid,bigint,text,text,text,integer)',
    'public.organization_has_team(uuid)'
  ]::regprocedure[]) f
   where has_function_privilege('anon', f, 'execute')
      or has_function_privilege('authenticated', f, 'execute')),
  0, 'no client role can call the organization functions');
select is(
  (select count(*)::int from unnest(array[
    'public.organization_create(uuid,text)',
    'public.organization_invite(uuid,uuid,text)',
    'public.organization_accept_invite(uuid,text)',
    'public.organization_revoke_invite(uuid,uuid)',
    'public.organization_remove_member(uuid,uuid,uuid)',
    'public.organization_leave(uuid,uuid)',
    'public.organization_attach_installation(uuid,uuid,bigint)',
    'public.github_link_installation(uuid,bigint,text,text,text,integer)',
    'public.organization_has_team(uuid)'
  ]::regprocedure[]) f
   where has_function_privilege('service_role', f, 'execute')),
  9, 'the service role can call every organization function');
select function_privs_are('public', 'is_organization_member', array['uuid'], 'anon', array[]::text[],
  'anon cannot call is_organization_member');
select function_privs_are('public', 'is_organization_member', array['uuid'], 'authenticated', array['EXECUTE'],
  'RLS policies can call is_organization_member for the signed-in user');
select table_privs_are('public', 'organizations', 'authenticated', array['SELECT'],
  'clients can only read organizations');
select table_privs_are('public', 'organization_members', 'authenticated', array['SELECT'],
  'clients can only read organization_members, so nobody can promote themselves');
select table_privs_are('public', 'organization_invites', 'authenticated', array[]::text[],
  'clients hold no table-wide privilege on organization_invites');
select table_privs_are('public', 'organization_members', 'anon', array[]::text[],
  'anon has no privileges on organization_members');

-- ── Create ────────────────────────────────────────────────────────────────
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select set_config('test.org',
  public.organization_create('00000000-0000-0000-0000-0000000000e1', '  Acme  ')->>'id', true);
select is((select name from public.organizations where id = current_setting('test.org')::uuid), 'Acme',
  'the organization is created with a trimmed name');
select results_eq(
  $$select user_id, role from public.organization_members where org_id = current_setting('test.org')::uuid$$,
  $$values ('00000000-0000-0000-0000-0000000000e1'::uuid, 'owner'::text)$$,
  'the creator is its only member and its owner');
select throws_ok(
  $$select public.organization_create('00000000-0000-0000-0000-0000000000e1', '   ')$$,
  'PT400', null, 'a blank name is refused');

-- ── Invite ────────────────────────────────────────────────────────────────
select set_config('test.invite_e2',
  (public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, ' Mia@Example.com ')::text), true);
select set_config('test.token_e2', current_setting('test.invite_e2')::jsonb->>'token', true);
select ok(current_setting('test.token_e2') ~ '^[0-9a-f]{64}$',
  'an invite returns a 256-bit hex token');
select results_eq(
  $$select email, token_hash, invited_by from public.organization_invites
    where org_id = current_setting('test.org')::uuid$$,
  $$values ('mia@example.com'::text,
            encode(sha256(convert_to(current_setting('test.token_e2'), 'UTF8')), 'hex'),
            '00000000-0000-0000-0000-0000000000e1'::uuid)$$,
  'the invite stores the email for display and only the SHA-256 of the token');
select ok(
  (select expires_at between now() + interval '7 days' - interval '1 minute'
                         and now() + interval '7 days' + interval '1 minute'
   from public.organization_invites where org_id = current_setting('test.org')::uuid),
  'an invite expires 7 days after it is sent');
select throws_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e3',
    current_setting('test.org')::uuid, 'someone@example.com')$$,
  'PT403', null, 'someone outside the organization cannot invite');
select throws_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'not-an-email')$$,
  'PT400', null, 'an invalid email is refused');
select throws_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'org-owner@example.com')$$,
  'PT409', null, 'inviting the address of an existing member is refused');

-- Inviting the same address again issues a new link and retires the old one.
select set_config('test.token_e2_old', current_setting('test.token_e2'), true);
select set_config('test.token_e2',
  public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'mia@example.com')->>'token', true);
select isnt(current_setting('test.token_e2'), current_setting('test.token_e2_old'),
  'inviting the same address again issues a new token');
select is((select count(*)::int from public.organization_invites
           where org_id = current_setting('test.org')::uuid),
  1, 'the new link reuses the pending invite instead of taking another seat');

-- ── Accept ────────────────────────────────────────────────────────────────
select throws_ok(
  $$select public.organization_accept_invite('00000000-0000-0000-0000-0000000000e2',
    current_setting('test.token_e2_old'))$$,
  'PT404', null, 'the retired link no longer works');
select throws_ok(
  $$select public.organization_accept_invite('00000000-0000-0000-0000-0000000000e2', 'not-a-token')$$,
  'PT404', null, 'a malformed token matches nothing');
-- Matched by token, not by email: e2 signs in as org-member@example.com.
select is(
  public.organization_accept_invite('00000000-0000-0000-0000-0000000000e2',
    current_setting('test.token_e2'))->>'result',
  'joined', 'any signed-in account holding the token joins');
select is(
  (select role from public.organization_members
   where org_id = current_setting('test.org')::uuid
     and user_id = '00000000-0000-0000-0000-0000000000e2'),
  'member', 'an invite makes a member, never an owner');
select isnt(
  (select accepted_at from public.organization_invites where org_id = current_setting('test.org')::uuid),
  null, 'the invite is marked accepted');
select throws_ok(
  $$select public.organization_accept_invite('00000000-0000-0000-0000-0000000000e4',
    current_setting('test.token_e2'))$$,
  'PT410', null, 'a token works once');
select is((select count(*)::int from public.organization_members
           where org_id = current_setting('test.org')::uuid
             and user_id = '00000000-0000-0000-0000-0000000000e4'),
  0, 'the second account did not join');

-- An expired invite cannot be accepted.
select set_config('test.token_expired',
  public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'late@example.com')->>'token', true);
reset role;
update public.organization_invites set expires_at = now() - interval '1 second'
  where email = 'late@example.com';
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select throws_ok(
  $$select public.organization_accept_invite('00000000-0000-0000-0000-0000000000e4',
    current_setting('test.token_expired'))$$,
  'PT410', null, 'an expired token is refused');

-- A revoked invite cannot be accepted.
select set_config('test.invite_revoked',
  public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'revoked@example.com')::text, true);
select throws_ok(
  $$select public.organization_revoke_invite('00000000-0000-0000-0000-0000000000e2',
    (current_setting('test.invite_revoked')::jsonb->>'invite_id')::uuid)$$,
  'PT404', null, 'a member cannot revoke an invite');
select lives_ok(
  $$select public.organization_revoke_invite('00000000-0000-0000-0000-0000000000e1',
    (current_setting('test.invite_revoked')::jsonb->>'invite_id')::uuid)$$,
  'the owner revokes an invite');
select throws_ok(
  $$select public.organization_accept_invite('00000000-0000-0000-0000-0000000000e4',
    current_setting('test.invite_revoked')::jsonb->>'token')$$,
  'PT410', null, 'a revoked token is refused');
select throws_ok(
  $$select public.organization_revoke_invite('00000000-0000-0000-0000-0000000000e1',
    (current_setting('test.invite_revoked')::jsonb->>'invite_id')::uuid)$$,
  'PT409', null, 'an invite that is no longer pending cannot be revoked again');

-- ── No member cap: seats beyond 5 are billed, not refused ────────────────
-- Two members (e1, e2) and eight invites, then more past what 028 allowed.
do $$
begin
  for i in 1..8 loop
    perform public.organization_invite('00000000-0000-0000-0000-0000000000e1',
      current_setting('test.org')::uuid, format('seat%s@example.com', i));
  end loop;
end;
$$;
select lives_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'eleventh@example.com')$$,
  'an eleventh seat is invited: Team has no member cap');
select lives_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'seat1@example.com')$$,
  'a new link for a pending invite reuses it');
select set_config('test.token_seat8',
  public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'seat8@example.com')->>'token', true);
select lives_ok(
  $$select public.organization_revoke_invite('00000000-0000-0000-0000-0000000000e1',
    (select id from public.organization_invites where email = 'seat2@example.com'))$$,
  'a pending invite can be revoked');
select lives_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'eleventh@example.com')$$,
  'a pending invite gets a new link');

-- Accepting turns a pending invite into a member.
select is(
  public.organization_accept_invite('00000000-0000-0000-0000-0000000000e4',
    current_setting('test.token_seat8'))->>'result',
  'joined', 'a pending invite is accepted');
select lives_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'twelfth@example.com')$$,
  'inviting still works after the accept');

-- A member who accepts an invite meant for someone else's address uses it up
-- without joining twice.
select is(
  public.organization_accept_invite('00000000-0000-0000-0000-0000000000e2',
    public.organization_invite('00000000-0000-0000-0000-0000000000e1',
      current_setting('test.org')::uuid, 'seat3@example.com')->>'token')->>'result',
  'already_member', 'an existing member accepting an invite does not join twice');
select is((select count(*)::int from public.organization_members
           where org_id = current_setting('test.org')::uuid
             and user_id = '00000000-0000-0000-0000-0000000000e2'),
  1, 'the member still has one membership');
select lives_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, 'twelfth@example.com')$$,
  'a new link for that pending invite still works');

-- ── RLS: members ──────────────────────────────────────────────────────────
reset role;
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e2","role":"authenticated"}', true);

select results_eq($$select name from public.organizations$$, $$values ('Acme'::text)$$,
  'a member reads their organization');
select results_eq(
  $$select user_id, role from public.organization_members order by role desc, user_id$$,
  $$values ('00000000-0000-0000-0000-0000000000e1'::uuid, 'owner'::text),
           ('00000000-0000-0000-0000-0000000000e2'::uuid, 'member'::text),
           ('00000000-0000-0000-0000-0000000000e4'::uuid, 'member'::text)$$,
  'a member reads the member list');
select results_eq(
  $$select email from public.profiles order by email$$,
  $$values ('org-late@example.com'::text), ('org-member@example.com'::text), ('org-owner@example.com'::text)$$,
  'a member reads the profiles of the other members, and no one else''s');
select is((select count(*)::int from public.organization_invites), 0,
  'a member cannot read the invites');
select throws_ok($$select token_hash from public.organization_invites$$, '42501', null,
  'no client can read a token hash');
select throws_ok(
  $$update public.organization_members set role = 'owner'
    where user_id = '00000000-0000-0000-0000-0000000000e2'$$,
  '42501', null, 'a member cannot promote themselves');
select throws_ok(
  $$insert into public.organization_members (org_id, user_id, role)
    values (current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e3', 'owner')$$,
  '42501', null, 'a member cannot add anyone');
select throws_ok(
  $$insert into public.organization_invites (org_id, email, token_hash, expires_at)
    values (current_setting('test.org')::uuid, 'x@example.com', 'x', now() + interval '1 day')$$,
  '42501', null, 'a member cannot write an invite');

-- ── RLS: the owner ────────────────────────────────────────────────────────
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e1","role":"authenticated"}', true);
select is(
  (select count(*)::int from public.organization_invites
   where accepted_at is null and revoked_at is null and expires_at > now()),
  7, 'the owner reads the pending invites');
select lives_ok($$select id, email, expires_at, accepted_at, revoked_at from public.organization_invites$$,
  'the owner reads the invite columns the dashboard needs');

-- ── RLS: an outsider ──────────────────────────────────────────────────────
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e3","role":"authenticated"}', true);
select is((select count(*)::int from public.organizations), 0, 'an outsider reads no organization');
select is((select count(*)::int from public.organization_members), 0, 'an outsider reads no members');
select is((select count(*)::int from public.organization_invites), 0, 'an outsider reads no invites');
select results_eq($$select email from public.profiles$$, $$values ('org-outsider@example.com'::text)$$,
  'an outsider reads only their own profile');
select is(public.is_organization_member(current_setting('test.org')::uuid), false,
  'is_organization_member answers only for the signed-in user');

-- ── Remove, leave, last owner ─────────────────────────────────────────────
reset role;
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select throws_ok(
  $$select public.organization_remove_member('00000000-0000-0000-0000-0000000000e2',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e4')$$,
  'PT403', null, 'a member cannot remove another member');
select throws_ok(
  $$select public.organization_remove_member('00000000-0000-0000-0000-0000000000e2',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e1')$$,
  'PT403', null, 'a member cannot remove the owner');
select throws_ok(
  $$select public.organization_leave('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid)$$,
  'PT409', null, 'the only owner cannot leave');
select throws_ok(
  $$select public.organization_remove_member('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e1')$$,
  'PT409', null, 'the only owner cannot remove themselves');
select throws_ok(
  $$select public.organization_remove_member('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e3')$$,
  'PT404', null, 'removing someone who is not a member is refused');
select lives_ok(
  $$select public.organization_remove_member('00000000-0000-0000-0000-0000000000e1',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e4')$$,
  'the owner removes a member');
select lives_ok(
  $$select public.organization_leave('00000000-0000-0000-0000-0000000000e2',
    current_setting('test.org')::uuid)$$,
  'a member leaves');
select throws_ok(
  $$select public.organization_leave('00000000-0000-0000-0000-0000000000e2',
    current_setting('test.org')::uuid)$$,
  'PT404', null, 'leaving twice is refused');
select results_eq(
  $$select user_id from public.organization_members where org_id = current_setting('test.org')::uuid$$,
  $$values ('00000000-0000-0000-0000-0000000000e1'::uuid)$$,
  'only the owner is left');

-- With a second owner, either owner can leave or be removed.
reset role;
insert into public.organization_members (org_id, user_id, role)
  values (current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e5', 'owner');
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select lives_ok(
  $$select public.organization_remove_member('00000000-0000-0000-0000-0000000000e5',
    current_setting('test.org')::uuid, '00000000-0000-0000-0000-0000000000e1')$$,
  'an owner can remove another owner while one remains');
select throws_ok(
  $$select public.organization_leave('00000000-0000-0000-0000-0000000000e5',
    current_setting('test.org')::uuid)$$,
  'PT409', null, 'the remaining owner is now the last one and cannot leave');
select is((select count(*)::int from public.organization_members
           where org_id = current_setting('test.org')::uuid and role = 'owner'),
  1, 'the organization always keeps an owner');

-- Inviting still works after removal and leaving.
select is(
  (select count(*)::int from public.organization_invites
   where org_id = current_setting('test.org')::uuid
     and accepted_at is null and revoked_at is null and expires_at > now()),
  7, 'removing and leaving leave the pending invites alone');
select lives_ok(
  $$select public.organization_invite('00000000-0000-0000-0000-0000000000e5',
    current_setting('test.org')::uuid, 'after-leave@example.com')$$,
  'inviting works after members leave');

reset role;

select * from finish();
rollback;
