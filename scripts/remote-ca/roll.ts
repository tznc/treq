#!/usr/bin/env -S deno run --allow-read --allow-env --allow-net
// Rolls the SSH CA secrets one step at a time (TREQ-342; runbook in README.md).
//
// Usage:
//   SUPABASE_ACCESS_TOKEN=... TREQ_SUPABASE_URL=https://<ref>.supabase.co REMOTE_ADMIN_API_KEY=... \
//     deno run --allow-read --allow-env --allow-net scripts/remote-ca/roll.ts <step> <files>
//
//   stage <current> <next>      trust both CAs, sweep running machines
//   promote <current> <next>    sign with next (both stay trusted)
//   retire <next>               trust next alone, sweep (20+ minutes after promote)
//   emergency <next>            sign with and trust next alone at once, sweep
//   sweep                       rewrite the trusted CAs on running machines again
//
// Each step waits for the functions to load the new secrets, sweeps when the
// trusted set changed, then runs verify.ts's checks. Exits 1 on any problem.

import { caFingerprint } from "../../supabase/functions/_shared/remote/ssh-cert.ts";
import { base64Decode } from "../../supabase/functions/_shared/remote/ssh-wire.ts";
import {
  adminCall,
  applySecrets,
  type CaStatus,
  checkCaStatus,
  parseCaFile,
  planRoll,
  type RollStep,
  sweepAll,
} from "./lib.ts";

const NEXT_STEP: Record<RollStep, string> = {
  stage: "Next: roll.ts promote <current> <next>",
  promote: "Wait at least 20 minutes for certificates from the old CA to expire, then: roll.ts retire <next>",
  retire: "Done. Keep <next> as the current CA file.",
  emergency: "Done. Keep <next> as the current CA file.",
};

async function sweep(): Promise<boolean> {
  const failed = await sweepAll();
  for (const f of failed) console.error(`FAIL sweep ${f.instance_id}: ${f.error}`);
  return failed.length === 0;
}

const [step, ...paths] = Deno.args;
if (step === "sweep") Deno.exit(await sweep() ? 0 : 1);
if (!step || !(step in NEXT_STEP) || paths.length === 0) {
  console.error("usage: roll.ts stage|promote <current> <next> | retire|emergency <next> | sweep");
  Deno.exit(2);
}
const files = await Promise.all(paths.map(async (p) => parseCaFile(await Deno.readTextFile(p))));
const next = files[files.length - 1];
if (!next) Deno.exit(2);
const plan = planRoll({ step: step as RollStep, current: files.length > 1 ? files[0] : undefined, next });

const fingerprint = (publicKey: string) => caFingerprint(base64Decode(publicKey));
const expected = {
  signing: await fingerprint(plan.expect.signing),
  trusted: await Promise.all(plan.expect.trusted.map(fingerprint)),
};

await applySecrets(plan);
console.log(`Set [${Object.keys(plan.set).join(", ")}], unset [${plan.unset.join(", ")}]`);

// New secrets reach the functions within seconds; wait until ca_status shows them.
let problems: string[] = [];
for (let attempt = 0; attempt < 12; attempt++) {
  const status = await adminCall("ca_status") as CaStatus;
  problems = checkCaStatus("ca_error" in status ? status : { ...status, stale_machines: [] }, expected);
  if (problems.length === 0) break;
  await new Promise((resolve) => setTimeout(resolve, 5_000));
}
for (const problem of problems) console.error(`FAIL ${problem}`);
if (problems.length > 0) Deno.exit(1);

if (plan.sweep && !(await sweep())) Deno.exit(1);
const final = checkCaStatus(await adminCall("ca_status") as CaStatus, expected);
for (const problem of final) console.error(`FAIL ${problem}`);
if (final.length > 0) Deno.exit(1);
console.log(`OK signing ${expected.signing}; trusted ${expected.trusted.join(", ")}`);
console.log(NEXT_STEP[step as RollStep]);
