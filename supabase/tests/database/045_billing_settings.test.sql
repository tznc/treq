-- Billing rates come from treq_internal.billing_settings (062), not from
-- custom GUCs, which any database session can SET for itself.
begin;
select plan(5);

set local treq.byos_markup_bps = '0';
set local treq.spend_cap_max_usd = '1000000000';
select is(treq_internal.byos_markup_bps(), 500, 'a session GUC cannot change the BYOS rate');
select is(treq_internal.billed_nanousd(1000, 2000, 'byos'), 50::numeric, 'BYOS still bills 5% of cost');
select is(treq_internal.spend_cap_max_usd(), 1000::numeric, 'a session GUC cannot raise the spend cap ceiling');

update treq_internal.billing_settings set byos_markup_bps = 700, spend_cap_max_usd = 2000;
select is(treq_internal.byos_markup_bps(), 700, 'operators change the BYOS rate in billing_settings');
select is(treq_internal.spend_cap_max_usd(), 2000::numeric, 'operators change the cap ceiling in billing_settings');

select * from finish();
rollback;
