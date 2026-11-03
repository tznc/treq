-- BYOS: two first saves of one kind serialize instead of failing on
-- Vault's unique secret name.
begin;
select plan(1);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000035b1', 'chaos-a@example.com');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, stripe_event_created)
values
  ('sub_chaos_a', 'cus_chaos_a', 'user', '00000000-0000-0000-0000-0000000035b1', 'pro', 'active', now());

-- Two first saves for one kind both found no row and both inserted; the
-- loser failed on Vault's unique secret name. Saves serialize per owner+kind.
select ok(
  pg_get_functiondef('public.provider_credential_save(uuid,text,text,text)'::regprocedure)
    ~* 'pg_advisory_xact_lock',
  'saving a key serializes on the owner and kind before looking for the row');

select * from finish();
rollback;
