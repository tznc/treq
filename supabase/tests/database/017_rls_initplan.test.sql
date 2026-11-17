-- Regression test for 036_rls_initplan.sql. A policy that calls auth.uid()
-- directly runs it once per row; wrapped as (select auth.uid()) Postgres
-- runs it once per query (Supabase advisor lint auth_rls_initplan). Postgres
-- prints the wrapped form as "( SELECT auth.uid() AS uid)", so any
-- auth.uid() left after removing those is a bare call.
begin;
select plan(1);

select is_empty(
  $$
    select tablename || ': ' || policyname
    from pg_policies
    where schemaname = 'public'
      and regexp_replace(coalesce(qual, '') || ' ' || coalesce(with_check, ''),
                         '\(\s*select auth\.uid\(\)( as uid)?\s*\)', '', 'gi') ~ 'auth\.uid\(\)'
  $$,
  'no public policy calls auth.uid() once per row'
);

select * from finish();
rollback;
