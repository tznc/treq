import {
  CreditCard,
  Crown,
  ExternalLink,
  Loader2,
  LogOut,
  User,
} from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { WEB_URL } from "../lib/supabase";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAuthStore } from "../stores/authStore";
import {
  isProSubscription,
  subscriptionPeriodLabel,
  subscriptionStatusLabel,
} from "../lib/subscription";
import { useEffect, useState } from "react";
import {
  CloudWorkspaceCard,
  type CloudWorkspaceCardProps,
} from "./remote/CloudWorkspaceCard";

const isDev = import.meta.env.DEV;

/** State and actions for the account's Treq-managed cloud workspace. */
export interface CloudWorkspaceControls extends CloudWorkspaceCardProps {
  /** Reloads status from the control plane; called when the card mounts. */
  onRefreshStatus: () => Promise<void>;
}

interface AccountSettingsProps {
  cloudWorkspace?: CloudWorkspaceControls;
}

export const AccountSettings: React.FC<AccountSettingsProps> = ({
  cloudWorkspace,
}) => {
  const { user, loading, subscription, signIn, signOut, exchangeToken } =
    useAuthStore();
  const [callbackUrl, setCallbackUrl] = useState("");
  const [devError, setDevError] = useState<string | null>(null);
  const [devLoading, setDevLoading] = useState(false);
  const refreshCloudWorkspace = cloudWorkspace?.onRefreshStatus;
  const signedIn = Boolean(user);

  useEffect(() => {
    if (signedIn) void refreshCloudWorkspace?.();
    // Load once per sign-in; the callback identity changes every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signedIn]);

  const handleDevCallback = async () => {
    setDevError(null);
    setDevLoading(true);
    try {
      // Accept either a full treq://auth/callback?token=XXX URL or just the token
      let token = callbackUrl.trim();
      if (token.includes("token=")) {
        const url = new URL(token);
        token = url.searchParams.get("token") || "";
      }
      if (!token) {
        setDevError("No token found in URL");
        return;
      }
      await exchangeToken(token);
      setCallbackUrl("");
    } catch (err: unknown) {
      setDevError(err instanceof Error ? err.message : "Token exchange failed");
    } finally {
      setDevLoading(false);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-16">
        <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (!user) {
    return (
      <div className="flex flex-col items-center justify-center py-16 gap-4">
        <div className="w-16 h-16 rounded-full bg-muted flex items-center justify-center">
          <User className="w-8 h-8 text-muted-foreground" />
        </div>
        <div className="text-center">
          <h3 className="text-lg font-semibold">Sign in to Treq</h3>
          <p className="text-sm text-muted-foreground mt-1">
            Sync your settings and manage your subscription
          </p>
        </div>
        <Button onClick={signIn} className="gap-2">
          <ExternalLink className="w-4 h-4" />
          Sign in with Browser
        </Button>

        {isDev && (
          <div className="w-full max-w-sm mt-4 p-3 border border-dashed border-yellow-500/50 rounded-lg bg-yellow-500/5">
            <div className="text-xs font-medium text-yellow-600 dark:text-yellow-400 mb-2">
              Dev: Paste callback URL
            </div>
            <div className="flex gap-2">
              <Input
                value={callbackUrl}
                onChange={(event) => setCallbackUrl(event.target.value)}
                placeholder="treq://auth/callback?token=..."
                className="text-xs font-mono"
              />
              <Button
                size="sm"
                variant="outline"
                onClick={handleDevCallback}
                disabled={devLoading || !callbackUrl.trim()}
              >
                {devLoading ? "..." : "Go"}
              </Button>
            </div>
            {devError && (
              <p className="text-xs text-red-500 mt-1">{devError}</p>
            )}
          </div>
        )}
      </div>
    );
  }

  const avatarUrl =
    user.user_metadata?.avatar_url || user.user_metadata?.picture;
  const fullName =
    user.user_metadata?.full_name || user.user_metadata?.name || "User";
  const { email } = user;

  const isPro = isProSubscription(subscription);

  return (
    <div className="space-y-6">
      {/* User Info Card */}
      <div className="border border-border rounded-lg p-4">
        <div className="flex items-center gap-4">
          {avatarUrl ? (
            <img
              src={avatarUrl}
              alt={fullName}
              className="w-12 h-12 rounded-full"
            />
          ) : (
            <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center">
              <User className="w-6 h-6 text-muted-foreground" />
            </div>
          )}
          <div className="flex-1 min-w-0">
            <div className="font-medium truncate">{fullName}</div>
            <div className="text-sm text-muted-foreground truncate">
              {email}
            </div>
          </div>
        </div>
      </div>

      {/* Subscription Card */}
      <div className="border border-border rounded-lg p-4">
        <div className="flex items-center gap-2 mb-3">
          <CreditCard className="w-4 h-4" />
          <span className="font-medium">Subscription</span>
        </div>
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm text-muted-foreground">Plan</span>
            <span className="text-sm font-medium flex items-center gap-1">
              {isPro && <Crown className="w-3 h-3 text-yellow-500" />}
              {isPro ? "Pro" : "Free"}
            </span>
          </div>
          {subscription?.status && (
            <div className="flex items-center justify-between">
              <span className="text-sm text-muted-foreground">Status</span>
              <span
                className={`text-sm font-medium ${
                  subscription.status === "active" ||
                  subscription.status === "trialing"
                    ? "text-green-600 dark:text-green-400"
                    : subscription.status === "canceled" ||
                        subscription.status === "past_due"
                      ? "text-yellow-600 dark:text-yellow-400"
                      : "text-muted-foreground"
                }`}
              >
                {subscriptionStatusLabel(subscription.status)}
              </span>
            </div>
          )}
          {subscription?.current_period_end && (
            <div className="flex items-center justify-between">
              <span className="text-sm text-muted-foreground">
                {subscriptionPeriodLabel(subscription.status)}
              </span>
              <span className="text-sm">
                {new Date(subscription.current_period_end).toLocaleDateString()}
              </span>
            </div>
          )}
        </div>
        <div className="mt-4">
          <Button
            variant="outline"
            size="sm"
            className="w-full gap-2"
            onClick={() => openUrl(`${WEB_URL}/dashboard`)}
          >
            <ExternalLink className="w-3 h-3" />
            {isPro ? "Manage Subscription" : "Upgrade to Pro"}
          </Button>
        </div>
      </div>

      {cloudWorkspace && <CloudWorkspaceCard {...cloudWorkspace} />}

      {/* Sign Out */}
      <Button
        variant="outline"
        size="sm"
        className="gap-2 text-destructive hover:text-destructive"
        onClick={signOut}
      >
        <LogOut className="w-4 h-4" />
        Sign Out
      </Button>
    </div>
  );
};
