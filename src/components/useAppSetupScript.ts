import { useState } from "react";
import useSWR from "swr";
import {
  getAppSetupScriptStatus,
  runAppSetupScript,
  saveAppSetupScript,
} from "../lib/api";

/** Draft state, status polling, save, and run for the app setup script. */
export function useAppSetupScript(onError: (error: unknown) => void) {
  const [scriptDraft, setScript] = useState<string | null>(null);
  const [alwaysRunDraft, setAlwaysRun] = useState<boolean | null>(null);
  const [running, setRunning] = useState(false);
  const {
    data: status,
    error,
    mutate,
  } = useSWR(
    "app-setup-script-status",
    getAppSetupScriptStatus,
    // Poll while the script runs in the background.
    {
      refreshInterval: running ? 1000 : 0,
      onSuccess: (latest) => setRunning(latest.running),
    },
  );
  const script = scriptDraft ?? status?.script ?? "";
  const alwaysRun = alwaysRunDraft ?? status?.always_run ?? false;

  const save = async () => {
    // Unedited, `script` may be the "" fallback of an unloaded status.
    if (scriptDraft === null && alwaysRunDraft === null) return;
    await saveAppSetupScript(script, alwaysRun);
    await mutate();
  };

  const run = async () => {
    try {
      await runAppSetupScript();
      await mutate();
    } catch (e) {
      onError(e);
    }
  };

  return {
    script,
    alwaysRun,
    status,
    loadError: error ? String(error) : undefined,
    setScript,
    setAlwaysRun,
    save,
    run,
  };
}
