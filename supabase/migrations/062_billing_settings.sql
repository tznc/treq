-- The BYOS rate and the spend cap ceiling were read from custom GUCs
-- (treq.byos_markup_bps, treq.spend_cap_max_usd). Any database session can
-- SET a custom GUC for itself, so a rate that reads one is only as safe as
-- every path that prices usage. They now live in one service-only row.

create table treq_internal.billing_settings (
  id boolean primary key default true check (id),
  byos_markup_bps int not null default 500 check (byos_markup_bps between 0 and 10000),
  spend_cap_max_usd numeric not null default 1000 check (spend_cap_max_usd >= 5)
);
insert into treq_internal.billing_settings default values;
revoke all on treq_internal.billing_settings from public, anon, authenticated;

create or replace function treq_internal.byos_markup_bps()
returns int
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce((select s.byos_markup_bps from treq_internal.billing_settings s), 500);
$$;

create or replace function treq_internal.spend_cap_max_usd()
returns numeric
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce((select s.spend_cap_max_usd from treq_internal.billing_settings s), 1000);
$$;

revoke all on function treq_internal.byos_markup_bps() from public, anon, authenticated;
revoke all on function treq_internal.spend_cap_max_usd() from public, anon, authenticated;
