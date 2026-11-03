-- BYOS chaos QA fix (2026-11-01).

-- 1. Concurrent first saves for one owner and kind serialize.
create or replace function public.provider_credential_save(
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
  -- With no row yet, `for update` below locks nothing: two first saves
  -- would both insert and the loser fail on Vault's unique secret name.
  perform pg_advisory_xact_lock(hashtextextended('treq-byos:' || p_user_id || ':' || p_kind, 0));

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
