import { describe, expect, it } from "vitest";
import { isProRequiredError, proRequiredMessage } from "./pro-required";
import {
  RemoteFunctionError,
  remoteFunctionError,
} from "./remote-control-plane";

function functionsHttpError(status: number, body: unknown) {
  // supabase.functions.invoke's FunctionsHttpError keeps the raw Response.
  return Object.assign(
    new Error("Edge Function returned a non-2xx status code"),
    {
      name: "FunctionsHttpError",
      context: new Response(JSON.stringify(body), { status }),
    },
  );
}

describe("isProRequiredError", () => {
  it("recognizes set_merge_queue_enabled's PostgREST refusal", () => {
    expect(
      isProRequiredError({
        code: "PT402",
        message: "The merge queue needs Pro",
        details: null,
        hint: "pro_required",
      }),
    ).toBe(true);
  });

  it("recognizes an Edge Function's 402 answer", () => {
    expect(
      isProRequiredError(
        functionsHttpError(402, {
          error: "Linear OAuth needs Pro.",
          code: "pro_required",
        }),
      ),
    ).toBe(true);
  });

  it("recognizes remote-instance's refusal, as an error or as the stored message", async () => {
    const error = await remoteFunctionError(
      functionsHttpError(402, {
        error: "Cloud workspaces need Pro.",
        code: "pro_required",
        correlation_id: "c-1",
      }),
    );
    expect(error).toBeInstanceOf(RemoteFunctionError);
    expect(isProRequiredError(error)).toBe(true);
    expect(isProRequiredError(error.message)).toBe(true);
  });

  it("does not treat other failures as an upsell", async () => {
    expect(isProRequiredError(null)).toBe(false);
    expect(isProRequiredError(new Error("network down"))).toBe(false);
    expect(isProRequiredError({ code: "P0002", message: "not linked" })).toBe(
      false,
    );
    expect(
      isProRequiredError(functionsHttpError(403, { error: "Forbidden" })),
    ).toBe(false);
    expect(
      isProRequiredError(
        await remoteFunctionError(
          functionsHttpError(409, { error: "busy", code: "already_exists" }),
        ),
      ),
    ).toBe(false);
    expect(isProRequiredError("[internal_error] Internal error")).toBe(false);
  });
});

describe("proRequiredMessage", () => {
  it("strips the code prefix and HTTP detail from a stored remote error", () => {
    expect(
      proRequiredMessage(
        "[pro_required] Cloud workspaces need Pro.\nHTTP 402 · Correlation ID: c-1",
      ),
    ).toBe("Cloud workspaces need Pro.");
  });
});
