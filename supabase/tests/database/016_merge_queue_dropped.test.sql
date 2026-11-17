-- 035_drop_merge_queue.sql removes the merge queue: its tables, RPCs, PGMQ
-- queues and the Pro trigger are gone, and the GitHub App tables it shared
-- with installation linking stay.
begin;
select plan(13);

select hasnt_table('public', 'merge_queue_configs', 'merge_queue_configs is dropped');
select hasnt_table('public', 'merge_queues', 'merge_queues is dropped');
select hasnt_table('public', 'merge_queue_entries', 'merge_queue_entries is dropped');
select hasnt_table('public', 'ci_runs', 'ci_runs is dropped');
select hasnt_table('public', 'ci_run_entries', 'ci_run_entries is dropped');
select hasnt_table('public', 'merge_queue_command_executions', 'merge_queue_command_executions is dropped');
select hasnt_table('public', 'merge_queue_execution_leases', 'merge_queue_execution_leases is dropped');

select is(
  (select count(*)::int from pg_proc p join pg_namespace n on n.oid = p.pronamespace
    where n.nspname in ('public', 'treq_internal')
      and (p.proname like '%merge_queue%' or p.proname like '%queue_status%')),
  0,
  'no merge queue function is left');
select is(
  (select count(*)::int from pgmq.list_queues() where queue_name like 'merge_queue%'),
  0,
  'the merge queue PGMQ queues are dropped');
select hasnt_view('public', 'merge_queue_stuck_ci_runs', 'merge_queue_stuck_ci_runs is dropped');

select has_table('public', 'github_app_installations', 'github_app_installations stays');
select has_table('public', 'github_repositories', 'github_repositories stays');
select has_function('public', 'installation_has_pro', array['bigint'], 'installation_has_pro stays');

select * from finish();
rollback;
