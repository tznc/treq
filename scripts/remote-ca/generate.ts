#!/usr/bin/env -S deno run --allow-read --allow-write
// Generates a new SSH CA key pair for Treq Cloud (TREQ-342).
//
// Usage:
//   deno run --allow-read --allow-write scripts/remote-ca/generate.ts [--out treq-ca.env]
//
// Writes the seed and public key to a new file (mode 600) and prints the
// fingerprint; the seed is never printed. Keep the file in a password
// manager. roll.ts sets the secrets from it; verify.ts checks them.

import { caFingerprint } from "../../supabase/functions/_shared/remote/ssh-cert.ts";
import { base64Decode } from "../../supabase/functions/_shared/remote/ssh-wire.ts";
import { formatCaFile, generateCa } from "./lib.ts";

const at = Deno.args.indexOf("--out");
const out = at >= 0 ? Deno.args[at + 1] : "treq-ca.env";
if (!out) {
  console.error("usage: generate.ts [--out <file>]");
  Deno.exit(2);
}

const ca = await generateCa();
try {
  await Deno.writeTextFile(out, formatCaFile(ca), { mode: 0o600, createNew: true });
} catch (err) {
  if (!(err instanceof Deno.errors.AlreadyExists)) throw err;
  console.error(`${out} already exists; pick another --out`);
  Deno.exit(1);
}
console.log(`Wrote ${out} (mode 600)`);
console.log(`Fingerprint: ${await caFingerprint(base64Decode(ca.publicKey))}`);
