import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2, Zap } from "lucide-react";
import { useState } from "react";
import useSWR from "swr";
import { useAuthStore } from "../stores/authStore";
import { isProSubscription } from "../lib/subscription";
import { FEATURES } from "../lib/features";
import { isProRequiredError } from "../lib/pro-required";
import { supabase, WEB_URL } from "../lib/supabase";
import {
  getLinearApiKey,
  setLinearApiKey,
  getLinearAutoKickoffLabel,
  setLinearAutoKickoffLabel,
  linearStartAutoKickoffPolling,
} from "../lib/api-linear";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { useToastStore } from "../stores/toastStore";

interface LinearIntegrationSettingsProps {
  repoPath?: string;
}

const SettingRow: React.FC<{
  title: string;
  description: string;
  children?: React.ReactNode;
}> = ({ title, description, children }) => (
  <div className="flex items-start justify-between gap-4 py-3">
    <div className="min-w-0">
      <p className="font-medium">{title}</p>
      <p className="text-base text-muted-foreground">{description}</p>
    </div>
    <div className="flex items-center gap-2 shrink-0">{children}</div>
  </div>
);

export const LinearIntegrationSettings: React.FC<
  LinearIntegrationSettingsProps
> = ({ repoPath }) => {
  const { subscription } = useAuthStore();
  const isPro = isProSubscription(subscription);
  const { addToast } = useToastStore();

  const [apiKey, setApiKey] = useState<string | null>(null);
  const [apiKeyEditing, setApiKeyEditing] = useState(false);
  const [apiKeySaving, setApiKeySaving] = useState(false);

  const [autoKickoffLabel, setAutoKickoffLabel] = useState<string | null>(null);
  const [autoKickoffLabelEditing, setAutoKickoffLabelEditing] = useState(false);
  const [autoKickoffLabelSaving, setAutoKickoffLabelSaving] = useState(false);

  const [authLoading, setAuthLoading] = useState(false);

  useSWR(
    repoPath ? ["linear-api-key", repoPath] : null,
    async () => {
      const key = await getLinearApiKey(repoPath!);
      setApiKey(key);
      setApiKeyEditing(false);
    },
    // dedupingInterval: 0 -- otherwise a quick unmount/remount (e.g. tab
    // away and back within Settings) skips the fetcher and the field is
    // left showing its unset default instead of the persisted value.
    { revalidateOnFocus: false, dedupingInterval: 0 },
  );

  useSWR(
    repoPath ? ["linear-auto-kickoff-label", repoPath] : null,
    async () => {
      const label = await getLinearAutoKickoffLabel(repoPath!);
      setAutoKickoffLabel(label);
      setAutoKickoffLabelEditing(false);
    },
    { revalidateOnFocus: false, dedupingInterval: 0 },
  );

  const handleSaveApiKey = async () => {
    if (!repoPath || !apiKey) return;
    try {
      setApiKeySaving(true);
      await setLinearApiKey(repoPath, apiKey);
      setApiKeyEditing(false);
      addToast({
        title: "API Key saved",
        type: "success",
      });
    } catch (err) {
      addToast({
        title: "Error saving API key",
        description: err instanceof Error ? err.message : String(err),
        type: "error",
      });
    } finally {
      setApiKeySaving(false);
    }
  };

  const handleSaveAutoKickoffLabel = async () => {
    if (!repoPath || !autoKickoffLabel) return;
    try {
      setAutoKickoffLabelSaving(true);
      await setLinearAutoKickoffLabel(repoPath, autoKickoffLabel);
      setAutoKickoffLabelEditing(false);
      void linearStartAutoKickoffPolling(repoPath);
      addToast({
        title: "Auto-kickoff label saved",
        type: "success",
      });
    } catch (err) {
      addToast({
        title: "Error saving auto-kickoff label",
        description: err instanceof Error ? err.message : String(err),
        type: "error",
      });
    } finally {
      setAutoKickoffLabelSaving(false);
    }
  };

  const handleConnectViaOAuth = async () => {
    try {
      setAuthLoading(true);
      const { data, error } = await supabase.functions.invoke(
        "create-linear-oauth-intent",
        { body: {} },
      );
      if (error && isProRequiredError(error)) {
        // The server decides entitlement; the Pro gate above is display only.
        addToast({
          title: "Linear OAuth needs Pro",
          description:
            "Add a personal Linear API key above to use Linear on the Free plan.",
          type: "info",
          action: {
            label: "Upgrade to Pro",
            onClick: () => void openUrl(`${WEB_URL}/dashboard`),
          },
        });
        return;
      }
      if (error) {
        throw new Error(
          error instanceof Error ? error.message : JSON.stringify(error),
        );
      }
      if (data?.authorization_url) {
        await openUrl(data.authorization_url);
      } else {
        throw new Error("No authorization URL returned");
      }
    } catch (err) {
      addToast({
        title: "Error starting OAuth flow",
        description: err instanceof Error ? err.message : String(err),
        type: "error",
      });
    } finally {
      setAuthLoading(false);
    }
  };

  if (!repoPath) {
    return (
      <div className="text-center py-8 text-muted-foreground">
        Select a repository to configure Linear integration
      </div>
    );
  }

  return (
    <div className="space-y-10">
      <section>
        <div className="flex items-center gap-3 pb-2 border-b border-border">
          <Zap className="w-5 h-5" />
          <h3 className="font-semibold">Linear</h3>
          <span className="text-base text-muted-foreground">
            Issue tracker integration
          </span>
        </div>

        <div className="divide-y divide-border">
          <SettingRow
            title="API Key"
            description={
              apiKey
                ? "Direct API access configured"
                : "Free plan: provide your API key"
            }
          >
            {apiKeyEditing ? (
              <div className="flex gap-2 shrink-0">
                <Input
                  type="password"
                  placeholder="linear_api_..."
                  value={apiKey || ""}
                  onChange={(e) => setApiKey(e.target.value)}
                  className="w-64"
                  disabled={apiKeySaving}
                />
                <Button
                  size="sm"
                  onClick={handleSaveApiKey}
                  disabled={apiKeySaving || !apiKey}
                >
                  {apiKeySaving ? (
                    <Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />
                  ) : null}
                  Save
                </Button>
              </div>
            ) : (
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setApiKeyEditing(true);
                  setApiKey(apiKey || "");
                }}
              >
                {apiKey ? "Update" : "Add"} API Key
              </Button>
            )}
          </SettingRow>

          {isPro && FEATURES.pro && (
            <SettingRow
              title="OAuth Connect"
              description={
                apiKey
                  ? "API key takes priority, but you can also connect via OAuth"
                  : "Pro plan: connect via OAuth"
              }
            >
              <Button
                size="sm"
                onClick={handleConnectViaOAuth}
                disabled={authLoading}
              >
                {authLoading ? (
                  <Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />
                ) : (
                  <ExternalLink className="w-3 h-3 mr-1" />
                )}
                Connect via OAuth
              </Button>
            </SettingRow>
          )}

          <SettingRow
            title="Auto-kickoff Label"
            description={
              autoKickoffLabel
                ? `Issues with label "${autoKickoffLabel}" auto-open a workspace`
                : "Label name that triggers automatic workspace creation"
            }
          >
            {autoKickoffLabelEditing ? (
              <div className="flex gap-2 shrink-0">
                <Input
                  placeholder="e.g. ready-to-work"
                  value={autoKickoffLabel || ""}
                  onChange={(e) => setAutoKickoffLabel(e.target.value)}
                  className="w-64"
                  disabled={autoKickoffLabelSaving}
                />
                <Button
                  size="sm"
                  onClick={handleSaveAutoKickoffLabel}
                  disabled={autoKickoffLabelSaving || !autoKickoffLabel}
                >
                  {autoKickoffLabelSaving ? (
                    <Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />
                  ) : null}
                  Save
                </Button>
              </div>
            ) : (
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setAutoKickoffLabelEditing(true);
                  setAutoKickoffLabel(autoKickoffLabel || "");
                }}
              >
                {autoKickoffLabel ? "Update" : "Add"} Label
              </Button>
            )}
          </SettingRow>

          {apiKey && isPro && (
            <div className="py-3">
              <p className="text-sm text-muted-foreground">
                Direct API key configured. This takes priority over OAuth
                authentication.
              </p>
            </div>
          )}
        </div>
      </section>
    </div>
  );
};
