// OpenSSH public-key line parsing and fingerprinting, per the PRD's "Client
// key policy": Treq only ever stores and reasons about public material.

import { base64Decode, sshFingerprintSha256, SshReader } from "./ssh-wire.ts";

export type SupportedKeyAlgorithm = "ssh-ed25519";

export interface ParsedPublicKey {
  algorithm: string;
  /// Raw SSH-wire key blob (the full `string algorithm, ... key fields`
  /// structure), exactly as it appears base64-encoded in an
  /// authorized_keys-format line.
  blob: Uint8Array;
  comment: string | null;
  fingerprintSha256: string;
}

export class UnsupportedKeyError extends Error {}

// Parses a single "authorized_keys"-format line: "<algorithm> <base64> [comment]".
export async function parseOpenSshPublicKey(line: string): Promise<ParsedPublicKey> {
  const [algorithm, encoded, ...commentParts] = line.trim().split(/\s+/);
  if (encoded === undefined) {
    throw new UnsupportedKeyError("public key must be in 'algorithm base64 [comment]' format");
  }
  let blob: Uint8Array;
  try {
    blob = base64Decode(encoded);
  } catch {
    throw new UnsupportedKeyError("public key base64 payload is invalid");
  }
  // The blob's own embedded algorithm name must match the line's prefix -
  // this is what stops a client from mislabeling a key's algorithm.
  const reader = new SshReader(blob);
  let embeddedAlgorithm: string;
  try {
    embeddedAlgorithm = reader.readUtf8String();
  } catch {
    throw new UnsupportedKeyError("public key blob is malformed");
  }
  if (embeddedAlgorithm !== algorithm) {
    throw new UnsupportedKeyError("public key algorithm does not match its encoded blob");
  }
  const fingerprintSha256 = await sshFingerprintSha256(blob);
  return {
    algorithm,
    blob,
    comment: commentParts.length > 0 ? commentParts.join(" ") : null,
    fingerprintSha256,
  };
}

// Extracts the raw 32-byte ed25519 public key from a parsed ssh-ed25519 key
// blob (string "ssh-ed25519", string <32-byte key>).
export function extractEd25519RawKey(parsed: ParsedPublicKey): Uint8Array {
  if (parsed.algorithm !== "ssh-ed25519") {
    throw new UnsupportedKeyError(`unsupported key algorithm '${parsed.algorithm}'`);
  }
  const reader = new SshReader(parsed.blob);
  reader.readUtf8String(); // algorithm name, already validated
  const key = reader.readString();
  if (key.length !== 32) {
    throw new UnsupportedKeyError("ssh-ed25519 key blob has an unexpected length");
  }
  return key;
}
