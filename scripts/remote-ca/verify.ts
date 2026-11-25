#!/usr/bin/env -S deno run --allow-read --allow-env --allow-net
// Verifies the SSH CA the deployed Edge Functions use (TREQ-342): the
// signing CA is the one in --signing, its seed matches its public key, the
// trusted CAs are exactly the --trust files (default: the signing CA alone),
// and every running machine last recorded that trusted set. Reads what the
// functions see through remote-admin `ca_status`; no secret value is read.
//
// Usage:
//   TREQ_SUPABASE_URL=https://<ref>.supabase.co REMOTE_ADMIN_API_KEY=... \
//     deno run --allow-read --allow-env --allow-net scripts/remote-ca/verify.ts \
//     --signing new.env [--trust old.env --trust new.env]
//
// SUPABASE_ANON_KEY is sent as the bearer token when set. Exits 1 on any problem.

import { caFingerprint } from "../../supabase/functions/_shared/remote/ssh-cert.ts";
import { base64Decode } from "../../supabase/functions/_shared/remote/ssh-wire.ts";
import { adminCall, type CaStatus, checkCaStatus, parseCaFile } from "./lib.ts";

const files = (flag: string) => Deno.args.flatMap((arg, i) => arg === flag && Deno.args[i + 1] ? [Deno.args[i + 1]] : []);
const fingerprintOf = async (path: string) =>
  await caFingerprint(base64Decode(parseCaFile(await Deno.readTextFile(path)).publicKey));

const [signingPath] = files("--signing");
if (!signingPath) {
  console.error("usage: verify.ts --signing <ca file> [--trust <ca file>]...");
  Deno.exit(2);
}
const signing = await fingerprintOf(signingPath);
const trustPaths = files("--trust");
const trusted = trustPaths.length > 0 ? await Promise.all(trustPaths.map(fingerprintOf)) : [signing];

const status = await adminCall("ca_status") as CaStatus;
console.log(JSON.stringify(status, null, 2));
const problems = checkCaStatus(status, { signing, trusted });
for (const problem of problems) console.error(`FAIL ${problem}`);
if (problems.length > 0) Deno.exit(1);
const running = "running_machines" in status ? status.running_machines : 0;
console.log(`OK signing ${signing}; trusted ${trusted.join(", ")}; ${running} running machine(s) up to date`);
