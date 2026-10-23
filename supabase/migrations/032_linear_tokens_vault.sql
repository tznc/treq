-- Linear OAuth tokens move from plaintext columns into Supabase Vault.
--
-- linear_oauth_tokens keeps only the Vault secret ids. Edge Functions read
-- and write the tokens through two service-role-only functions that run as
-- their owner (postgres, which has Vault access), the same way
-- 013_subscriptions_security_definer.sql reaches the Stripe key.

alter table public.linear_oauth_tokens
  add column access_token_secret_id uuid,
  add column refresh_token_secret_id uuid;

-- Existing connections keep working: their tokens move into Vault.
update public.linear_oauth_tokens
set
  access_token_secret_id = vault.create_secret(access_token, null, 'Linear access token for ' || user_id),
  refresh_token_secret_id = case
    when refresh_token is null then null
    else vault.create_secret(refresh_token, null, 'Linear refresh token for ' || user_id)
  end;

alter table public.linear_oauth_tokens
  drop column access_token,
  drop column refresh_token,
  alter column access_token_secret_id set not null;

-- The signed-in user's tokens, decrypted.
create or replace function public.linear_tokens_load(p_user_id uuid)
returns table (
  access_token text,
  refresh_token text,
  expires_at timestamptz,
  updated_at timestamptz
)
language sql
security definer
set search_path = ''
as $$
  select a.decrypted_secret, r.decrypted_secret, t.expires_at, t.updated_at
  from public.linear_oauth_tokens t
  join vault.decrypted_secrets a on a.id = t.access_token_secret_id
  left join vault.decrypted_secrets r on r.id = t.refresh_token_secret_id
  where t.user_id = p_user_id;
$$;

-- Saves a user's tokens. A new connection needs the workspace; a refresh
-- leaves it as it is (pass null).
create or replace function public.linear_tokens_save(
  p_user_id uuid,
  p_access_token text,
  p_refresh_token text,
  p_expires_at timestamptz,
  p_workspace_id text default null,
  p_workspace_name text default null
)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_row public.linear_oauth_tokens%rowtype;
  v_refresh uuid;
begin
  select * into v_row from public.linear_oauth_tokens where user_id = p_user_id for update;

  if not found then
    if p_workspace_id is null or p_workspace_name is null then
      raise exception 'linear_tokens_save: no Linear connection for %', p_user_id;
    end if;
    insert into public.linear_oauth_tokens
      (user_id, access_token_secret_id, refresh_token_secret_id, expires_at,
       linear_workspace_id, linear_workspace_name)
    values (
      p_user_id,
      vault.create_secret(p_access_token, null, 'Linear access token for ' || p_user_id),
      case
        when p_refresh_token is null then null
        else vault.create_secret(p_refresh_token, null, 'Linear refresh token for ' || p_user_id)
      end,
      p_expires_at,
      p_workspace_id,
      p_workspace_name
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
    v_refresh := vault.create_secret(p_refresh_token, null, 'Linear refresh token for ' || p_user_id);
  else
    perform vault.update_secret(v_refresh, p_refresh_token);
  end if;

  update public.linear_oauth_tokens
  set
    refresh_token_secret_id = v_refresh,
    expires_at = p_expires_at,
    linear_workspace_id = coalesce(p_workspace_id, linear_workspace_id),
    linear_workspace_name = coalesce(p_workspace_name, linear_workspace_name),
    updated_at = now()
  where user_id = p_user_id;
end;
$$;

-- A deleted connection (or user, through the cascade) takes its secrets along.
create or replace function public.linear_tokens_delete_secrets()
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

create trigger linear_oauth_tokens_delete_secrets
after delete on public.linear_oauth_tokens
for each row execute function public.linear_tokens_delete_secrets();

revoke all on function public.linear_tokens_load(uuid) from public, anon, authenticated;
grant execute on function public.linear_tokens_load(uuid) to service_role;
revoke all on function public.linear_tokens_save(uuid, text, text, timestamptz, text, text)
  from public, anon, authenticated;
grant execute on function public.linear_tokens_save(uuid, text, text, timestamptz, text, text)
  to service_role;
revoke all on function public.linear_tokens_delete_secrets() from public, anon, authenticated;
