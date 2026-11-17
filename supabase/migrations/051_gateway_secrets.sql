-- Per-placement secrets for treq-gateway (crates/treq-gateway), kept in
-- Vault like 032's Linear tokens. The relay signs a short-lived token with
-- the secret on every connect; the gateway reads it from
-- /etc/treq/gateway.secret.

create table public.remote_gateway_secrets (
  placement_id uuid primary key
    references public.remote_instance_placements (id) on delete cascade,
  secret_id uuid not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

alter table public.remote_gateway_secrets enable row level security;
revoke all on table public.remote_gateway_secrets from public, anon, authenticated;

-- The placement's secret, decrypted; null when it has none.
create function public.gateway_secret_load(p_placement_id uuid)
returns text
language sql
security definer
set search_path = ''
as $$
  select s.decrypted_secret
  from public.remote_gateway_secrets g
  join vault.decrypted_secrets s on s.id = g.secret_id
  where g.placement_id = p_placement_id;
$$;

-- Saves or replaces the placement's secret.
create function public.gateway_secret_save(p_placement_id uuid, p_secret text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_secret_id uuid;
begin
  select secret_id into v_secret_id
  from public.remote_gateway_secrets
  where placement_id = p_placement_id
  for update;

  if found then
    perform vault.update_secret(v_secret_id, p_secret);
    update public.remote_gateway_secrets set updated_at = now()
    where placement_id = p_placement_id;
    return;
  end if;

  insert into public.remote_gateway_secrets (placement_id, secret_id)
  values (
    p_placement_id,
    vault.create_secret(p_secret, 'treq-gateway:' || p_placement_id, 'treq-gateway secret for placement ' || p_placement_id)
  );
end;
$$;

-- Deleting a placement (or its instance or user) deletes the Vault secret.
create function public.gateway_secret_delete_vault()
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

create trigger remote_gateway_secrets_delete_vault
after delete on public.remote_gateway_secrets
for each row execute function public.gateway_secret_delete_vault();

revoke all on function public.gateway_secret_load(uuid) from public, anon, authenticated;
grant execute on function public.gateway_secret_load(uuid) to service_role;
revoke all on function public.gateway_secret_save(uuid, text) from public, anon, authenticated;
grant execute on function public.gateway_secret_save(uuid, text) to service_role;
revoke all on function public.gateway_secret_delete_vault() from public, anon, authenticated;
