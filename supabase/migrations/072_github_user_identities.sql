-- The GitHub account behind each treq user who linked a GitHub App
-- installation, recorded by complete-github-installation from the user's
-- OAuth token (the token itself is never stored). integration-proxy checks
-- this account's permission on a repository before minting a token for it,
-- because an installation token acts as the App, not the user.

create table public.github_user_identities (
  user_id uuid primary key references auth.users (id) on delete cascade,
  github_user_id bigint not null,
  github_login text not null,
  updated_at timestamptz not null default now()
);

alter table public.github_user_identities enable row level security;
revoke all on table public.github_user_identities from public, anon, authenticated;
