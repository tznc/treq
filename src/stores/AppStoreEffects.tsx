import { useEffect } from "react";
import useSWR from "swr";
import { listen } from "@tauri-apps/api/event";
import { detectEditorApps, getSettingsBatch } from "../lib/api";
import { PREVIEW_FEATURE_IDS, previewSettingKey } from "../lib/features";
import { TREQ_SEND_EVENT, type TreqSendPayload } from "../lib/treqSend";
import { startSubscriptionRefreshOnFocus, useAuthStore } from "./authStore";
import { useEditorAppsStore } from "./editorAppsStore";
import { applySettingsRecord } from "./settingsHydration";
import { useThemeStore } from "./themeStore";
import { useTreqSendStore } from "./treqSendStore";
import { applyZoomToDocument, useZoomSettingsStore } from "./zoomSettingsStore";

const SETTINGS_BATCH_KEYS = [
  "theme",
  "terminal_font_size",
  "diff_font_size",
  "ui_zoom",
  "workspace_sidebar_width",
  ...PREVIEW_FEATURE_IDS.map(previewSettingKey),
] as const;

/**
 * One-shot side effects for Zustand stores: persist hydration, DOM/theme,
 * Tauri listeners, and editor detection. Mount once at the app root.
 */
export function AppStoreEffects() {
  const theme = useThemeStore((s) => s.theme);
  const systemTheme = useThemeStore((s) => s.systemTheme);
  const zoom = useZoomSettingsStore((s) => s.zoom);
  const actualTheme = theme === "system" ? systemTheme : theme;

  useSWR(
    "settings-batch-hydrate",
    () => getSettingsBatch([...SETTINGS_BATCH_KEYS]),
    {
      onSuccess: (record) => {
        if (record) applySettingsRecord(record);
      },
      onError: (error) => {
        console.error("Failed to preload settings:", error);
      },
    },
  );

  useSWR("detect-editor-apps", detectEditorApps, {
    onSuccess: (apps) => {
      if (!apps) {
        useEditorAppsStore.getState().setLoading(false);
        return;
      }
      useEditorAppsStore.getState().setEditorApps(apps);
    },
    onError: (error) => {
      console.error("Failed to detect editor apps:", error);
      useEditorAppsStore.getState().setLoading(false);
    },
  });

  useSWR("auth-restore-session", () =>
    useAuthStore.getState().restoreSession(),
  );

  useEffect(() => startSubscriptionRefreshOnFocus(), []);

  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    useThemeStore
      .getState()
      .setSystemTheme(mediaQuery.matches ? "dark" : "light");

    const onSchemeChange = (event: MediaQueryListEvent) => {
      useThemeStore.getState().setSystemTheme(event.matches ? "dark" : "light");
    };
    mediaQuery.addEventListener("change", onSchemeChange);

    const onZoomKeyDown = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
      const isZoomIn =
        event.key === "=" || event.key === "+" || event.code === "Equal";
      const isZoomOut =
        event.key === "-" || event.key === "_" || event.code === "Minus";
      if (!isZoomIn && !isZoomOut) return;
      event.preventDefault();
      if (isZoomIn) {
        void useZoomSettingsStore.getState().zoomIn();
      } else {
        void useZoomSettingsStore.getState().zoomOut();
      }
    };
    window.addEventListener("keydown", onZoomKeyDown, true);

    let unlistenDeepLink: (() => void) | undefined;
    let unlistenSend: (() => void) | undefined;

    listen<string[]>("deep-link-received", async (event) => {
      try {
        const url = event.payload.find((u) => u.includes("auth/callback"));
        if (!url) return;
        const token = new URL(url).searchParams.get("token");
        if (!token) return;
        await useAuthStore.getState().exchangeToken(token);
      } catch {
        // Deep link handling failed
      }
    })
      .then((fn) => {
        unlistenDeepLink = fn;
      })
      .catch(() => {});

    listen<TreqSendPayload>(TREQ_SEND_EVENT, (event) => {
      useTreqSendStore.getState().ingestPayload(event.payload);
    })
      .then((fn) => {
        unlistenSend = fn;
      })
      .catch((error) => {
        console.error("Failed to listen for treq send events:", error);
      });

    return () => {
      mediaQuery.removeEventListener("change", onSchemeChange);
      window.removeEventListener("keydown", onZoomKeyDown, true);
      unlistenDeepLink?.();
      unlistenSend?.();
    };
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    if (actualTheme === "dark") {
      root.classList.add("dark");
    } else {
      root.classList.remove("dark");
    }
  }, [actualTheme]);

  useEffect(() => {
    applyZoomToDocument(zoom);
  }, [zoom]);

  return null;
}
