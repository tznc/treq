# SSH CA operator scripts

Treq Cloud machines accept SSH certificates signed by one certificate authority (CA). The CA lives in Edge Function secrets:

| Secret | Holds |
|---|---|
| `REMOTE_SSH_CA_ED25519_SEED_BASE64` | The private key (raw 32-byte seed). It signs the 20-minute certificates. |
| `REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64` | The matching public key. |
| `REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64` | Optional. Public keys machines trust during a roll, comma separated. Unset means the signing CA alone. It must include the signing CA. |

Machines write the trusted keys to `/etc/ssh/treq_ca.pub` when they are created and on every wake. sshd reads that file on every login, so a change applies without a restart. The `refresh_ca_trust` admin action writes it on running machines right away.

Anyone with the seed can log in to every user's cloud machine. Keep CA files in a password manager and delete local copies after use.

## Setup

Run the scripts with Deno from the repository root. They need:

| Variable | For |
|---|---|
| `TREQ_SUPABASE_URL` | The project URL, `https://<ref>.supabase.co`. |
| `REMOTE_ADMIN_API_KEY` | The `remote-admin` operator key. |
| `SUPABASE_ACCESS_TOKEN` | A Supabase personal access token. Only `roll.ts` needs it, to set secrets. |
| `SUPABASE_ANON_KEY` | Optional. Sent as the bearer token if the function checks JWTs. |

## Generate a CA

```bash
deno run --allow-read --allow-write scripts/remote-ca/generate.ts --out treq-ca-2026-11.env
```

This writes a new file with mode 600 and prints the fingerprint. It never prints the seed. The file uses the `NAME=value` format that `supabase secrets set --env-file` reads.

## Verify

```bash
deno run --allow-read --allow-env --allow-net scripts/remote-ca/verify.ts \
  --signing current.env [--trust old.env --trust new.env]
```

`verify.ts` asks `remote-admin ca_status` what the deployed functions load. It never reads a secret value. It checks four things:

- The signing CA is the one in `--signing`.
- The seed matches its public key.
- The trusted CAs are exactly the `--trust` files. With no `--trust`, it expects the signing CA alone.
- Every running machine last recorded that trusted set.

It exits 1 and lists each problem. Stopped machines are not listed, because they write the current set when they wake.

## Planned roll

Each `roll.ts` step sets the secrets and waits until the functions load them. If the trusted set changed, it then sweeps running machines. It finishes with the same checks as `verify.ts` and prints the next step.

```bash
R="deno run --allow-read --allow-env --allow-net scripts/remote-ca/roll.ts"
$R stage   current.env next.env   # trust both CAs, sweep
$R promote current.env next.env   # sign with next, both stay trusted
# wait at least 20 minutes, so certificates from the current CA expire
$R retire  next.env               # trust next alone, sweep
```

After `retire`, `next.env` is the current CA file.

## Emergency roll

Use this when the seed may have leaked. It signs with and trusts only the new CA at once, then sweeps running machines.

```bash
$R emergency next.env
```

New logins with certificates from the old CA fail as soon as the sweep reaches a machine. Open sessions stay up until they disconnect. If the sweep reports failures, fix the machine and run `$R sweep` again.

## Roll back

- Before `retire`, sign with the old CA again: `$R promote next.env current.env`.
- After `retire`, start a new roll back to the old CA: `$R stage next.env current.env`, then `promote` and `retire`.

## Next steps

- Linear issue `TREQ-342` tracks the first end-to-end test of these steps.
- `supabase/functions/_shared/remote/ca-status.ts` and `ca-trust-refresh.ts` hold the admin actions these scripts call.
