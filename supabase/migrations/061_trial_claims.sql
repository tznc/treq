-- One Pro trial per email. trial_used_at lives on the Stripe customer, and a
-- deleted account's next sign-up gets a new user and a new customer, so
-- delete-and-sign-up-again earned a fresh 14-day trial each time. A claim
-- keyed by a hash of the normalised email outlives the account.

-- Lower case, no +tag, and for Gmail no dots and one domain.
create function treq_internal.trial_email_key(p_email text)
returns text
language sql
immutable
set search_path = ''
as $$
  with parts as (
    select split_part(lower(trim(p_email)), '@', 1) as local,
           split_part(lower(trim(p_email)), '@', 2) as domain
  ),
  norm as (
    select case when domain in ('gmail.com', 'googlemail.com')
                then replace(split_part(local, '+', 1), '.', '')
                else split_part(local, '+', 1) end as local,
           case when domain = 'googlemail.com' then 'gmail.com' else domain end as domain
      from parts
  )
  select encode(sha256(convert_to(local || '@' || domain, 'UTF8')), 'hex') from norm;
$$;

create table public.billing_trial_claims (
  email_key text primary key,
  claimed_at timestamptz not null default now()
);
alter table public.billing_trial_claims enable row level security;
revoke all on public.billing_trial_claims from public, anon, authenticated;

create function treq_internal.record_trial_claim()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  if new.owner_type = 'user' and new.trial_used_at is not null then
    insert into public.billing_trial_claims (email_key)
    select treq_internal.trial_email_key(u.email)
      from auth.users u
     where u.id = new.owner_id and u.email is not null
    on conflict (email_key) do nothing;
  end if;
  return null;
end;
$$;

create trigger billing_customers_record_trial_claim
after insert or update of trial_used_at on public.billing_customers
for each row execute function treq_internal.record_trial_claim();

insert into public.billing_trial_claims (email_key)
select distinct treq_internal.trial_email_key(u.email)
  from public.billing_customers c
  join auth.users u on u.id = c.owner_id
 where c.owner_type = 'user' and c.trial_used_at is not null and u.email is not null
on conflict (email_key) do nothing;

create function public.billing_trial_claimed(p_user_id uuid)
returns boolean
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  perform treq_internal.require_service_role();
  return exists (
    select 1
      from auth.users u
      join public.billing_trial_claims t on t.email_key = treq_internal.trial_email_key(u.email)
     where u.id = p_user_id
  );
end;
$$;

revoke all on function treq_internal.trial_email_key(text) from public, anon, authenticated;
revoke all on function treq_internal.record_trial_claim() from public, anon, authenticated;
revoke all on function public.billing_trial_claimed(uuid) from public, anon, authenticated;
grant execute on function public.billing_trial_claimed(uuid) to service_role;
