-- Remove the merge queue feature and its data.
--
-- Drops every object 003, 004, 005, 007, 009, 010, 027, 028 and 033 created
-- for the queue: the Pro trigger on merge_queue_configs, the queue RPCs and
-- PGMQ wrappers, the observability views, the queue and CI tables, their
-- enums, and the two PGMQ queues. No pg_cron job was ever scheduled for the
-- worker or reconciler (they ran from an external cron), so there is none to
-- unschedule.
--
-- Kept: github_app_installations, github_repositories, github_webhook_receipts,
-- link_github_installation, has_pro and installation_has_pro (GitHub App
-- linking and billing still use them), and the pgmq extension itself.

-- ── Trigger (027) ────────────────────────────────────────────────────────────
drop trigger if exists merge_queue_configs_require_pro on public.merge_queue_configs;
drop function if exists treq_internal.merge_queue_configs_require_pro();

-- ── RPCs ─────────────────────────────────────────────────────────────────────
drop function if exists public.get_repo_queue_status(bigint);
drop function if exists public.get_workspace_queue_status(text, text);
drop function if exists public.get_repo_branch_queue_statuses(text);
drop function if exists public.get_repo_branch_queue_statuses(text, int);
drop function if exists public.get_merge_queue_enabled(text);
drop function if exists public.set_merge_queue_enabled(text, boolean);
drop function if exists public.get_or_create_merge_queue(bigint, text);
drop function if exists public.enqueue_merge_queue_entry(uuid, int, text, text, text, text);
drop function if exists public.reserve_next_merge_queue_lane(uuid, uuid, uuid, int);
drop function if exists public.acquire_merge_queue_lease(uuid, uuid, int);
drop function if exists public.renew_merge_queue_lease(uuid, uuid, int);
drop function if exists public.release_merge_queue_lease(uuid, uuid);
drop function if exists public.merge_queue_send(text, jsonb, int);
drop function if exists public.merge_queue_read(text, int, int);
drop function if exists public.merge_queue_archive(text, bigint);
drop function if exists public.merge_queue_delete(text, bigint);
drop function if exists public.merge_queue_set_vt(text, bigint, int);
drop function if exists public.merge_queue_metrics();

-- ── Views (005) ──────────────────────────────────────────────────────────────
drop view if exists public.merge_queue_stuck_ci_runs;
drop view if exists public.merge_queue_command_failures;

-- ── Tables, children first ───────────────────────────────────────────────────
drop table if exists public.ci_run_entries;
drop table if exists public.ci_runs;
drop table if exists public.merge_queue_command_executions;
drop table if exists public.merge_queue_execution_leases;
drop table if exists public.merge_queue_entries;
drop table if exists public.merge_queues;
drop table if exists public.merge_queue_configs;

drop type if exists public.ci_run_status;
drop type if exists public.merge_queue_entry_status;

-- ── PGMQ queues (005) ────────────────────────────────────────────────────────
do $$
begin
  if exists (select 1 from pgmq.list_queues() where queue_name = 'merge_queue_commands') then
    perform pgmq.drop_queue('merge_queue_commands');
  end if;
  if exists (select 1 from pgmq.list_queues() where queue_name = 'merge_queue_dead_letters') then
    perform pgmq.drop_queue('merge_queue_dead_letters');
  end if;
end $$;
