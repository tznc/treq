import { describe, expect, it, vi } from "vitest";
import {
  authorizeRelayRequest,
  RelayError,
  type RelayStore,
} from "../../supabase/functions/_shared/remote/ssh-relay.ts";

const OWNER = "user-1";

function store(pro: boolean): RelayStore {
  return {
    getEndpoint: async () => ({
      id: "ep-1",
      instance_id: "inst-1",
      source: "managed",
      hostname: "localhost",
      port: 2222,
    }),
    getInstance: async () => ({
      id: "inst-1",
      status: "suspended",
      provider_resource_id: "dev-treq-user-1",
    }),
    getClientKey: async () => ({ id: "key-1", revoked_at: null }),
    hasPro: vi.fn(async () => pro),
  };
}

function relayRequest(): Request {
  return new Request(
    "https://example.test/functions/v1/remote-ssh-relay?endpoint_id=ep-1&key_id=key-1",
    { headers: { authorization: "Bearer session-jwt", upgrade: "websocket" } },
  );
}

// A paused Sprite wakes when the relay connects to it, so the relay is where
// a lapsed owner's cloud workspace stays stopped.
describe("authorizeRelayRequest and Pro", () => {
  it("refuses an owner without Pro with 402 pro_required", async () => {
    const s = store(false);
    const error = await authorizeRelayRequest(relayRequest(), {
      store: s,
      getUserId: async () => OWNER,
    }).catch((err: unknown) => err);

    expect(error).toBeInstanceOf(RelayError);
    expect((error as RelayError).status).toBe(402);
    expect((error as RelayError).code).toBe("pro_required");
    expect(s.hasPro).toHaveBeenCalledWith(OWNER);
  });

  it("lets a Pro owner through", async () => {
    const target = await authorizeRelayRequest(relayRequest(), {
      store: store(true),
      getUserId: async () => OWNER,
    });
    expect(target).toMatchObject({
      ownerUserId: OWNER,
      instanceId: "inst-1",
      spriteName: "dev-treq-user-1",
    });
  });
});
