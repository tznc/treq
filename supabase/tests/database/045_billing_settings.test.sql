-- Billing rates come from treq_admin.admin_config (073; before it,
-- treq_internal.billing_settings, 062), not from custom GUCs, which any
-- database session can SET for itself.
begin;
select plan(7);

set local treq.byos_markup_bps = '0';
set local treq.spend_cap_max_usd = '1000000000';
select is(treq_internal.byos_markup_bps(), 500, 'a session GUC cannot change the BYOS rate');
select is(treq_internal.billed_nanousd(1000, 2000, 'byos'), 50::numeric, 'BYOS still bills 5% of cost');
select is(treq_internal.spend_cap_max_usd(), 1000::numeric, 'a session GUC cannot raise the spend cap ceiling');

update treq_admin.admin_config set value = '700' where key = 'byos_markup_bps';
update treq_admin.admin_config set value = '2000' where key = 'spend_cap_max_usd';
select is(treq_internal.byos_markup_bps(), 700, 'operators change the BYOS rate in admin_config');
select is(treq_internal.spend_cap_max_usd(), 2000::numeric, 'operators change the cap ceiling in admin_config');
select throws_ok($$ update treq_admin.admin_config set value = '20000' where key = 'byos_markup_bps' $$,
  'P0001', 'byos_markup_bps must be a number between 0 and 10000', 'an out-of-range BYOS rate is rejected');
select is(to_regclass('treq_internal.billing_settings'), null, 'billing_settings is gone; admin_config replaces it');

select * from finish();
rollback;
