-- billing_attach_customer from 026_billing_checkout.sql: billing-checkout
-- records the Stripe customer it created for a user before it opens
-- Checkout. An owner keeps its first customer, so two checkouts racing for
-- the same user end up on one customer.
begin;
select plan(11);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000c1', 'attach-a@example.com'),
  ('00000000-0000-0000-0000-0000000000c2', 'attach-b@example.com');

select function_privs_are('public', 'billing_attach_customer', array['text', 'uuid', 'text'],
  'anon', array[]::text[], 'anon cannot execute billing_attach_customer');
select function_privs_are('public', 'billing_attach_customer', array['text', 'uuid', 'text'],
  'authenticated', array[]::text[], 'authenticated cannot execute billing_attach_customer');
select function_privs_are('public', 'billing_attach_customer', array['text', 'uuid', 'text'],
  'service_role', array['EXECUTE'], 'service_role can execute billing_attach_customer');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);

select is(
  public.billing_attach_customer('user', '00000000-0000-0000-0000-0000000000c1', 'cus_attach_first'),
  'cus_attach_first', 'the first customer for an owner is recorded');
select is(
  public.billing_attach_customer('user', '00000000-0000-0000-0000-0000000000c1', 'cus_attach_second'),
  'cus_attach_first', 'a second customer for the same owner returns the first one');
select is((select count(*)::int from public.billing_customers
           where owner_id = '00000000-0000-0000-0000-0000000000c1'), 1,
  'the owner still has one customer');
select is(
  public.billing_attach_customer('user', '00000000-0000-0000-0000-0000000000c1', 'cus_attach_first'),
  'cus_attach_first', 'attaching the same customer again is a no-op');

select throws_ok(
  $$select public.billing_attach_customer('user', '00000000-0000-0000-0000-0000000000c2', 'cus_attach_first')$$,
  'P0001', null,
  'a customer that belongs to another owner cannot be attached');
select throws_ok(
  $$select public.billing_attach_customer('user', '00000000-0000-0000-0000-00000000dead', 'cus_attach_ghost')$$,
  'P0001', null,
  'a customer cannot be attached to a user that does not exist');
select throws_ok(
  $$select public.billing_attach_customer('team', '00000000-0000-0000-0000-0000000000c2', 'cus_attach_org')$$,
  '22023', null,
  'an owner type other than user or organization is rejected');

-- The webhook confirms the mapping billing-checkout wrote.
select is(
  public.billing_record_checkout_completed('evt_attach_confirm', 'cus_attach_first', 'user',
    '00000000-0000-0000-0000-0000000000c1'),
  'applied', 'checkout.session.completed confirms an attached customer');

reset role;

select * from finish();
rollback;
