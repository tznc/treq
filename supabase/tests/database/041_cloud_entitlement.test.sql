-- Cloud workspaces need a personal Pro subscription (058). Cloud usage is
-- metered on the owner's own Stripe customer; a Team member without one
-- would run usage nobody is billed for.
begin;
select plan(8);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000038a1', 'cloud-pro@example.com'),
  ('00000000-0000-0000-0000-0000000038a2', 'cloud-team@example.com');
insert into public.organizations (id, name) values ('00000000-0000-0000-0000-0000000038f1', 'Cloud Team');
insert into public.organization_members (org_id, user_id, role)
  values ('00000000-0000-0000-0000-0000000038f1', '00000000-0000-0000-0000-0000000038a2', 'owner');
insert into public.billing_subscriptions
  (stripe_subscription_id, stripe_customer_id, owner_type, owner_id, plan, status, current_period_end, stripe_event_created)
values
  ('sub_cloud_pro', 'cus_cloud_pro', 'user', '00000000-0000-0000-0000-0000000038a1', 'pro', 'active', now() + interval '30 days', now()),
  ('sub_cloud_team', 'cus_cloud_team', 'organization', '00000000-0000-0000-0000-0000000038f1', 'team', 'active', now() + interval '30 days', now());

select is(treq_internal.user_has_pro('00000000-0000-0000-0000-0000000038a2'), true,
  'a Team member still has Pro features');
select is(treq_internal.user_has_cloud('00000000-0000-0000-0000-0000000038a1'), true,
  'a personal Pro subscriber has cloud');
select is(treq_internal.user_has_cloud('00000000-0000-0000-0000-0000000038a2'), false,
  'a Team member without personal Pro has no cloud');
select is(treq_internal.user_can_byos('00000000-0000-0000-0000-0000000038a2'), false,
  'a Team member without personal Pro cannot bring their own sandbox');

insert into public.remote_instances (id, owner_user_id, provider_kind, region, size_preset, status)
values ('00000000-0000-0000-0000-0000000038b2', '00000000-0000-0000-0000-0000000038a2', 'local', 'us_east', 'small', 'ready'),
       ('00000000-0000-0000-0000-0000000038b1', '00000000-0000-0000-0000-0000000038a1', 'local', 'us_east', 'small', 'ready');

select ok(exists (select 1 from public.remote_lapse_candidates() where instance_id = '00000000-0000-0000-0000-0000000038b2'),
  'the lapse sweep finds a Team-only member''s workspace');
select ok(not exists (select 1 from public.remote_lapse_candidates() where instance_id = '00000000-0000-0000-0000-0000000038b1'),
  'the lapse sweep leaves a personal Pro workspace alone');
select isnt(public.remote_sync_instance_lapse('00000000-0000-0000-0000-0000000038b2'), null,
  'a Team-only member''s workspace is marked lapsed');

set local role service_role;
select set_config('request.jwt.claims', '{"role":"service_role"}', true);
select is(public.has_cloud('00000000-0000-0000-0000-0000000038a2'), false,
  'has_cloud answers the service role');
reset role;

select * from finish();
rollback;
