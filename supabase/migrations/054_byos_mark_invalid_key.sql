-- BYOS chaos QA fix (2026-11-01).

-- 3. A 401 marks the key invalid only if it is still the key that failed:
-- a request that started before a rotation must not flag the new key.
drop function public.provider_credential_mark_invalid(uuid);
create function public.provider_credential_mark_invalid(p_credential_id uuid, p_key text)
returns void
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  update public.remote_provider_credentials c
     set validated_at = null, invalid_at = now()
   where c.id = p_credential_id
     and exists (select 1 from vault.decrypted_secrets s
                  where s.id = c.vault_secret_id and s.decrypted_secret = p_key);
end;
$$;

revoke all on function public.provider_credential_mark_invalid(uuid, text) from public, anon, authenticated;
grant execute on function public.provider_credential_mark_invalid(uuid, text) to service_role;
