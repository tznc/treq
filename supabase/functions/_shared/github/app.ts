// GitHub App authentication: signs an App JWT and reads installation
// metadata. Used by the installation linking flow.
//
// Required env vars:
//   GITHUB_APP_ID                 - numeric App ID
//   GITHUB_APP_PRIVATE_KEY_BASE64 - base64-encoded PEM private key

import { fetchWithTimeout } from "../http/fetch-with-timeout.ts";

function base64url(data: ArrayBuffer | string): string {
  const bytes =
    typeof data === "string" ? new TextEncoder().encode(data) : new Uint8Array(data);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=/g, "");
}

function pemToDer(pem: string): ArrayBuffer {
  const b64 = pem.replace(/-----[A-Z ]+-----/g, "").replace(/\s/g, "");
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

async function createAppJWT(appId: string, privateKeyPem: string): Promise<string> {
  const now = Math.floor(Date.now() / 1000);
  const header = base64url(JSON.stringify({ alg: "RS256", typ: "JWT" }));
  const payload = base64url(
    JSON.stringify({ iat: now - 60, exp: now + 540, iss: appId }),
  );
  const signingInput = `${header}.${payload}`;

  const key = await crypto.subtle.importKey(
    "pkcs8",
    pemToDer(privateKeyPem),
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["sign"],
  );

  const sig = await crypto.subtle.sign(
    "RSASSA-PKCS1-v1_5",
    key,
    new TextEncoder().encode(signingInput),
  );

  return `${signingInput}.${base64url(sig)}`;
}

function appCredentials(): { appId: string; privateKey: string } {
  const appId = Deno.env.get("GITHUB_APP_ID") ?? "";
  const keyB64 = Deno.env.get("GITHUB_APP_PRIVATE_KEY_BASE64") ?? "";
  if (!appId || !keyB64) {
    throw new Error(
      "GITHUB_APP_ID and GITHUB_APP_PRIVATE_KEY_BASE64 must be configured",
    );
  }
  return { appId, privateKey: atob(keyB64) };
}
// Fetch installation metadata using app-level (JWT) auth. Used by the
// installation linking flow to verify an installation actually exists and
// which account it belongs to.
export async function getInstallation(installationId: number): Promise<{
  id: number;
  account: { login: string; type: string; avatar_url: string | null };
  app_id: number;
} | null> {
  const { appId, privateKey } = appCredentials();
  const jwt = await createAppJWT(appId, privateKey);

  const res = await fetchWithTimeout(
    `https://api.github.com/app/installations/${installationId}`,
    {
      headers: {
        Authorization: `Bearer ${jwt}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "treq-github-app/1.0",
      },
    },
  );

  if (res.status === 404) return null;
  if (!res.ok) {
    throw new Error(
      `Failed to fetch installation ${installationId}: ${res.status} ${await res.text()}`,
    );
  }
  return res.json();
}
