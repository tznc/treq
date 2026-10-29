-- Spend cap and notifications (042_spend_cap.sql).
begin;
select plan(24);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000e1', 'cap-owner@example.com'),
  ('00000000-0000-0000-0000-0000000000e2', 'cap-other@example.com');

-- ── The cap ─────────────────────────────────────────────────────────────

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e1","role":"authenticated"}', true);
select results_eq(
  $$ select (s->>'cap_usd')::numeric, (s->>'spent_usd')::numeric, s->>'state'
       from public.get_spend_status() s $$,
  $$ values (50::numeric, 0::numeric, 'ok') $$,
  'a user without a cap row has the US$50 default and no spend');
select is((public.get_spend_status()->>'month_start')::timestamptz,
  date_trunc('month', now(), 'UTC'), 'the month starts at UTC midnight on the 1st');
select throws_ok($$ select public.set_spend_cap(4.99) $$, 'P0001', 'spend_cap_too_low',
  'a cap under US$5 is refused');
select throws_ok($$ select public.set_spend_cap(1000.01) $$, 'P0001', 'spend_cap_too_high',
  'a cap over the maximum is refused');
select throws_ok($$ select public.set_spend_cap(null) $$, 'P0001', 'spend_cap_too_low',
  'a null cap is refused');
select is((public.set_spend_cap(75)->>'cap_usd')::numeric, 75::numeric,
  'set_spend_cap returns the new status');
select is((public.get_spend_status()->>'cap_usd')::numeric, 75::numeric, 'the cap is stored');
select throws_ok($$ update public.spend_caps set cap_usd = 1 $$, '42501', null,
  'the cap table cannot be written directly');
reset role;

-- ── Month-to-date spend: finished reports plus the open hour ────────────

insert into public.usage_reports (identifier, owner_user_id, hour, seconds, provider_cost_nanousd, billed_nanousd)
values
  ('e1:this-month', '00000000-0000-0000-0000-0000000000e1', date_trunc('month', now(), 'UTC'),
   3600, 50000000000, 60000000000),
  ('e1:last-month', '00000000-0000-0000-0000-0000000000e1',
   date_trunc('month', now(), 'UTC') - interval '1 hour', 3600, 1000000000000, 1200000000000);

insert into public.remote_instances
  (id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status,
   tier_id, machine_type_id)
values
  ('00000000-0000-0000-0000-00000000c001', '00000000-0000-0000-0000-0000000000e1',
   'boat', 'boat-e1', 'us_east', 'small', 'ready', 's', 'boat:small');
insert into public.remote_instance_placements
  (id, instance_id, owner_user_id, provider_kind, machine_type_id, region,
   provider_resource_id, state, role)
values
  ('00000000-0000-0000-0000-00000000c101', '00000000-0000-0000-0000-00000000c001',
   '00000000-0000-0000-0000-0000000000e1', 'boat', 'boat:small', 'us_east',
   'boat-e1', 'ready', 'active');
-- An open interval in the current hour: 600 s on boat:small at 5000 nUSD/s
-- costs 3,000,000 nUSD and bills 3,600,000 nUSD.
insert into public.usage_intervals
  (placement_id, owner_user_id, kind, price_id, started_at)
select '00000000-0000-0000-0000-00000000c101', '00000000-0000-0000-0000-0000000000e1',
       'running', p.id, now() - interval '600 seconds'
  from public.machine_type_prices p
 where p.machine_type_id = 'boat:small' and p.effective_to is null;

select is(
  treq_internal.month_spend_nanousd('00000000-0000-0000-0000-0000000000e1', now()),
  -- If the month began less than 600 s ago, the part before it is last month's.
  60000000000::bigint + round(extract(epoch from
    now() - greatest(now() - interval '600 seconds', date_trunc('month', now(), 'UTC'))) * 6000)::bigint,
  'spend is this month''s reports plus the unreported open interval at cost x 1.2');

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e1","role":"authenticated"}', true);
select results_eq(
  $$ select s->>'state', (s->>'spent_usd')::numeric between 60 and 60.01
       from public.get_spend_status() s $$,
  $$ values ('warning', true) $$,
  'US$60 of a US$75 cap is a warning (80% or more)');
select set_spend_cap(60);
select is(public.get_spend_status()->>'state', 'capped', 'at or over the cap is capped');
select set_spend_cap(100);
select is(public.get_spend_status()->>'state', 'ok', 'raising the cap unblocks at once');
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e2","role":"authenticated"}', true);
select is((public.get_spend_status()->>'spent_usd')::numeric, 0::numeric,
  'another user sees only their own spend');
select throws_ok(
  $$ select public.get_spend_status_for('00000000-0000-0000-0000-0000000000e1') $$,
  '42501', null, 'users cannot read another user''s status');
reset role;

-- ── Service-role enforcement helpers ────────────────────────────────────

update public.spend_caps set cap_usd = 60 where user_id = '00000000-0000-0000-0000-0000000000e1';
set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(
  public.get_spend_status_for('00000000-0000-0000-0000-0000000000e1')->>'state', 'capped',
  'the service role reads any user''s status');
select results_eq(
  $$ select user_id, state from public.spend_cap_candidates(now()) $$,
  $$ values ('00000000-0000-0000-0000-0000000000e1'::uuid, 'capped') $$,
  'candidates are the users at 80% or more this month');
select is(
  public.record_user_notification('00000000-0000-0000-0000-0000000000e1',
    'spend_cap_reached', '2026-10', 'Spend cap reached', 'Stopped.'),
  true, 'the first notification for a month is stored');
select is(
  public.record_user_notification('00000000-0000-0000-0000-0000000000e1',
    'spend_cap_reached', '2026-10', 'Spend cap reached', 'Stopped.'),
  false, 'the same event in the same month is not stored twice');
select is(
  public.record_user_notification('00000000-0000-0000-0000-0000000000e1',
    'spend_cap_warning', '2026-10', 'Spend at 80%', 'Close.'),
  true, 'the 80% event is separate');
reset role;

set local role authenticated;
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e1","role":"authenticated"}', true);
select throws_ok(
  $$ select public.record_user_notification('00000000-0000-0000-0000-0000000000e1',
       'x', 'y', 't', 'b') $$,
  '42501', null, 'users cannot write notifications');
select is((select count(*)::int from public.list_notifications()), 2,
  'the owner lists their unread notifications');
select public.dismiss_notification((select id from public.list_notifications() limit 1));
select is((select count(*)::int from public.list_notifications()), 1,
  'a dismissed notification is not listed again');
select set_config('request.jwt.claims',
  '{"sub":"00000000-0000-0000-0000-0000000000e2","role":"authenticated"}', true);
select is((select count(*)::int from public.list_notifications()), 0,
  'another user sees none of them');
select is((select count(*)::int from public.user_notifications), 0,
  'row security hides other users'' notifications');
reset role;

select * from finish();
rollback;
