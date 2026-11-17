-- Test account coupon (076): emails and coupon id live in
-- treq_internal.admin_config under 'test_coupon', checked by its schema.
begin;
select plan(5);

select is((select value from treq_internal.admin_config where key = 'test_coupon'),
  '{"emails": [], "coupon_id": ""}'::jsonb,
  'seeded empty, so the coupon is off');
select lives_ok(
  $$ update treq_internal.admin_config
        set value = '{"emails": ["dev@example.com", "qa@example.com"], "coupon_id": "coupon_free"}'
      where key = 'test_coupon' $$,
  'a list of emails is accepted');
select throws_ok(
  $$ update treq_internal.admin_config set value = '{"emails": "dev@example.com", "coupon_id": "c"}' where key = 'test_coupon' $$,
  'P0001', 'admin_config: test_coupon does not match its schema',
  'a single email string is rejected');
select throws_ok(
  $$ update treq_internal.admin_config set value = '{"emails": [1], "coupon_id": "c"}' where key = 'test_coupon' $$,
  'P0001', 'admin_config: test_coupon does not match its schema',
  'a non-string email is rejected');
select throws_ok(
  $$ update treq_internal.admin_config set value = '{"emails": []}' where key = 'test_coupon' $$,
  'P0001', 'admin_config: test_coupon does not match its schema',
  'a missing coupon_id is rejected');

select * from finish();
rollback;
