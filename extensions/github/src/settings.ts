import type { Provider } from "./composer";

/** Which agents besides Treq the line buttons offer. */
export type Settings = { claude: boolean; codex: boolean };

/** `chrome.storage.sync`'s get-with-defaults and set. */
export type Store = {
  get(defaults: Settings): Promise<Settings>;
  set(items: Partial<Settings>): Promise<void>;
};

const DEFAULTS: Settings = { claude: false, codex: false };

/** The line buttons to show: Treq always, after the agents turned on. */
export const providers = (s: Settings): Provider[] => [
  ...(s.claude ? (["claude"] as const) : []),
  ...(s.codex ? (["codex"] as const) : []),
  "treq",
];

export const loadSettings = (store: Store): Promise<Settings> =>
  store.get(DEFAULTS);

/** Checks the options page's boxes as saved, and saves each change. */
export async function bindOptions(doc: Document, store: Store): Promise<void> {
  const settings = await loadSettings(store);
  for (const name of Object.keys(DEFAULTS) as (keyof Settings)[]) {
    const box = doc.querySelector<HTMLInputElement>(`input[name="${name}"]`);
    if (!box) continue;
    box.checked = settings[name];
    box.addEventListener("change", () => store.set({ [name]: box.checked }));
  }
}
