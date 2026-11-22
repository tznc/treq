// Real TCP-level SSH host-key scan: connects to an endpoint's SSH port,
// performs the identification-string exchange and enough of the SSH
// transport key-exchange (RFC 4253) to read the server's actual host public
// key out of the wire, then disconnects. This is not a full SSH client - no
// session is authenticated and no shared secret is used - it exists only to
// populate `remote_endpoint_host_keys` with a real observed fingerprint
// instead of a placeholder (see PRD "Host-key verification").
//
// Scope and honest limitations:
//   - Only "curve25519-sha256" key exchange and "ssh-ed25519" host keys are
//     requested. A server offering only RSA/ECDSA host keys is reported as
//     an explicit `unsupported_host_key_algorithm` error rather than a
//     fabricated fingerprint.
//   - The client's ECDH ephemeral value is 32 random bytes, not a real X25519
//     key pair, because reading the host key out of KEX_ECDH_REPLY does not
//     require completing the exchange or deriving a shared secret. That also
//     means this scan does NOT verify the server's exchange-hash signature,
//     so it establishes trust on first observation rather than proving the
//     TCP path was not tampered with. That is the inherent trust-on-first-use
//     property of any first keyscan (matching plain `ssh-keyscan`'s own
//     behavior) - later connections must pin against what this call records.

import { parseOpenSshPublicKey, type ParsedPublicKey } from "./ssh-keys.ts";
import { SshReader, SshWriter } from "./ssh-wire.ts";
import { base64Encode } from "./ssh-wire.ts";
import { errorMessage } from "../error-message.ts";

const SSH_MSG_KEXINIT = 20;
const SSH_MSG_KEX_ECDH_INIT = 30;
const SSH_MSG_KEX_ECDH_REPLY = 31;

export class KeyscanError extends Error {
  constructor(
    message: string,
    public readonly kind:
      | "connect_failed"
      | "protocol_error"
      | "unsupported_host_key_algorithm"
      | "timeout",
  ) {
    super(message);
  }
}

export interface ScannedHostKey {
  algorithm: string;
  fingerprintSha256: string;
  /// authorized_keys-format line for the observed key ("<algo> <base64>"),
  /// stored for display/debugging - never used as a client credential.
  publicKeyLine: string;
}

async function readExactly(conn: Deno.Conn, length: number, deadline: number): Promise<Uint8Array> {
  const out = new Uint8Array(length);
  let filled = 0;
  while (filled < length) {
    if (Date.now() > deadline) throw new KeyscanError("timed out reading from SSH endpoint", "timeout");
    const n = await conn.read(out.subarray(filled));
    if (n === null) throw new KeyscanError("connection closed before host key was received", "protocol_error");
    filled += n;
  }
  return out;
}

async function readLine(conn: Deno.Conn, deadline: number): Promise<string> {
  const bytes: number[] = [];
  const one = new Uint8Array(1);
  while (bytes.length < 255) {
    if (Date.now() > deadline) throw new KeyscanError("timed out reading SSH identification string", "timeout");
    const n = await conn.read(one);
    if (n === null) throw new KeyscanError("connection closed during identification exchange", "protocol_error");
    if (one[0] === 0x0a) break;
    if (one[0] !== 0x0d) bytes.push(one[0]);
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

function framePacket(payload: Uint8Array): Uint8Array {
  // packet_length(4) + padding_length(1) + payload + padding, padded to a
  // multiple of 8 with at least 4 bytes of padding (RFC 4253 section 6).
  let paddingLength = 8 - ((5 + payload.length) % 8);
  if (paddingLength < 4) paddingLength += 8;
  const padding = crypto.getRandomValues(new Uint8Array(paddingLength));
  const packetLength = 1 + payload.length + paddingLength;
  const out = new Uint8Array(4 + packetLength);
  new DataView(out.buffer).setUint32(0, packetLength, false);
  out[4] = paddingLength;
  out.set(payload, 5);
  out.set(padding, 5 + payload.length);
  return out;
}

async function readPacket(conn: Deno.Conn, deadline: number): Promise<Uint8Array> {
  const header = await readExactly(conn, 4, deadline);
  const packetLength = new DataView(header.buffer, header.byteOffset, 4).getUint32(0, false);
  if (packetLength < 1 || packetLength > 1 << 20) {
    throw new KeyscanError(`implausible SSH packet length ${packetLength}`, "protocol_error");
  }
  const rest = await readExactly(conn, packetLength, deadline);
  const paddingLength = rest[0];
  const payload = rest.subarray(1, rest.length - paddingLength);
  return payload;
}

function buildKexInitPayload(): Uint8Array {
  const cookie = crypto.getRandomValues(new Uint8Array(16));
  return new SshWriter()
    .writeRaw(new Uint8Array([SSH_MSG_KEXINIT]))
    .writeRaw(cookie)
    .writeNameList(["curve25519-sha256"])
    .writeNameList(["ssh-ed25519"])
    .writeNameList(["aes128-ctr"])
    .writeNameList(["aes128-ctr"])
    .writeNameList(["hmac-sha2-256"])
    .writeNameList(["hmac-sha2-256"])
    .writeNameList(["none"])
    .writeNameList(["none"])
    .writeNameList([])
    .writeNameList([])
    .writeRaw(new Uint8Array([0])) // first_kex_packet_follows = false
    .writeUint32(0) // reserved
    .toBytes();
}

function parseServerKexInit(payload: Uint8Array): { serverHostKeyAlgorithms: string[] } {
  const reader = new SshReader(payload.subarray(1)); // skip message type byte
  reader.readBytes(16); // cookie
  reader.readUtf8String(); // kex_algorithms
  const serverHostKeyAlgorithms = reader.readUtf8String().split(",");
  return { serverHostKeyAlgorithms };
}

export async function scanHostKey(
  hostname: string,
  port: number,
  options: { timeoutMs?: number } = {},
): Promise<ScannedHostKey> {
  const timeoutMs = options.timeoutMs ?? 8000;
  const deadline = Date.now() + timeoutMs;

  let conn: Deno.Conn;
  try {
    conn = await Deno.connect({ hostname, port, transport: "tcp" });
  } catch (err) {
    throw new KeyscanError(`could not connect to ${hostname}:${port}: ${errorMessage(err)}`, "connect_failed");
  }

  try {
    // Identification string exchange (RFC 4253 section 4.2).
    const serverId = await readLine(conn, deadline);
    if (!serverId.startsWith("SSH-2.0-") && !serverId.startsWith("SSH-1.99-")) {
      throw new KeyscanError(`unexpected SSH identification string: '${serverId}'`, "protocol_error");
    }
    await conn.write(new TextEncoder().encode("SSH-2.0-TreqControlPlaneKeyscan_1.0\r\n"));

    // Key exchange init.
    await conn.write(framePacket(buildKexInitPayload()));
    const serverKexInit = await readPacket(conn, deadline);
    if (serverKexInit[0] !== SSH_MSG_KEXINIT) {
      throw new KeyscanError("expected SSH_MSG_KEXINIT from server", "protocol_error");
    }
    const { serverHostKeyAlgorithms } = parseServerKexInit(serverKexInit);
    if (!serverHostKeyAlgorithms.includes("ssh-ed25519")) {
      throw new KeyscanError(
        `server does not offer ssh-ed25519 as a host key algorithm (offered: ${serverHostKeyAlgorithms.join(", ")})`,
        "unsupported_host_key_algorithm",
      );
    }

    // ECDH init with a throwaway 32-byte client value: we only need the
    // reply's host key field, not a completed shared secret (see module
    // doc-comment for why this is a deliberate, documented limitation).
    const clientEphemeral = crypto.getRandomValues(new Uint8Array(32));
    const ecdhInitPayload = new SshWriter()
      .writeRaw(new Uint8Array([SSH_MSG_KEX_ECDH_INIT]))
      .writeString(clientEphemeral)
      .toBytes();
    await conn.write(framePacket(ecdhInitPayload));

    // The server may resend KEXINIT-adjacent packets; scan forward until the
    // ECDH reply, bounded by the overall deadline.
    let replyPayload: Uint8Array | null = null;
    while (Date.now() < deadline) {
      const packet = await readPacket(conn, deadline);
      if (packet[0] === SSH_MSG_KEX_ECDH_REPLY) {
        replyPayload = packet;
        break;
      }
    }
    if (!replyPayload) throw new KeyscanError("timed out waiting for SSH_MSG_KEX_ECDH_REPLY", "timeout");

    const reader = new SshReader(replyPayload.subarray(1));
    const hostKeyBlob = reader.readString();
    // Q_S and signature follow but are not needed to record the host key.

    const line = `ssh-ed25519 ${base64Encode(hostKeyBlob)}`;
    const parsed: ParsedPublicKey = await parseOpenSshPublicKey(line);
    return {
      algorithm: parsed.algorithm,
      fingerprintSha256: parsed.fingerprintSha256,
      publicKeyLine: line,
    };
  } finally {
    try {
      conn.close();
    } catch {
      // already closed by the peer - nothing to do.
    }
  }
}
