-- Merged entries are never purged, so the queue tab downloaded every merged
-- PR the repository ever had on each 30 s poll. p_history_limit keeps every
-- queued entry and only the newest N merged ones (highest positions first).
-- Left out (null) it returns what it always did, so older clients keep working.
drop function if exists public.get_repo_branch_queue_statuses(text);

create function public.get_repo_branch_queue_statuses(
  p_repo_full_name text,
  p_history_limit int default null
)
returns table (
  branch_name text,
  pr_number int,
  pr_title text,
  status text,
  "position" int,
  target_branch text
)
language sql
security definer
set search_path = ''
as $$
  select s.branch_name, s.pr_number, s.pr_title, s.status, s.position, s.target_branch
  from (
    select
      e.branch_name,
      e.pr_number,
      e.pr_title,
      e.status::text as status,
      e.position,
      q.target_branch,
      row_number() over (
        partition by (e.status::text = 'merged')
        order by e.position desc, e.id
      ) as rank
    from public.merge_queue_entries e
    join public.merge_queues q
      on q.id = e.queue_id
    join public.github_repositories r
      on r.id = q.repo_id
    join public.github_app_installations i
      on i.id = r.installation_id
    where r.full_name = p_repo_full_name
      and e.status::text not in ('failed', 'dequeued')
      and e.branch_name is not null
      and i.linked_user_id = (select auth.uid())
  ) s
  where p_history_limit is null
    or s.status <> 'merged'
    or s.rank <= greatest(p_history_limit, 0);
$$;

revoke all on function public.get_repo_branch_queue_statuses(text, int) from public, anon;
grant execute on function public.get_repo_branch_queue_statuses(text, int) to authenticated, service_role;
