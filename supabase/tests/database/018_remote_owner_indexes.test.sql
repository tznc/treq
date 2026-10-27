-- Regression test for 037_remote_owner_indexes.sql. Every remote_* table is
-- read through an `owner_user_id = auth.uid()` policy, so each needs an index
-- leading with owner_user_id or a signed-in read scans every user's rows.
begin;
select plan(1);

select is_empty(
  $$
    select c.relname
    from pg_class c
    join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
    join pg_attribute a on a.attrelid = c.oid and a.attname = 'owner_user_id' and not a.attisdropped
    where c.relkind = 'r' and c.relname like 'remote\_%'
      and not exists (select 1 from pg_index i where i.indrelid = c.oid and i.indkey[0] = a.attnum)
  $$,
  'every remote_* table has an index leading with owner_user_id'
);

select * from finish();
rollback;
