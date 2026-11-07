-- Per-placement integration secrets, kept in Vault like 051's gateway
-- secrets. machine-setup writes each to /etc/treq/integration.json on the
-- machine; its sandbox signs integration-proxy requests with it, and
-- integration-proxy checks them here.

create table public.remote_integration_secrets (
  placement_id uuid primary key
    references public.remote_instance_placements (id) on delete cascade,
  secret_id uuid not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

alter table public.remote_integration_secrets enable row level security;
revoke all on table public.remote_integration_secrets from public, anon, authenticated;

-- The placement's secret, decrypted; null when it has none.
create function public.integration_secret_load(p_placement_id uuid)
returns text
language sql
security definer
set search_path = ''
as $$
  select s.decrypted_secret
  from public.remote_integration_secrets i
  join vault.decrypted_secrets s on s.id = i.secret_id
  where i.placement_id = p_placement_id;
$$;

-- Saves or replaces the placement's secret.
create function public.integration_secret_save(p_placement_id uuid, p_secret text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_secret_id uuid;
begin
  select secret_id into v_secret_id
  from public.remote_integration_secrets
  where placement_id = p_placement_id
  for update;

  if found then
    perform vault.update_secret(v_secret_id, p_secret);
    update public.remote_integration_secrets set updated_at = now()
    where placement_id = p_placement_id;
    return;
  end if;

  insert into public.remote_integration_secrets (placement_id, secret_id)
  values (
    p_placement_id,
    vault.create_secret(p_secret, 'treq-integration:' || p_placement_id,
      'treq integration secret for placement ' || p_placement_id)
  );
end;
$$;

-- Deleting a placement (or its instance or user) deletes the Vault secret.
create function public.integration_secret_delete_vault()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  delete from vault.secrets where id = old.secret_id;
  return old;
end;
$$;

create trigger remote_integration_secrets_delete_vault
after delete on public.remote_integration_secrets
for each row execute function public.integration_secret_delete_vault();

revoke all on function public.integration_secret_load(uuid) from public, anon, authenticated;
grant execute on function public.integration_secret_load(uuid) to service_role;
revoke all on function public.integration_secret_save(uuid, text) from public, anon, authenticated;
grant execute on function public.integration_secret_save(uuid, text) to service_role;
revoke all on function public.integration_secret_delete_vault() from public, anon, authenticated;
