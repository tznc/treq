-- 031_alpha_waitlist.sql: a signed-in user reads, joins, leaves, and rejoins
-- only their own waitlist row. They never see or change the unsubscribe
-- token, and the consent and unsubscribe times come from the database
-- clock, not the browser.
begin;
select plan(25);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000a1', 'alpha-a@example.com'),
  ('00000000-0000-0000-0000-0000000000b1', 'alpha-b@example.com'),
  ('00000000-0000-0000-0000-0000000000c1', 'alpha-c@example.com');

-- User B is already on the waitlist, inserted as postgres.
insert into public.alpha_waitlist (user_id, form_version, source_page, unsubscribe_token) values
  ('00000000-0000-0000-0000-0000000000b1', 'v-test', '/roadmap', '40000000-0000-0000-0000-0000000000b1');

-- ── Table setup and privileges ────────────────────────────────────────────
select ok((select relrowsecurity from pg_class where oid = 'public.alpha_waitlist'::regclass),
  'RLS enabled on public.alpha_waitlist');
select table_privs_are('public', 'alpha_waitlist', 'anon', array[]::text[],
  'anon has no privileges on alpha_waitlist');
select table_privs_are('public', 'alpha_waitlist', 'authenticated', array[]::text[],
  'authenticated has no table-wide privileges on alpha_waitlist (column grants only)');
select ok(not has_column_privilege('anon', 'public.alpha_waitlist', 'user_id', 'SELECT'),
  'anon cannot read alpha_waitlist');
select ok(not has_column_privilege('authenticated', 'public.alpha_waitlist', 'unsubscribe_token', 'SELECT, INSERT, UPDATE'),
  'authenticated has no privileges on unsubscribe_token');
select ok(not has_column_privilege('authenticated', 'public.alpha_waitlist', 'consented_at', 'INSERT, UPDATE'),
  'authenticated cannot write consented_at directly');
select ok(not has_table_privilege('authenticated', 'public.alpha_waitlist', 'DELETE'),
  'authenticated cannot delete waitlist rows (leaving sets unsubscribed_at)');
select ok(has_table_privilege('service_role', 'public.alpha_waitlist', 'SELECT, INSERT, UPDATE'),
  'service_role keeps access for the unsubscribe function and metrics');

-- ── As user A ─────────────────────────────────────────────────────────────
set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a1","role":"authenticated"}', true);

select is_empty(
  $$select user_id from public.alpha_waitlist$$,
  'user A sees no rows before joining, not even user B''s');

select throws_ok(
  $$insert into public.alpha_waitlist (user_id, form_version, source_page)
    values ('00000000-0000-0000-0000-0000000000b1', 'v-test', '/x')
    on conflict (user_id) do update set source_page = excluded.source_page$$,
  '42501', null,
  'user A cannot insert or upsert a row for user B');

select throws_ok(
  $$insert into public.alpha_waitlist (user_id, form_version, unsubscribe_token)
    values ('00000000-0000-0000-0000-0000000000a1', 'v-test', gen_random_uuid())$$,
  '42501', null,
  'user A cannot choose their own unsubscribe token');

select lives_ok(
  $$insert into public.alpha_waitlist (user_id, form_version, source_page, unsubscribed_at)
    values ('00000000-0000-0000-0000-0000000000a1', '2026-10-alpha-v1', '/dashboard', null)
    on conflict (user_id) do update set
      form_version = excluded.form_version,
      source_page = excluded.source_page,
      unsubscribed_at = excluded.unsubscribed_at$$,
  'user A can join (upsert) their own row');

select results_eq(
  $$select user_id, form_version, source_page, unsubscribed_at is null, consented_at = now()
    from public.alpha_waitlist$$,
  $$values ('00000000-0000-0000-0000-0000000000a1'::uuid, '2026-10-alpha-v1'::text, '/dashboard'::text, true, true)$$,
  'user A reads exactly their own row, with consent stamped by the database');

select throws_ok(
  $$select unsubscribe_token from public.alpha_waitlist$$,
  '42501', null,
  'user A cannot read their unsubscribe token');

select throws_ok(
  $$update public.alpha_waitlist set unsubscribe_token = gen_random_uuid()
    where user_id = '00000000-0000-0000-0000-0000000000a1'$$,
  '42501', null,
  'user A cannot change their unsubscribe token');

select throws_ok(
  $$update public.alpha_waitlist set consented_at = '2000-01-01'
    where user_id = '00000000-0000-0000-0000-0000000000a1'$$,
  '42501', null,
  'user A cannot rewrite their consent date');

select is_empty(
  $$update public.alpha_waitlist set unsubscribed_at = now()
    where user_id = '00000000-0000-0000-0000-0000000000b1' returning user_id$$,
  'user A cannot unsubscribe user B');

select throws_ok(
  $$update public.alpha_waitlist set user_id = '00000000-0000-0000-0000-0000000000b1'
    where user_id = '00000000-0000-0000-0000-0000000000a1'$$,
  '42501', null,
  'user A cannot move their row onto user B');

-- Leave: the browser's timestamp is replaced with the database clock.
update public.alpha_waitlist set unsubscribed_at = '2000-01-01'
  where user_id = '00000000-0000-0000-0000-0000000000a1';

select is(
  (select unsubscribed_at from public.alpha_waitlist),
  now(),
  'leaving stamps unsubscribed_at with the database clock');

-- ── Rejoin records a new consent ──────────────────────────────────────────
-- Backdate A's consent as postgres so the new stamp is distinguishable
-- inside this single transaction (now() is fixed per transaction).
reset role;
update public.alpha_waitlist set consented_at = '2020-01-01'
  where user_id = '00000000-0000-0000-0000-0000000000a1';

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000a1","role":"authenticated"}', true);

update public.alpha_waitlist set source_page = '/roadmap'
  where user_id = '00000000-0000-0000-0000-0000000000a1';

select is(
  (select consented_at from public.alpha_waitlist),
  '2020-01-01'::timestamptz,
  'an update that does not rejoin keeps the original consent date');

update public.alpha_waitlist set unsubscribed_at = null, form_version = '2026-10-alpha-v1'
  where user_id = '00000000-0000-0000-0000-0000000000a1';

select results_eq(
  $$select unsubscribed_at is null, consented_at = now() from public.alpha_waitlist$$,
  $$values (true, true)$$,
  'rejoining clears unsubscribed_at and records a new consent date');

-- ── As postgres: the token survives every user write ──────────────────────
reset role;

select isnt(
  (select unsubscribe_token from public.alpha_waitlist where user_id = '00000000-0000-0000-0000-0000000000a1'),
  null,
  'user A''s row got a generated unsubscribe token');

select results_eq(
  $$select unsubscribed_at, unsubscribe_token from public.alpha_waitlist
    where user_id = '00000000-0000-0000-0000-0000000000b1'$$,
  $$values (null::timestamptz, '40000000-0000-0000-0000-0000000000b1'::uuid)$$,
  'user B''s row is untouched by user A');

select throws_ok(
  $$insert into public.alpha_waitlist (user_id, form_version, unsubscribe_token)
    values ('00000000-0000-0000-0000-0000000000c1', 'v-test', '40000000-0000-0000-0000-0000000000b1')$$,
  '23505', null,
  'unsubscribe tokens are unique');

select throws_ok(
  $$insert into public.alpha_waitlist (user_id, form_version, source_page)
    values ('00000000-0000-0000-0000-0000000000c1', 'v-test', repeat('/a', 200))$$,
  '23514', null,
  'source_page length is bounded');

select * from finish();
rollback;
