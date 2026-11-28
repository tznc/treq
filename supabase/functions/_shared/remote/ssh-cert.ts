// Real OpenSSH user certificate signing (`ssh-ed25519-cert-v01@openssh.com`),
// per PRD "SSH identity and certificates". The CA private key never leaves
// this module: it is read once from an Edge Function secret, used to sign,
// and discarded - it is never returned in a response or written to a table.
//
// Wire format: OpenSSH's PROTOCOL.certkeys, "ssh-ed25519-cert-v01@openssh.com"
// section:
//
//   string    "ssh-ed25519-cert-v01@openssh.com"
//   string    nonce
//   string    pk (the 32-byte ed25519 public key being certified)
//   uint64    serial
//   uint32    type (1 = user)
//   string    key id
//   string    valid principals
//   uint64    valid after
//   uint64    valid before
//   string    critical options
//   string    extensions
//   string    reserved
//   string    signature key (CA's own public key blob)
//   string    signature
//
// Only ed25519 user keys are certified today. RSA/ECDSA support is a real
// gap, not a stub: signing those cert types needs different key material
// handling and is left for a follow-up rather than faked here.

import { SshReader, SshWriter } from "./ssh-wire.ts";
import { base64Decode, base64Encode, sshFingerprintSha256 } from "./ssh-wire.ts";

const CERT_TYPE_ED25519 = "ssh-ed25519-cert-v01@openssh.com";
const CA_KEY_TYPE_ED25519 = "ssh-ed25519";
const SSH_CERT_TYPE_USER = 1;

// Extensions granted to every issued certificate: the standard interactive
// permissions OpenSSH clients expect for a login/shell/exec session. No
// critical options are set.
const DEFAULT_EXTENSIONS = [
  "permit-X11-forwarding",
  "permit-agent-forwarding",
  "permit-port-forwarding",
  "permit-pty",
  "permit-user-rc",
];

export interface CaKeyMaterial {
  /// Raw 32-byte ed25519 seed. Held only in memory for the duration of one
  /// signing call.
  privateSeed: Uint8Array;
  /// Raw 32-byte ed25519 public key, matching `privateSeed`.
  publicKey: Uint8Array;
}

// Reads the CA key pair from Edge Function secrets. Both are base64-encoded
// raw 32-byte values (not OpenSSH-formatted lines) so this module owns the
// one place that turns them into wire-format blobs.
export function caKeyMaterialFromEnv(): CaKeyMaterial {
  const seedB64 = Deno.env.get("REMOTE_SSH_CA_ED25519_SEED_BASE64");
  const publicB64 = Deno.env.get("REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64");
  if (!seedB64 || !publicB64) {
    throw new Error(
      "REMOTE_SSH_CA_ED25519_SEED_BASE64 and REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64 must be set as Edge Function secrets",
    );
  }
  const privateSeed = base64Decode(seedB64);
  const publicKey = base64Decode(publicB64);
  if (privateSeed.length !== 32 || publicKey.length !== 32) {
    throw new Error("SSH CA key material must be raw 32-byte ed25519 values");
  }
  return { privateSeed, publicKey };
}

// The CA public keys machines trust: the signing CA alone, or during a CA
// roll the comma-separated base64 raw 32-byte keys in
// REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64. The list must hold the signing
// CA, or every new certificate would be refused.
export function trustedCaKeysFromEnv(): Uint8Array[] {
  const signing = caKeyMaterialFromEnv().publicKey;
  const list = Deno.env.get("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64")?.trim();
  if (!list) return [signing];
  const keys = list.split(",").map((entry) => base64Decode(entry.trim()));
  if (keys.some((k) => k.length !== 32)) {
    throw new Error("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64 entries must be raw 32-byte ed25519 keys");
  }
  if (!keys.some((k) => k.every((byte, i) => byte === signing[i]))) {
    throw new Error("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64 must include the signing CA's public key");
  }
  return keys;
}

// One `TrustedUserCAKeys` line per trusted CA.
export function caTrustLines(keys: readonly Uint8Array[]): string[] {
  return keys.map((publicKey) => caPublicKeyLine({ publicKey }));
}

// The OpenSSH SHA256 fingerprint of a CA public key, as audit events record it.
export async function caFingerprint(publicKey: Uint8Array): Promise<string> {
  return await sshFingerprintSha256(ed25519PublicKeyBlob(publicKey));
}

// SSH-wire-format blob for an ssh-ed25519 public key: string "ssh-ed25519",
// string <32 bytes>.
export function ed25519PublicKeyBlob(rawPublicKey: Uint8Array): Uint8Array {
  return new SshWriter().writeString(CA_KEY_TYPE_ED25519).writeString(rawPublicKey).toBytes();
}

// The OpenSSH `authorized_keys`/`TrustedUserCAKeys`-format public key line
// for the CA, e.g. "ssh-ed25519 AAAA... treq-ssh-ca".
export function caPublicKeyLine(ca: Pick<CaKeyMaterial, "publicKey">, comment = "treq-ssh-ca"): string {
  const blob = ed25519PublicKeyBlob(ca.publicKey);
  return `${CA_KEY_TYPE_ED25519} ${base64Encode(blob)} ${comment}`;
}

// PKCS8 DER wrapper for a raw ed25519 seed (RFC 8410): a fixed 16-byte
// prefix followed by the 32-byte seed, since ed25519 PKCS8 has no variable
// fields at this key size.
const PKCS8_ED25519_PREFIX = new Uint8Array([
  0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
]);

async function importEd25519PrivateKey(seed: Uint8Array): Promise<CryptoKey> {
  const der = new Uint8Array(PKCS8_ED25519_PREFIX.length + seed.length);
  der.set(PKCS8_ED25519_PREFIX, 0);
  der.set(seed, PKCS8_ED25519_PREFIX.length);
  return await crypto.subtle.importKey("pkcs8", der as BufferSource, { name: "Ed25519" }, false, ["sign"]);
}

async function signEd25519(seed: Uint8Array, message: Uint8Array): Promise<Uint8Array> {
  const key = await importEd25519PrivateKey(seed);
  const signature = await crypto.subtle.sign("Ed25519", key, message as BufferSource);
  return new Uint8Array(signature);
}

export interface IssueCertificateParams {
  ca: CaKeyMaterial;
  /// Raw 32-byte ed25519 public key of the user's registered client key.
  userPublicKey: Uint8Array;
  principals: string[];
  /// Unique per-certificate serial for audit correlation. Must fit in a
  /// uint64; caller is responsible for uniqueness (a random 63-bit value is
  /// used by `randomSerial`).
  serial: bigint;
  keyId: string;
  validAfter: Date;
  validBefore: Date;
}

export interface IssuedCertificate {
  /// Full "ssh-ed25519-cert-v01@openssh.com <base64> <comment>" line, ready
  /// to write to a certificate file next to the user's private key.
  certificateLine: string;
  serial: string;
}

export function randomSerial(): bigint {
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  let value = 0n;
  for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  return value & 0x7fff_ffff_ffff_ffffn; // keep it a positive int64 for readability in logs
}

export async function issueEd25519UserCertificate(params: IssueCertificateParams): Promise<IssuedCertificate> {
  if (params.userPublicKey.length !== 32) {
    throw new Error("user public key must be a raw 32-byte ed25519 key");
  }
  const nonce = crypto.getRandomValues(new Uint8Array(32));

  const toBeSigned = new SshWriter()
    .writeString(CERT_TYPE_ED25519)
    .writeString(nonce)
    .writeString(params.userPublicKey)
    .writeUint64(params.serial)
    .writeUint32(SSH_CERT_TYPE_USER)
    .writeString(params.keyId)
    .writeNameList(params.principals)
    .writeUint64(BigInt(Math.floor(params.validAfter.getTime() / 1000)))
    .writeUint64(BigInt(Math.floor(params.validBefore.getTime() / 1000)))
    .writeString("") // critical options: none
    .writeString(encodeExtensions(DEFAULT_EXTENSIONS))
    .writeString("") // reserved
    .writeString(ed25519PublicKeyBlob(params.ca.publicKey))
    .toBytes();

  const signatureBytes = await signEd25519(params.ca.privateSeed, toBeSigned);
  const signatureBlob = new SshWriter().writeString(CA_KEY_TYPE_ED25519).writeString(signatureBytes).toBytes();

  const full = new SshWriter().writeRaw(toBeSigned).writeString(signatureBlob).toBytes();
  const comment = params.keyId;
  return {
    certificateLine: `${CERT_TYPE_ED25519} ${base64Encode(full)} ${comment}`,
    serial: params.serial.toString(),
  };
}

// Extensions are encoded as a name/value map where every value here is
// empty, per PROTOCOL.certkeys: each entry is `string name, string value`
// concatenated and wrapped in one outer string.
function encodeExtensions(names: string[]): Uint8Array {
  const writer = new SshWriter();
  for (const name of names) {
    writer.writeString(name);
    writer.writeString("");
  }
  return writer.toBytes();
}

// Parses back a certificate this module issued, for tests: returns the
// fields relevant to verifying the signing round-trip.
export function decodeCertificateForTests(certificateLine: string): {
  serial: bigint;
  principals: string[];
  validAfter: bigint;
  validBefore: bigint;
} {
  const [, encoded] = certificateLine.split(" ");
  if (encoded === undefined) throw new Error("certificate line has no base64 field");
  const bytes = base64Decode(encoded);
  const reader = new SshReader(bytes);
  reader.readUtf8String(); // cert type
  reader.readString(); // nonce
  reader.readString(); // pk
  const serialBytes = reader.readBytes(8);
  const serial = bytesToBigUint64(serialBytes);
  reader.readUint32(); // type
  reader.readString(); // key id
  const principalsRaw = new TextDecoder().decode(reader.readString());
  const principals = principalsRaw.length > 0 ? principalsRaw.split("\0") : [];
  const validAfter = bytesToBigUint64(reader.readBytes(8));
  const validBefore = bytesToBigUint64(reader.readBytes(8));
  return { serial, principals, validAfter, validBefore };
}

function bytesToBigUint64(bytes: Uint8Array): bigint {
  let value = 0n;
  for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  return value;
}
