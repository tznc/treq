-- Bring your own sandbox (BYOS): a user's own provider API key, stored in
-- Vault, and the one pricing rule for managed and BYOS usage. Placements
-- reference these credentials (038).

-- The single plan gate for BYOS; change it only here.
create function treq_internal.user_can_byos(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select treq_internal.user_has_pro(p_user_id);
$$;

create function treq_internal.byos_markup_bps()
returns int
language sql
stable
set search_path = ''
as $$
  select coalesce(nullif(current_setting('treq.byos_markup_bps', true), '')::int, 500);
$$;

-- What treq bills for a provider cost: cost x (1 + markup) on a managed
-- machine, cost x byos_markup_bps on the user's own account.
create function treq_internal.billed_nanousd(p_cost numeric, p_markup_bps int, p_billing_mode text)
returns numeric
language sql
stable
set search_path = ''
as $$
  select case when p_billing_mode = 'byos'
              then p_cost * treq_internal.byos_markup_bps() / 10000.0
              else p_cost * (10000 + p_markup_bps) / 10000.0 end;
$$;

create function public.can_byos(p_user_id uuid)
returns boolean
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return treq_internal.user_can_byos(p_user_id);
end;
$$;

create table public.remote_provider_credentials (
  id uuid primary key default gen_random_uuid(),
  owner_user_id uuid not null references auth.users (id) on delete cascade,
  provider_kind text not null references public.sandbox_providers (kind),
  vault_secret_id uuid not null,
  label text check (label is null or char_length(label) <= 80),
  validated_at timestamptz,
  invalid_at timestamptz,
  created_at timestamptz not null default now(),
  unique (owner_user_id, provider_kind)
);

alter table public.remote_provider_credentials enable row level security;
revoke all on public.remote_provider_credentials from public, anon, authenticated;
grant select (id, owner_user_id, provider_kind, label, validated_at, invalid_at, created_at)
  on public.remote_provider_credentials to authenticated;
create policy remote_provider_credentials_owner_read on public.remote_provider_credentials
  for select to authenticated using (owner_user_id = (select auth.uid()));

create function treq_internal.provider_credential_delete_vault()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  delete from vault.secrets where id = old.vault_secret_id;
  return old;
end;
$$;

create trigger remote_provider_credentials_delete_vault
after delete on public.remote_provider_credentials
for each row execute function treq_internal.provider_credential_delete_vault();

-- Stores a key the caller has already validated against the provider.
create function public.provider_credential_save(
  p_user_id uuid, p_kind text, p_key text, p_label text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_row public.remote_provider_credentials;
begin
  perform treq_internal.require_service_role();
  if not treq_internal.user_can_byos(p_user_id) then
    raise exception 'byos_not_allowed';
  end if;
  if p_kind is null or p_kind = 'local'
     or not exists (select 1 from public.sandbox_providers where kind = p_kind) then
    raise exception 'byos_kind_unsupported';
  end if;
  if p_key is null or length(p_key) = 0 then
    raise exception 'byos_key_required';
  end if;

  select * into v_row from public.remote_provider_credentials
   where owner_user_id = p_user_id and provider_kind = p_kind
   for update;
  if found then
    perform vault.update_secret(v_row.vault_secret_id, p_key);
    update public.remote_provider_credentials
       set label = p_label, validated_at = now(), invalid_at = null
     where id = v_row.id
    returning * into v_row;
  else
    insert into public.remote_provider_credentials
      (owner_user_id, provider_kind, vault_secret_id, label, validated_at)
    values (p_user_id, p_kind,
            vault.create_secret(p_key, 'treq-byos:' || p_user_id || ':' || p_kind,
                                'BYOS provider key'),
            p_label, now())
    returning * into v_row;
  end if;
  return jsonb_build_object('id', v_row.id, 'provider_kind', v_row.provider_kind, 'label', v_row.label,
                            'validated_at', v_row.validated_at, 'invalid_at', v_row.invalid_at,
                            'created_at', v_row.created_at);
end;
$$;

-- {credential_source, credential_id, key} for a placement. key is null for
-- managed placements and for a BYOS placement whose credential is gone.
-- Volatile: it share-locks the credential row while reading the key.
create function public.provider_credential_load_for_placement(p_placement_id uuid)
returns jsonb
language plpgsql
volatile
security definer
set search_path = ''
as $$
declare
  v_source text;
  v_credential uuid;
  v_vault uuid;
  v_key text;
begin
  perform treq_internal.require_service_role();
  select p.credential_source, p.credential_id
    into v_source, v_credential
    from public.remote_instance_placements p
   where p.id = p_placement_id;
  if not found then
    return null;
  end if;
  if v_credential is not null then
    select c.vault_secret_id into v_vault
      from public.remote_provider_credentials c
     where c.id = v_credential
       for share;
    if v_vault is not null then
      select s.decrypted_secret into v_key from vault.decrypted_secrets s where s.id = v_vault;
    end if;
  end if;
  return jsonb_build_object('credential_source', v_source, 'credential_id', v_credential, 'key', v_key);
end;
$$;

-- A provider answered 401/403 with this key.
create function public.provider_credential_mark_invalid(p_credential_id uuid)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  update public.remote_provider_credentials
     set validated_at = null, invalid_at = now()
   where id = p_credential_id;
end;
$$;

create function public.list_provider_credentials()
returns table (id uuid, provider_kind text, label text, validated_at timestamptz,
               invalid_at timestamptz, created_at timestamptz)
language sql
stable
security definer
set search_path = ''
as $$
  select c.id, c.provider_kind, c.label, c.validated_at, c.invalid_at, c.created_at
    from public.remote_provider_credentials c
   where c.owner_user_id = (select auth.uid())
   order by c.provider_kind;
$$;

create function public.delete_provider_credential(p_kind text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_user uuid := (select auth.uid());
begin
  if v_user is null then
    raise exception 'not_authenticated' using errcode = '42501';
  end if;
  -- Lock first: a concurrent placement insert holds or waits on a key share
  -- lock on this row, so the check below sees its placement.
  perform 1 from public.remote_provider_credentials
    where owner_user_id = v_user and provider_kind = p_kind
    for update;
  if exists (
    select 1 from public.remote_instance_placements p
      join public.remote_provider_credentials c on c.id = p.credential_id
     where c.owner_user_id = v_user and c.provider_kind = p_kind and p.role <> 'dead'
  ) then
    raise exception 'credential_in_use';
  end if;
  delete from public.remote_provider_credentials
   where owner_user_id = v_user and provider_kind = p_kind;
end;
$$;

-- Machine types per provider the caller has a key for: provider list price
-- per hour and what treq bills for it.
create function public.get_byos_catalog()
returns jsonb
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce(jsonb_agg(jsonb_build_object(
           'provider_kind', c.provider_kind,
           'provider_name', sp.display_name,
           'credential_valid', c.invalid_at is null,
           'tier_id', t.tier_id,
           'machine_type_id', t.id,
           'regions', to_jsonb(t.regions),
           'provider_hourly_usd', round(p.running_nanousd_per_sec * 3600 / 1e9, 6),
           'hourly_usd', round(treq_internal.billed_nanousd(
              p.running_nanousd_per_sec * 3600, p.markup_bps, 'byos') / 1e9, 6)
         ) order by c.provider_kind, st.sort), '[]'::jsonb)
    from public.remote_provider_credentials c
    join public.sandbox_providers sp on sp.kind = c.provider_kind
    join lateral (
      select distinct on (m.tier_id) m.*
        from public.provider_machine_types m
       where m.provider_kind = c.provider_kind
       order by m.tier_id, m.selectable desc, m.id
    ) t on true
    join public.size_tiers st on st.id = t.tier_id
    join public.machine_type_prices p on p.id = treq_internal.current_price_id(t.id, now())
   where c.owner_user_id = (select auth.uid());
$$;

revoke all on function treq_internal.user_can_byos(uuid) from public, anon, authenticated, service_role;
revoke all on function treq_internal.byos_markup_bps() from public, anon, authenticated;
revoke all on function treq_internal.billed_nanousd(numeric, int, text) from public, anon, authenticated;
revoke all on function treq_internal.provider_credential_delete_vault() from public, anon, authenticated;
revoke all on function public.can_byos(uuid) from public, anon, authenticated;
revoke all on function public.provider_credential_save(uuid, text, text, text) from public, anon, authenticated;
revoke all on function public.provider_credential_load_for_placement(uuid) from public, anon, authenticated;
revoke all on function public.provider_credential_mark_invalid(uuid) from public, anon, authenticated;
grant execute on function public.can_byos(uuid) to service_role;
grant execute on function public.provider_credential_save(uuid, text, text, text) to service_role;
grant execute on function public.provider_credential_load_for_placement(uuid) to service_role;
grant execute on function public.provider_credential_mark_invalid(uuid) to service_role;
revoke all on function public.list_provider_credentials() from public, anon;
revoke all on function public.delete_provider_credential(text) from public, anon;
revoke all on function public.get_byos_catalog() from public, anon;
grant execute on function public.list_provider_credentials() to authenticated;
grant execute on function public.delete_provider_credential(text) to authenticated;
grant execute on function public.get_byos_catalog() to authenticated;
