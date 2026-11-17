-- Regression test for 022_remote_instances_recreate_after_delete.sql: a
-- user can create a new managed instance after deleting the old one, but
-- still holds at most one instance that is not deleted.
begin;
select plan(3);

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000c1', 'recreate@example.com');

insert into public.remote_instances (owner_user_id, provider_kind, region, size_preset, status) values
  ('00000000-0000-0000-0000-0000000000c1', 'boat', 'us_east', 'small', 'deleted');

select lives_ok(
  $$insert into public.remote_instances (owner_user_id, provider_kind, region, size_preset, status)
    values ('00000000-0000-0000-0000-0000000000c1', 'boat', 'us_east', 'small', 'provisioning')$$,
  'a new instance can be created next to a deleted one'
);

select throws_ok(
  $$insert into public.remote_instances (owner_user_id, provider_kind, region, size_preset, status)
    values ('00000000-0000-0000-0000-0000000000c1', 'boat', 'us_east', 'small', 'provisioning')$$,
  '23505',
  null,
  'a second live instance for the same owner is rejected'
);

select lives_ok(
  $$insert into public.remote_instances (owner_user_id, provider_kind, region, size_preset, status)
    values ('00000000-0000-0000-0000-0000000000c1', 'boat', 'us_east', 'small', 'deleted')$$,
  'deleted instances do not count toward the limit'
);

select * from finish();
rollback;
