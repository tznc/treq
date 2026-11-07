-- Integration proxying through the Data API: the desktop app and agents in
-- cloud sandboxes call these RPCs with a Supabase JWT, and Postgres makes
-- the outbound call with the `http` extension. Nothing here needs an Edge
-- Function except signing the GitHub App JWT (RS256), which
-- github-app-jwt does and these functions cache in Vault.
--
-- Footprint: an outbound call holds a backend and a PostgREST connection
-- for its whole round trip. Every call here takes one of
-- INTEGRATION_SLOTS advisory locks first and fails fast with 429 when all
-- are taken, and curl is capped at 5s, so a burst of slow upstream calls
-- cannot take over the pool. pg_cron refreshes Linear tokens and the
-- GitHub App JWT ahead of expiry, so requests rarely make a second call,
-- but only while they are in use: a Linear token unused for a day or an
-- App JWT unused for 15 minutes is left to expire and refreshed inline on
-- the next call. `last_used_at` is written at most once a minute.
--
-- Vault secrets the operator creates (names below):
--   project_jwt_secret    the project's JWT secret, to sign sandbox JWTs
--   linear_client_id      Linear OAuth client, for token refresh
--   linear_client_secret
--   github_app_jwt_url    https://<ref>.supabase.co/functions/v1/github-app-jwt
--   github_app_jwt_key    shared with the function's GITHUB_APP_JWT_KEY

create extension if not exists http with schema extensions;
create extension if not exists pg_cron;

-- ── Helpers ─────────────────────────────────────────────────────────────

-- PostgREST turns SQLSTATE PGRST into a response with this status and body.
create function public.integration_raise(p_status int, p_code text, p_message text)
returns void
language plpgsql
set search_path = ''
as $$
begin
  raise sqlstate 'PGRST' using
    message = json_build_object('code', p_code, 'message', p_message)::text,
    detail = json_build_object('status', p_status, 'headers', json_build_object())::text;
end;
$$;

create function public.integration_vault(p_name text)
returns text
language sql
stable
security definer
set search_path = ''
as $$
  select decrypted_secret from vault.decrypted_secrets where name = p_name;
$$;

-- The calling user. A sandbox JWT (treq_placement_id claim) is accepted
-- only while its placement is live, still owned by `sub`, and the token was
-- issued for at most an hour.
create function public.integration_caller()
returns uuid
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  v_uid uuid := (select auth.uid());
  v_claims jsonb := (select auth.jwt());
  v_placement text := v_claims ->> 'treq_placement_id';
begin
  if v_uid is null then
    perform public.integration_raise(401, 'unauthorized', 'Sign in to use integrations');
  end if;
  if v_placement is not null and not (
    (v_claims ->> 'exp')::bigint - (v_claims ->> 'iat')::bigint <= 3600
    and exists (
      select 1 from public.remote_instance_placements p
      where p.id::text = v_placement and p.owner_user_id = v_uid and p.role <> 'dead'
    )
  ) then
    perform public.integration_raise(401, 'unauthorized', 'Sandbox token is no longer valid');
  end if;
  return v_uid;
end;
$$;

create function public.integration_curl_limits()
returns void
language plpgsql
set search_path = ''
as $$
begin
  perform extensions.http_set_curlopt('CURLOPT_TIMEOUT_MS', '5000');
  perform extensions.http_set_curlopt('CURLOPT_CONNECTTIMEOUT_MS', '2000');
end;
$$;

-- Takes one of 4 outbound slots for the rest of the transaction, or 429.
create function public.integration_take_slot()
returns void
language plpgsql
set search_path = ''
as $$
begin
  for slot in 0..3 loop
    if pg_try_advisory_xact_lock(hashtext('treq.integration_slot'), slot) then
      perform public.integration_curl_limits();
      return;
    end if;
  end loop;
  perform public.integration_raise(429, 'rate_limited', 'Integrations are busy, retry shortly');
end;
$$;

create function public.integration_b64url(p_bytes bytea)
returns text
language sql
immutable
set search_path = ''
as $$
  select rtrim(translate(replace(encode(p_bytes, 'base64'), E'\n', ''), '+/', '-_'), '=');
$$;

-- ── Sandbox JWT ─────────────────────────────────────────────────────────

-- A Supabase JWT for an agent in the caller's placement: HS256 with the
-- project's JWT secret, `sub` = owner, at most an hour. A sandbox JWT cannot
-- mint another.
create function public.integration_sandbox_token(p_placement_id uuid)
returns jsonb
language plpgsql
volatile
security definer
set search_path = ''
as $$
declare
  v_uid uuid := public.integration_caller();
  v_secret text := public.integration_vault('project_jwt_secret');
  v_now bigint := extract(epoch from now())::bigint;
  v_input text;
begin
  if (select auth.jwt()) ? 'treq_placement_id' then
    perform public.integration_raise(403, 'forbidden', 'Sandbox tokens cannot mint tokens');
  end if;
  if not exists (
    select 1 from public.remote_instance_placements
    where id = p_placement_id and owner_user_id = v_uid and role <> 'dead'
  ) then
    perform public.integration_raise(404, 'not_found', 'Placement not found');
  end if;
  if v_secret is null then
    perform public.integration_raise(500, 'not_configured', 'JWT secret not configured');
  end if;

  v_input := public.integration_b64url(convert_to('{"alg":"HS256","typ":"JWT"}', 'utf8'))
    || '.' || public.integration_b64url(convert_to(jsonb_build_object(
      'sub', v_uid, 'role', 'authenticated', 'aud', 'authenticated',
      'iat', v_now, 'exp', v_now + 3600, 'is_anonymous', false,
      'treq_placement_id', p_placement_id)::text, 'utf8'));
  return jsonb_build_object(
    'token', v_input || '.' || public.integration_b64url(
      extensions.hmac(convert_to(v_input, 'utf8'), convert_to(v_secret, 'utf8'), 'sha256')),
    'expires_at', to_timestamp(v_now + 3600));
end;
$$;

-- ── Linear ──────────────────────────────────────────────────────────────

alter table public.linear_oauth_tokens add column last_used_at timestamptz;

-- Refreshes a user's Linear token and saves it; the new access token, or
-- null when Linear refuses (the user must reconnect).
create function public.integration_linear_refresh(p_user_id uuid, p_refresh_token text)
returns text
language plpgsql
volatile
security definer
set search_path = ''
as $$
declare
  v_res extensions.http_response;
  v_grant jsonb;
begin
  v_res := extensions.http((
    'POST', 'https://api.linear.app/oauth/token', null,
    'application/x-www-form-urlencoded',
    'grant_type=refresh_token'
      || '&client_id=' || extensions.urlencode(coalesce(public.integration_vault('linear_client_id'), ''))
      || '&client_secret=' || extensions.urlencode(coalesce(public.integration_vault('linear_client_secret'), ''))
      || '&refresh_token=' || extensions.urlencode(p_refresh_token)
  )::extensions.http_request);
  if v_res.status <> 200 then
    raise warning 'Linear token refresh for % answered %', p_user_id, v_res.status;
    return null;
  end if;
  v_grant := v_res.content::jsonb;
  perform public.linear_tokens_save(
    p_user_id, v_grant ->> 'access_token',
    coalesce(v_grant ->> 'refresh_token', p_refresh_token),
    case when v_grant ? 'expires_in'
      then now() + make_interval(secs => (v_grant ->> 'expires_in')::int) end);
  return v_grant ->> 'access_token';
end;
$$;

-- A usable Linear access token. pg_cron refreshes tokens 15 minutes ahead
-- (integration_refresh_linear_tokens); this refreshes inline only if that
-- missed.
create function public.integration_linear_token(p_user_id uuid)
returns text
language plpgsql
volatile
security definer
set search_path = ''
as $$
declare
  v_token record;
  v_access text;
begin
  select * into v_token from public.linear_tokens_load(p_user_id) limit 1;
  if not found then
    perform public.integration_raise(403, 'not_linked', 'Linear account not linked');
  end if;
  update public.linear_oauth_tokens set last_used_at = now()
  where user_id = p_user_id
    and (last_used_at is null or last_used_at < now() - interval '1 minute');
  if v_token.expires_at is null or v_token.expires_at > now() + interval '1 minute' then
    return v_token.access_token;
  end if;
  if v_token.refresh_token is not null then
    v_access := public.integration_linear_refresh(p_user_id, v_token.refresh_token);
    if v_access is not null then return v_access; end if;
  end if;
  perform public.integration_raise(401, 'reconnect',
    'Linear authorization expired. Reconnect Linear in treq settings.');
  return null;
end;
$$;

-- pg_cron, every 5 minutes: refreshes up to 20 Linear tokens used in the
-- last day and expiring in the next 15 minutes, one at a time, each
-- committed on its own.
create function public.integration_refresh_linear_tokens()
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_user uuid;
  v_token record;
begin
  perform public.integration_curl_limits();
  for v_user in
    select user_id from public.linear_oauth_tokens
    where refresh_token_secret_id is not null
      and expires_at < now() + interval '15 minutes'
      and last_used_at > now() - interval '1 day'
    order by expires_at
    limit 20
  loop
    begin
      select * into v_token from public.linear_tokens_load(v_user) limit 1;
      perform public.integration_linear_refresh(v_user, v_token.refresh_token);
    exception when others then
      raise warning 'Linear token refresh for % failed: %', v_user, sqlerrm;
    end;
  end loop;
end;
$$;

-- POST /rest/v1/rpc/linear_graphql { p_query, p_variables }: Linear's JSON.
-- Pro only, as linear-proxy.
create function public.linear_graphql(p_query text, p_variables jsonb default null)
returns jsonb
language plpgsql
volatile
security definer
set search_path = ''
set statement_timeout = '7s'
as $$
declare
  v_uid uuid := public.integration_caller();
  v_res extensions.http_response;
begin
  if not public.has_pro(v_uid) then
    perform public.integration_raise(402, 'pro_required',
      'Linear OAuth needs Pro. Add a personal Linear API key in Settings instead.');
  end if;
  perform public.integration_take_slot();
  v_res := extensions.http((
    'POST', 'https://api.linear.app/graphql',
    array[extensions.http_header('Authorization', 'Bearer ' || public.integration_linear_token(v_uid))],
    'application/json',
    jsonb_build_object('query', p_query, 'variables', p_variables)::text
  )::extensions.http_request);

  if v_res.status = 401 then
    perform public.integration_raise(401, 'reconnect',
      'Linear authorization expired. Reconnect Linear in treq settings.');
  elsif v_res.status >= 500 then
    raise warning 'Linear answered %', v_res.status;
    perform public.integration_raise(502, 'linear_error', 'Linear is unavailable, retry shortly');
  end if;
  -- A 4xx is returned as Linear's body (GraphQL `errors`), not raised: a
  -- raise rolls back the transaction, which would lose a refreshed token
  -- after its refresh token was spent.
  return v_res.content::jsonb;
end;
$$;

-- ── GitHub ──────────────────────────────────────────────────────────────

-- App JWTs from github-app-jwt, cached until a minute before expiry.
create table public.github_app_jwt_cache (
  id boolean primary key default true check (id),
  secret_id uuid not null,
  expires_at timestamptz not null,
  last_used_at timestamptz not null default now()
);
alter table public.github_app_jwt_cache enable row level security;
revoke all on table public.github_app_jwt_cache from public, anon, authenticated;

-- Fetches a fresh App JWT from github-app-jwt into the cache.
create function public.integration_github_app_jwt_refresh()
returns text
language plpgsql
volatile
security definer
set search_path = ''
as $$
declare
  v_secret_id uuid;
  v_res extensions.http_response;
  v_body jsonb;
  v_url text := public.integration_vault('github_app_jwt_url');
begin
  if v_url is null then return null; end if;
  v_res := extensions.http((
    'POST', v_url,
    array[extensions.http_header('x-github-app-jwt-key', coalesce(public.integration_vault('github_app_jwt_key'), ''))],
    'application/json', '{}'
  )::extensions.http_request);
  if v_res.status <> 200 then
    raise warning 'github-app-jwt answered %', v_res.status;
    return null;
  end if;
  v_body := v_res.content::jsonb;

  select secret_id into v_secret_id from public.github_app_jwt_cache for update;
  if found then
    perform vault.update_secret(v_secret_id, v_body ->> 'jwt');
    update public.github_app_jwt_cache set expires_at = (v_body ->> 'expires_at')::timestamptz;
  else
    insert into public.github_app_jwt_cache (secret_id, expires_at) values (
      vault.create_secret(v_body ->> 'jwt', 'github_app_jwt', 'Cached GitHub App JWT'),
      (v_body ->> 'expires_at')::timestamptz)
    on conflict (id) do nothing;
  end if;
  return v_body ->> 'jwt';
end;
$$;

-- The cached App JWT. pg_cron refreshes it every 4 minutes while it was
-- used in the last 15; this refreshes inline otherwise.
create function public.integration_github_app_jwt()
returns text
language plpgsql
volatile
security definer
set search_path = ''
as $$
declare
  v_cache public.github_app_jwt_cache;
  v_jwt text;
begin
  update public.github_app_jwt_cache set last_used_at = now()
  where last_used_at < now() - interval '1 minute';
  select * into v_cache from public.github_app_jwt_cache;
  if found and v_cache.expires_at > now() + interval '1 minute' then
    return (select decrypted_secret from vault.decrypted_secrets where id = v_cache.secret_id);
  end if;
  v_jwt := public.integration_github_app_jwt_refresh();
  if v_jwt is null then
    perform public.integration_raise(503, 'github_unavailable', 'GitHub App is not configured');
  end if;
  return v_jwt;
end;
$$;

-- POST /rest/v1/rpc/github_repo_token { p_repository: "owner/name" }:
-- { token, expires_at }, an installation token for that one repository.
create function public.github_repo_token(p_repository text)
returns jsonb
language plpgsql
volatile
security definer
set search_path = ''
set statement_timeout = '7s'
as $$
declare
  v_uid uuid := public.integration_caller();
  v_installation bigint;
  v_res extensions.http_response;
  v_grant jsonb;
begin
  if p_repository !~ '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$' then
    perform public.integration_raise(400, 'bad_request', 'repository must be owner/name');
  end if;
  select r.installation_id into v_installation
  from public.github_repositories r
  join public.github_app_installations i on i.id = r.installation_id
  where lower(r.full_name) = lower(p_repository)
    and i.linked_user_id = v_uid and i.suspended_at is null
  limit 1;
  if v_installation is null then
    perform public.integration_raise(403, 'not_installed',
      'The treq GitHub App is not installed for this repository');
  end if;

  perform public.integration_take_slot();
  v_res := extensions.http((
    'POST', 'https://api.github.com/app/installations/' || v_installation || '/access_tokens',
    array[
      extensions.http_header('Authorization', 'Bearer ' || public.integration_github_app_jwt()),
      extensions.http_header('Accept', 'application/vnd.github+json'),
      extensions.http_header('X-GitHub-Api-Version', '2022-11-28'),
      extensions.http_header('User-Agent', 'treq-github-app/1.0')
    ],
    'application/json',
    jsonb_build_object('repositories', jsonb_build_array(split_part(p_repository, '/', 2)))::text
  )::extensions.http_request);
  if v_res.status <> 201 then
    raise warning 'GitHub token mint answered %', v_res.status;
    perform public.integration_raise(502, 'github_error', 'Failed to mint GitHub token');
  end if;
  v_grant := v_res.content::jsonb;
  return jsonb_build_object('token', v_grant ->> 'token', 'expires_at', v_grant ->> 'expires_at');
end;
$$;

-- ── Grants ──────────────────────────────────────────────────────────────

revoke all on function public.integration_raise(int, text, text) from public, anon, authenticated;
revoke all on function public.integration_vault(text) from public, anon, authenticated;
revoke all on function public.integration_caller() from public, anon, authenticated;
revoke all on function public.integration_take_slot() from public, anon, authenticated;
revoke all on function public.integration_curl_limits() from public, anon, authenticated;
revoke all on function public.integration_linear_refresh(uuid, text) from public, anon, authenticated;
revoke all on function public.integration_refresh_linear_tokens() from public, anon, authenticated;
revoke all on function public.integration_github_app_jwt_refresh() from public, anon, authenticated;
revoke all on function public.integration_b64url(bytea) from public, anon, authenticated;
revoke all on function public.integration_linear_token(uuid) from public, anon, authenticated;
revoke all on function public.integration_github_app_jwt() from public, anon, authenticated;

revoke all on function public.integration_sandbox_token(uuid) from public, anon;
revoke all on function public.linear_graphql(text, jsonb) from public, anon;
revoke all on function public.github_repo_token(text) from public, anon;
grant execute on function public.integration_sandbox_token(uuid) to authenticated;
grant execute on function public.linear_graphql(text, jsonb) to authenticated;
grant execute on function public.github_repo_token(text) to authenticated;

-- ── Schedules ───────────────────────────────────────────────────────────

select cron.schedule('integration-refresh-linear-tokens', '*/5 * * * *',
  'select public.integration_refresh_linear_tokens()');
select cron.schedule('integration-refresh-github-app-jwt', '*/4 * * * *',
  $cmd$select public.integration_curl_limits(), public.integration_github_app_jwt_refresh()
    where exists (select 1 from public.github_app_jwt_cache
                  where last_used_at > now() - interval '15 minutes')$cmd$);
