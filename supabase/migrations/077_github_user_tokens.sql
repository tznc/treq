-- GitHub App user access tokens, kept in Vault like 032's Linear tokens.
-- complete-github-installation saves the grant when a user links an
-- installation; integration-proxy refreshes it and hands the desktop and
-- mobile apps a short-lived token that acts as the user. Only service_role
-- can read or write them, and a deleted row takes its secrets along.

create table public.github_user_tokens (
  user_id uuid primary key references auth.users (id) on delete cascade,
  access_token_secret_id uuid not null,
  refresh_token_secret_id uuid,
  expires_at timestamptz,
  refresh_expires_at timestamptz,
  updated_at timestamptz not null default now()
);

alter table public.github_user_tokens enable row level security;
revoke all on table public.github_user_tokens from public, anon, authenticated;

create function public.github_user_tokens_load(p_user_id uuid)
returns table (
  access_token text,
  refresh_token text,
  expires_at timestamptz,
  refresh_expires_at timestamptz,
  updated_at timestamptz
)
language sql
security definer
set search_path = ''
as $$
  select a.decrypted_secret, r.decrypted_secret, t.expires_at, t.refresh_expires_at, t.updated_at
  from public.github_user_tokens t
  join vault.decrypted_secrets a on a.id = t.access_token_secret_id
  left join vault.decrypted_secrets r on r.id = t.refresh_token_secret_id
  where t.user_id = p_user_id;
$$;

-- Saves or replaces the user's grant. GitHub rotates the refresh token on
-- every refresh, so both tokens are always replaced together.
create function public.github_user_tokens_save(
  p_user_id uuid,
  p_access_token text,
  p_refresh_token text,
  p_expires_at timestamptz,
  p_refresh_expires_at timestamptz
)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_row public.github_user_tokens%rowtype;
  v_refresh uuid;
begin
  select * into v_row from public.github_user_tokens where user_id = p_user_id for update;

  if not found then
    insert into public.github_user_tokens
      (user_id, access_token_secret_id, refresh_token_secret_id, expires_at, refresh_expires_at)
    values (
      p_user_id,
      vault.create_secret(p_access_token, null, 'GitHub user token for ' || p_user_id),
      case when p_refresh_token is null then null
        else vault.create_secret(p_refresh_token, null, 'GitHub refresh token for ' || p_user_id) end,
      p_expires_at,
      p_refresh_expires_at
    );
    return;
  end if;

  perform vault.update_secret(v_row.access_token_secret_id, p_access_token);
  v_refresh := v_row.refresh_token_secret_id;
  if p_refresh_token is null then
    if v_refresh is not null then
      delete from vault.secrets where id = v_refresh;
      v_refresh := null;
    end if;
  elsif v_refresh is null then
    v_refresh := vault.create_secret(p_refresh_token, null, 'GitHub refresh token for ' || p_user_id);
  else
    perform vault.update_secret(v_refresh, p_refresh_token);
  end if;

  update public.github_user_tokens
  set refresh_token_secret_id = v_refresh,
      expires_at = p_expires_at,
      refresh_expires_at = p_refresh_expires_at,
      updated_at = now()
  where user_id = p_user_id;
end;
$$;

create function public.github_user_tokens_delete_secrets()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  delete from vault.secrets
  where id in (old.access_token_secret_id, old.refresh_token_secret_id);
  return old;
end;
$$;

create trigger github_user_tokens_delete_secrets
after delete on public.github_user_tokens
for each row execute function public.github_user_tokens_delete_secrets();

revoke all on function public.github_user_tokens_load(uuid) from public, anon, authenticated;
revoke all on function public.github_user_tokens_save(uuid, text, text, timestamptz, timestamptz)
  from public, anon, authenticated;
revoke all on function public.github_user_tokens_delete_secrets() from public, anon, authenticated;
grant execute on function public.github_user_tokens_load(uuid) to service_role;
grant execute on function public.github_user_tokens_save(uuid, text, text, timestamptz, timestamptz)
  to service_role;
