-- One Pro trial per email (061): the claim outlives the account, and the
-- email is normalised so +tags and Gmail dots do not make a new one.
begin;
select plan(7);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000041a1', 'Trial.User+one@gmail.com'),
  ('00000000-0000-0000-0000-0000000041a2', 'someone-else@example.com');
insert into public.billing_customers (stripe_customer_id, owner_type, owner_id)
values ('cus_trial_a', 'user', '00000000-0000-0000-0000-0000000041a1');

select is(public.billing_trial_claimed('00000000-0000-0000-0000-0000000041a1'), false, 'no claim before a trial');
update public.billing_customers set trial_used_at = now() where stripe_customer_id = 'cus_trial_a';
select is(public.billing_trial_claimed('00000000-0000-0000-0000-0000000041a1'), true, 'using the trial records a claim');
select is((select count(*)::int from public.billing_trial_claims where email_key like '%@%'), 0,
  'the claim stores a hash, not the email');

delete from auth.users where id = '00000000-0000-0000-0000-0000000041a1';
insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000041a3', 'trialuser@googlemail.com'),
  ('00000000-0000-0000-0000-0000000041a4', 'TRIAL.USER+two@gmail.com');
select is(public.billing_trial_claimed('00000000-0000-0000-0000-0000000041a4'), true,
  'a re-signup with another +tag and case has no trial');
select is(public.billing_trial_claimed('00000000-0000-0000-0000-0000000041a3'), true,
  'googlemail and dots count as the same Gmail address');
select is(public.billing_trial_claimed('00000000-0000-0000-0000-0000000041a2'), false,
  'another email still has its trial');
select ok(not has_function_privilege('authenticated', 'public.billing_trial_claimed(uuid)', 'execute'),
  'only the service role asks');

select * from finish();
rollback;
