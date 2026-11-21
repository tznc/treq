import { bindOptions, type Store } from "./settings";

bindOptions(document, chrome.storage.sync as Store);
