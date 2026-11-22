/**
 * Edge Runtime main router for the treq fat image.
 *
 * No remote imports — the fat image must boot offline after build.
 * `github-webhook` skips JWT verification (HMAC is checked inside the
 * function), matching supabase/config.toml.
 */

console.log("treq edge main function started");

const JWT_SECRET = Deno.env.get("JWT_SECRET");
const VERIFY_JWT = Deno.env.get("VERIFY_JWT") === "true";
const SKIP_JWT = new Set(["github-webhook"]);

function getAuthToken(req: Request): string {
  const authHeader = req.headers.get("authorization");
  if (!authHeader) {
    throw new Error("Missing authorization header");
  }
  const [bearer, token] = authHeader.split(" ");
  if (bearer !== "Bearer") {
    throw new Error(`Auth header is not 'Bearer {token}'`);
  }
  return token;
}

function b64urlToBytes(input: string): Uint8Array {
  const padded = input.replace(/-/g, "+").replace(/_/g, "/") +
    "=".repeat((4 - (input.length % 4)) % 4);
  const binary = atob(padded);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

function decodeJwtPart(part: string): Record<string, unknown> {
  return JSON.parse(new TextDecoder().decode(b64urlToBytes(part)));
}

async function isValidHs256Jwt(jwt: string): Promise<boolean> {
  if (!JWT_SECRET) {
    console.error("JWT_SECRET not available for HS256 token verification");
    return false;
  }
  const parts = jwt.split(".");
  if (parts.length !== 3) return false;

  try {
    const header = decodeJwtPart(parts[0]);
    if (header.alg !== "HS256") {
      console.error(`Unsupported JWT alg: ${header.alg}`);
      return false;
    }
    const key = await crypto.subtle.importKey(
      "raw",
      new TextEncoder().encode(JWT_SECRET),
      { name: "HMAC", hash: "SHA-256" },
      false,
      ["verify"],
    );
    const data = new TextEncoder().encode(`${parts[0]}.${parts[1]}`);
    const signature = b64urlToBytes(parts[2]);
    const ok = await crypto.subtle.verify("HMAC", key, signature, data);
    if (!ok) return false;

    const payload = decodeJwtPart(parts[1]);
    if (typeof payload.exp === "number" && payload.exp * 1000 < Date.now()) {
      console.error("JWT expired");
      return false;
    }
    return true;
  } catch (e) {
    console.error("JWT verification error", e);
    return false;
  }
}

Deno.serve(async (req: Request) => {
  const url = new URL(req.url);
  const pathParts = url.pathname.split("/");
  const serviceName = pathParts[1];

  if (!serviceName) {
    return new Response(JSON.stringify({ msg: "missing function name in request" }), {
      status: 400,
      headers: { "Content-Type": "application/json" },
    });
  }

  const skipJwt = SKIP_JWT.has(serviceName);

  if (req.method !== "OPTIONS" && VERIFY_JWT && !skipJwt) {
    try {
      const token = getAuthToken(req);
      const ok = await isValidHs256Jwt(token);
      if (!ok) {
        return new Response(JSON.stringify({ msg: "Invalid JWT" }), {
          status: 401,
          headers: { "Content-Type": "application/json" },
        });
      }
    } catch (e) {
      console.error(e);
      return new Response(JSON.stringify({ msg: e instanceof Error ? e.message : "JWT check failed" }), {
        status: 401,
        headers: { "Content-Type": "application/json" },
      });
    }
  }

  const servicePath = `/home/deno/functions/${serviceName}`;
  console.error(`serving the request with ${servicePath}`);

  const envVarsObj = Deno.env.toObject();
  const envVars = Object.keys(envVarsObj).map((k) => [k, envVarsObj[k]]);

  try {
    const worker = await EdgeRuntime.userWorkers.create({
      servicePath,
      memoryLimitMb: 150,
      workerTimeoutMs: 60_000,
      noModuleCache: false,
      importMapPath: "/home/deno/functions/deno.json",
      envVars,
    });
    return await worker.fetch(req);
  } catch (e) {
    return new Response(JSON.stringify({ msg: e instanceof Error ? e.message : "worker failed" }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
});
