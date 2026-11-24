import { beforeEach, describe, expect, it } from "vitest";
import { bindOptions, loadSettings, providers, type Store } from "./settings";

/** `chrome.storage.sync`'s get-with-defaults and set, in memory. */
function memoryStore(
  saved: Record<string, boolean> = {},
): Store & { saved: Record<string, boolean> } {
  return {
    saved,
    get: async (defaults) => ({ ...defaults, ...saved }),
    set: async (items) => void Object.assign(saved, items),
  };
}

describe("providers", () => {
  it("always shows Treq, after the agents turned on", () => {
    expect(providers({ claude: false, codex: false })).toEqual(["treq"]);
    expect(providers({ claude: true, codex: true })).toEqual([
      "claude",
      "codex",
      "treq",
    ]);
    expect(providers({ claude: false, codex: true })).toEqual([
      "codex",
      "treq",
    ]);
  });
});

describe("loadSettings", () => {
  it("hides Claude and Codex until turned on", async () => {
    expect(await loadSettings(memoryStore())).toEqual({
      claude: false,
      codex: false,
    });
    expect(await loadSettings(memoryStore({ codex: true }))).toEqual({
      claude: false,
      codex: true,
    });
  });
});

describe("bindOptions", () => {
  beforeEach(() => {
    document.body.innerHTML = `<input type="checkbox" name="claude"><input type="checkbox" name="codex">`;
  });
  const box = (name: string) =>
    document.querySelector<HTMLInputElement>(`[name=${name}]`)!;

  it("shows the saved settings and saves each change", async () => {
    const store = memoryStore({ codex: true });
    await bindOptions(document, store);
    expect([box("claude").checked, box("codex").checked]).toEqual([
      false,
      true,
    ]);
    box("claude").checked = true;
    box("claude").dispatchEvent(new Event("change"));
    await Promise.resolve();
    expect(store.saved).toEqual({ codex: true, claude: true });
  });
});
