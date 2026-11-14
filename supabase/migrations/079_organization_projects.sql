-- Team projects: a project owned by a Team organization is shared with its
-- members. Personal projects stay local to the app.

-- Repos are shared across machines, so they are remote references
-- (`owner/name` or a clone URL), never local paths. Each machine maps them
-- to its own clone.
create function public.is_remote_repo_list(p_repos jsonb)
returns boolean
language sql
immutable
set search_path = ''
as $$
  select jsonb_typeof(p_repos) = 'array'
     and not exists (
       select 1 from jsonb_array_elements(p_repos) e
       where jsonb_typeof(e) <> 'string'
          or not (e #>> '{}') ~ '^([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+|https://\S+|ssh://\S+|[A-Za-z0-9_.-]+@[A-Za-z0-9.-]+:\S+)$'
     );
$$;

create table public.organization_projects (
  id uuid primary key default gen_random_uuid(),
  org_id uuid not null references public.organizations (id) on delete cascade,
  name text not null check (char_length(name) between 1 and 100 and name = btrim(name)),
  repos jsonb not null default '[]' check (public.is_remote_repo_list(repos)),
  prompt text not null default '',
  context_paths jsonb not null default '[]' check (jsonb_typeof(context_paths) = 'array'),
  created_by uuid references auth.users (id) on delete set null default auth.uid(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create index idx_organization_projects_org on public.organization_projects (org_id);

alter table public.organization_projects enable row level security;
revoke all on public.organization_projects from anon;
grant select, insert, update, delete on public.organization_projects to authenticated;

create policy organization_projects_member_all on public.organization_projects
  for all to authenticated
  using (public.is_organization_member(org_id))
  with check (public.is_organization_member(org_id));

-- Upsert by id, as the caller: RLS decides which organizations it may write.
create function public.put_organization_project(
  p_id uuid, p_org_id uuid, p_name text, p_repos jsonb, p_prompt text, p_context_paths jsonb
)
returns void
language sql
security invoker
set search_path = ''
as $$
  insert into public.organization_projects (id, org_id, name, repos, prompt, context_paths)
  values (p_id, p_org_id, p_name, p_repos, p_prompt, p_context_paths)
  on conflict (id) do update
    set org_id = excluded.org_id, name = excluded.name, repos = excluded.repos,
        prompt = excluded.prompt, context_paths = excluded.context_paths, updated_at = now();
$$;

revoke all on function public.put_organization_project(uuid, uuid, text, jsonb, text, jsonb) from public, anon;
grant execute on function public.put_organization_project(uuid, uuid, text, jsonb, text, jsonb) to authenticated;
