-- BYOS: a 401 for a key that was since rotated does not mark the new
-- key invalid.
begin;
select plan(3);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000035b1', 'chaos-a@example.com');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_chaos_a', 'cus_chaos_a', 'user', '00000000-0000-0000-0000-0000000035b1', 'pro', 'active', now());

-- A 401 for the old key arriving after a rotation leaves the new key valid.
select public.provider_credential_save('00000000-0000-0000-0000-0000000035b1', 'daytona', 'tok-old', null);
select public.provider_credential_save('00000000-0000-0000-0000-0000000035b1', 'daytona', 'tok-new', null);
select public.provider_credential_mark_invalid(
  (select id from public.remote_provider_credentials
    where owner_user_id = '00000000-0000-0000-0000-0000000035b1' and provider_kind = 'daytona'),
  'tok-old');
select is(
  (select invalid_at from public.remote_provider_credentials
    where owner_user_id = '00000000-0000-0000-0000-0000000035b1' and provider_kind = 'daytona'),
  null, 'a 401 for a key that was since replaced does not mark the new key invalid');
select public.provider_credential_mark_invalid(
  (select id from public.remote_provider_credentials
    where owner_user_id = '00000000-0000-0000-0000-0000000035b1' and provider_kind = 'daytona'),
  'tok-new');
select isnt(
  (select invalid_at from public.remote_provider_credentials
    where owner_user_id = '00000000-0000-0000-0000-0000000035b1' and provider_kind = 'daytona'),
  null, 'a 401 for the current key marks it invalid');
select ok(
  not has_function_privilege('authenticated',
    'public.provider_credential_mark_invalid(uuid,text)', 'execute'),
  'users cannot mark credentials invalid');

select * from finish();
rollback;
