// Minimal SSH wire-format primitives (RFC 4251 section 5), shared by
// certificate signing (ssh-cert.ts) and host-key scanning (ssh-keyscan.ts).
// No SSH library dependency: these are the handful of encode/decode rules
// (string, uint32, uint64, mpint) actually needed for those two jobs.

export class SshWriter {
  private chunks: Uint8Array[] = [];

  writeUint32(value: number): this {
    const buf = new Uint8Array(4);
    new DataView(buf.buffer).setUint32(0, value >>> 0, false);
    this.chunks.push(buf);
    return this;
  }

  writeUint64(value: bigint): this {
    const buf = new Uint8Array(8);
    new DataView(buf.buffer).setBigUint64(0, value, false);
    this.chunks.push(buf);
    return this;
  }

  writeRaw(bytes: Uint8Array): this {
    this.chunks.push(bytes);
    return this;
  }

  writeString(bytes: Uint8Array | string): this {
    const data = typeof bytes === "string" ? new TextEncoder().encode(bytes) : bytes;
    this.writeUint32(data.length);
    this.chunks.push(data);
    return this;
  }

  // Encodes a list of strings as one SSH "string" field: NUL-joined entries
  // wrapped in an outer length-prefixed string, per PROTOCOL.certkeys'
  // "valid principals" / extensions encoding.
  writeNameList(names: string[]): this {
    const joined = names.join("\0");
    this.writeString(joined);
    return this;
  }

  // Positive mpint per RFC 4251: a leading 0x00 byte is prepended when the
  // high bit of the first byte would otherwise be set, so the value cannot be
  // misread as negative.
  writeMpintFromUnsigned(bytes: Uint8Array): this {
    let value = bytes;
    if (((value[0] ?? 0) & 0x80) !== 0) {
      const padded = new Uint8Array(value.length + 1);
      padded.set(value, 1);
      value = padded;
    }
    this.writeString(value);
    return this;
  }

  toBytes(): Uint8Array {
    const total = this.chunks.reduce((sum, c) => sum + c.length, 0);
    const out = new Uint8Array(total);
    let offset = 0;
    for (const chunk of this.chunks) {
      out.set(chunk, offset);
      offset += chunk.length;
    }
    return out;
  }
}

export class SshReader {
  private offset = 0;
  constructor(private readonly data: Uint8Array) {}

  get remaining(): number {
    return this.data.length - this.offset;
  }

  readUint32(): number {
    const view = new DataView(this.data.buffer, this.data.byteOffset + this.offset, 4);
    this.offset += 4;
    return view.getUint32(0, false);
  }

  readBytes(length: number): Uint8Array {
    const out = this.data.subarray(this.offset, this.offset + length);
    this.offset += length;
    return out;
  }

  readString(): Uint8Array {
    const length = this.readUint32();
    return this.readBytes(length);
  }

  readUtf8String(): string {
    return new TextDecoder().decode(this.readString());
  }
}

export function base64Encode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function base64Decode(value: string): Uint8Array {
  const binary = atob(value);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

export async function sha256(bytes: Uint8Array): Promise<Uint8Array> {
  const digest = await crypto.subtle.digest("SHA-256", bytes as BufferSource);
  return new Uint8Array(digest);
}

// OpenSSH's `SHA256:<base64-no-padding>` fingerprint format.
export async function sshFingerprintSha256(keyBlob: Uint8Array): Promise<string> {
  const digest = await sha256(keyBlob);
  const b64 = base64Encode(digest).replace(/=+$/, "");
  return `SHA256:${b64}`;
}
