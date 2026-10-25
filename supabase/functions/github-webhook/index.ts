// GitHub webhook ingress.
//
// Responsibilities (and nothing more):
//   1. Verify the HMAC signature before parsing anything.
//   2. Deduplicate deliveries via github_webhook_receipts.
//   3. Upsert directly observable GitHub facts (installations/repos).
//   4. Return 202.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import {
  applyInstallationFacts,
  type GitHubWebhookPayload,
} from "./webhook-handlers.ts";

async function verifySignature(
  secret: string,
  body: string,
  sigHeader: string,
): Promise<boolean> {
  if (!sigHeader.startsWith("sha256=")) return false;

  const hex = sigHeader.slice(7);
  if (hex.length % 2 !== 0) return false;

  const sigBytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    sigBytes[i / 2] = parseInt(hex.slice(i, i + 2), 16);
  }

  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"],
  );

  return crypto.subtle.verify(
    "HMAC",
    key,
    sigBytes,
    new TextEncoder().encode(body),
  );
}

async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(input),
  );
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response(null, { status: 204 });
  }
  if (req.method !== "POST") {
    return new Response("Method not allowed", { status: 405 });
  }

  // Unsigned webhooks are never accepted: a missing secret is a
  // configuration failure, not a reason to skip verification.
  const webhookSecret = Deno.env.get("GITHUB_WEBHOOK_SECRET");
  if (!webhookSecret) {
    console.error("[github-webhook] GITHUB_WEBHOOK_SECRET is not configured");
    return new Response("Webhook secret not configured", { status: 500 });
  }

  const body = await req.text();
  const sigHeader = req.headers.get("x-hub-signature-256") ?? "";
  if (!(await verifySignature(webhookSecret, body, sigHeader))) {
    return new Response("Unauthorized", { status: 401 });
  }

  const deliveryId = req.headers.get("x-github-delivery");
  if (!deliveryId) {
    return new Response("Missing X-GitHub-Delivery", { status: 400 });
  }
  const event = req.headers.get("x-github-event") ?? "";

  let payload: GitHubWebhookPayload;
  try {
    payload = JSON.parse(body);
  } catch {
    return new Response("Invalid JSON", { status: 400 });
  }

  const supabase = createClient(
    Deno.env.get("SUPABASE_URL") ?? "",
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  const payloadHash = await sha256Hex(body);

  // Durable ingestion + delivery dedupe.
  const { error: insertError } = await supabase
    .from("github_webhook_receipts")
    .insert({
      delivery_id: deliveryId,
      event_name: event,
      action: (payload.action as string) ?? null,
      installation_id: payload.installation?.id ?? null,
      repository_id: payload.repository?.id ?? null,
      payload_hash: payloadHash,
    });

  if (insertError) {
    if (insertError.code !== "23505") {
      console.error("[github-webhook] receipt insert failed:", insertError.message);
      return new Response("Ingestion failed", { status: 500 });
    }
    const { data: existing } = await supabase
      .from("github_webhook_receipts")
      .select("payload_hash, accepted_at")
      .eq("delivery_id", deliveryId)
      .maybeSingle();
    if (existing && existing.payload_hash !== payloadHash) {
      // Same delivery ID with different content: someone is replaying the
      // endpoint with forged bodies.
      console.error(
        JSON.stringify({
          level: "security",
          message: "delivery id reused with different payload hash",
          github_delivery_id: deliveryId,
          event,
        }),
      );
      return new Response("Conflict", { status: 409 });
    }
    if (existing?.accepted_at) {
      // Exact redelivery of an already-ingested event.
      return new Response(JSON.stringify({ ok: true, duplicate: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    // The receipt exists but a previous attempt failed before acceptance —
    // fall through and reprocess this delivery.
  }

  try {
    await applyInstallationFacts(supabase, event, payload);
    await supabase
      .from("github_webhook_receipts")
      .update({ accepted_at: new Date().toISOString() })
      .eq("delivery_id", deliveryId);

    console.log(
      JSON.stringify({
        operation: "webhook_ingested",
        github_delivery_id: deliveryId,
        event,
        action: payload.action ?? null,
      }),
    );

    return new Response(JSON.stringify({ ok: true }), {
      status: 202,
      headers: { "Content-Type": "application/json" },
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error(`[github-webhook] ingestion error for "${event}":`, message);
    await supabase
      .from("github_webhook_receipts")
      .update({ processing_error: message })
      .eq("delivery_id", deliveryId);
    // 500 so GitHub retries the delivery; the receipt row's missing
    // accepted_at marks it as never fully ingested.
    return new Response("Ingestion failed", { status: 500 });
  }
});
