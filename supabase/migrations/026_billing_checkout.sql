-- Customer mapping for billing-checkout (prds/billing-and-teams.md, rollout
-- step 2).
--
-- billing-checkout creates a Stripe customer for a user before it opens
-- Checkout, then records it here. The webhook's
-- billing_record_checkout_completed later confirms the same mapping. Two
-- checkouts that race for the same user each create a customer (Stripe's
-- idempotency key usually returns the same one), and this function keeps
-- whichever was recorded first, so the user keeps one customer, one trial
-- and one billing history.

-- Records p_customer_id as the owner's Stripe customer unless the owner
-- already has one, and returns the owner's customer either way. Raises when
-- the customer belongs to someone else or the user does not exist.
create function public.billing_attach_customer(
  p_owner_type text,
  p_owner_id uuid,
  p_customer_id text
)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_customer_id text;
begin
  -- Organizations arrive with Team in rollout step 4.
  if p_owner_type is distinct from 'user' then
    raise exception 'unsupported billing owner type %', p_owner_type
      using errcode = 'invalid_parameter_value';
  end if;

  if not exists (select 1 from auth.users u where u.id = p_owner_id) then
    raise exception 'unknown billing owner %', p_owner_id;
  end if;

  insert into public.billing_customers (owner_type, owner_id, stripe_customer_id)
  values (p_owner_type, p_owner_id, p_customer_id)
  on conflict do nothing;

  select c.stripe_customer_id
    into v_customer_id
  from public.billing_customers c
  where c.owner_type = p_owner_type
    and c.owner_id = p_owner_id;

  if v_customer_id is null then
    raise exception 'Stripe customer % belongs to another owner', p_customer_id;
  end if;

  return v_customer_id;
end;
$$;

revoke all on function public.billing_attach_customer(text, uuid, text)
  from public, anon, authenticated;
grant execute on function public.billing_attach_customer(text, uuid, text)
  to service_role;
