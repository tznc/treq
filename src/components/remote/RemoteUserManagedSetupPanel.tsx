import { useEffect, useState } from "react";
import { listLocalSshIdentities } from "../../lib/api";
import {
  probeSshHostKey,
  resolveSshConfigAlias,
} from "../../lib/api-remote-ssh";
import type { LocalSshIdentity } from "../../lib/api-types-remote";
import { pickDefaultKeyReference } from "../../lib/managed-ssh-connection";
import { Button } from "../ui/button";
import type { UserManagedFormValues } from "./remoteSetupLabels";

export interface RemoteUserManagedSetupPanelProps {
  sshConfigAliasSuggestions: string[];
  onBack: () => void;
  onRegisterUserManaged: (values: UserManagedFormValues) => Promise<void>;
}

/**
 * "Your own VM" registration form plus the host-trust confirmation step
 * (PRD "Host-key verification": "show a clear trust-change confirmation for
 * user-managed endpoints"). Nothing is registered until the user explicitly
 * confirms trust. The user never types a fingerprint: Continue reads the
 * key the host presents and the confirmation shows it for the user to accept.
 */
export function RemoteUserManagedSetupPanel({
  sshConfigAliasSuggestions,
  onBack,
  onRegisterUserManaged,
}: RemoteUserManagedSetupPanelProps) {
  const [displayName, setDisplayName] = useState("");
  const [hostname, setHostname] = useState("");
  const [port, setPort] = useState("22");
  const [username, setUsername] = useState("");
  const [hostKeyFingerprint, setHostKeyFingerprint] = useState("");
  const [identities, setIdentities] = useState<LocalSshIdentity[]>([]);
  const [authIdentityReference, setAuthIdentityReference] = useState("");
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState("");
  const [aliasMode, setAliasMode] = useState(false);
  const [alias, setAlias] = useState("");
  const [showTrustConfirm, setShowTrustConfirm] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [resolvingAlias, setResolvingAlias] = useState(false);
  const [resolveAliasError, setResolveAliasError] = useState("");

  useEffect(() => {
    let cancelled = false;
    listLocalSshIdentities()
      .then((found) => {
        if (cancelled) return;
        setIdentities(found);
        setAuthIdentityReference(
          (current) => current || pickDefaultKeyReference(found) || "",
        );
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const probeHostKey = async () => {
    setProbing(true);
    setProbeError("");
    try {
      setHostKeyFingerprint(
        await probeSshHostKey(hostname.trim(), Number(port) || 22),
      );
      setShowTrustConfirm(true);
    } catch (error) {
      setProbeError(error instanceof Error ? error.message : String(error));
    } finally {
      setProbing(false);
    }
  };

  const resolveAlias = async () => {
    const trimmedAlias = alias.trim();
    if (!trimmedAlias) return;
    setResolvingAlias(true);
    setResolveAliasError("");
    try {
      const resolved = await resolveSshConfigAlias(trimmedAlias);
      setHostname(resolved.hostname);
      setPort(String(resolved.port));
      if (resolved.username) setUsername(resolved.username);
    } catch (error) {
      setResolveAliasError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setResolvingAlias(false);
    }
  };

  const submitUserManaged = async () => {
    setShowTrustConfirm(false);
    setSubmitting(true);
    try {
      await onRegisterUserManaged({
        display_name: displayName.trim() || hostname.trim(),
        hostname: hostname.trim(),
        port: Number(port) || 22,
        username: username.trim(),
        host_key_fingerprint: hostKeyFingerprint.trim(),
        auth_identity_reference: authIdentityReference.trim(),
        alias: aliasMode ? alias.trim() || null : null,
      });
    } finally {
      setSubmitting(false);
    }
  };

  const keySelected = identities.some(
    (identity) => identity.reference === authIdentityReference,
  );
  const userManagedValid = Boolean(
    hostname.trim() && username.trim() && keySelected,
  );

  return (
    <>
      <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <label className="flex flex-col gap-1">
          <span className="text-sm font-medium">Display name</span>
          <input
            className="rounded-md border border-border/60 bg-background px-2 py-1.5"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-sm font-medium">Hostname or IP address</span>
          <input
            className="rounded-md border border-border/60 bg-background px-2 py-1.5"
            value={hostname}
            onChange={(e) => setHostname(e.target.value)}
            list="remote-setup-ssh-config-aliases"
            placeholder="e.g. 203.0.113.4"
          />
          <datalist id="remote-setup-ssh-config-aliases">
            {sshConfigAliasSuggestions.map((suggestion) => (
              <option key={suggestion} value={suggestion} />
            ))}
          </datalist>
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-sm font-medium">SSH port</span>
          <input
            type="number"
            className="rounded-md border border-border/60 bg-background px-2 py-1.5"
            value={port}
            onChange={(e) => setPort(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-sm font-medium">Username</span>
          <input
            className="rounded-md border border-border/60 bg-background px-2 py-1.5"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1 sm:col-span-2">
          <span className="text-sm font-medium">SSH key</span>
          <select
            className="rounded-md border border-border/60 bg-background px-2 py-1.5"
            value={authIdentityReference}
            onChange={(e) => setAuthIdentityReference(e.target.value)}
          >
            {!keySelected && <option value="">Select a key</option>}
            {identities.map((identity) => (
              <option key={identity.reference} value={identity.reference}>
                {identity.label} ({identity.algorithm})
              </option>
            ))}
          </select>
          {identities.length === 0 && (
            <span className="text-xs text-muted-foreground">
              No SSH keys found in ~/.ssh. Create one with ssh-keygen -t
              ed25519.
            </span>
          )}
        </label>
        <label className="flex items-center gap-2 sm:col-span-2">
          <input
            type="checkbox"
            checked={aliasMode}
            onChange={(e) => setAliasMode(e.target.checked)}
          />
          <span className="text-sm">
            Use an explicit SSH alias for this endpoint (optional)
          </span>
        </label>
        {aliasMode && (
          <label className="flex flex-col gap-1 sm:col-span-2">
            <span className="text-sm font-medium">Alias</span>
            <div className="flex gap-2">
              <input
                className="flex-1 rounded-md border border-border/60 bg-background px-2 py-1.5"
                value={alias}
                onChange={(e) => setAlias(e.target.value)}
                list="remote-setup-ssh-config-aliases"
              />
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={!alias.trim() || resolvingAlias}
                onClick={() => void resolveAlias()}
              >
                {resolvingAlias ? "Resolving..." : "Autofill from alias"}
              </Button>
            </div>
            <span className="text-xs text-muted-foreground">
              Reads hostname/port/username from ~/.ssh/config for this alias
              only. This never grants trust - you still confirm the host key the
              server presents.
            </span>
            {resolveAliasError && (
              <span className="text-xs text-destructive">
                {resolveAliasError}
              </span>
            )}
          </label>
        )}
      </div>

      {showTrustConfirm && (
        <div
          data-testid="host-trust-confirmation"
          className="mt-4 rounded-md border border-yellow-500/40 bg-yellow-500/10 p-3 text-sm"
        >
          <p className="font-medium">Confirm host trust</p>
          <p className="mt-1 text-muted-foreground">
            You are about to trust{" "}
            <span className="font-mono">
              {username}@{hostname}:{port}
            </span>{" "}
            with host key fingerprint{" "}
            <span className="font-mono">{hostKeyFingerprint}</span>. Treq will
            reject this endpoint if the presented key ever changes. Compare it
            with the output of ssh-keygen -lf on the host before trusting it.
          </p>
          <div className="mt-3 flex gap-2">
            <Button
              size="sm"
              disabled={submitting}
              onClick={() => void submitUserManaged()}
            >
              Trust and connect
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setShowTrustConfirm(false);
                setHostKeyFingerprint("");
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}

      {probeError && (
        <p role="alert" className="mt-4 text-sm text-destructive">
          {probeError}
        </p>
      )}

      <div className="mt-6 flex justify-between">
        <Button variant="ghost" onClick={onBack} disabled={submitting}>
          Back
        </Button>
        {!showTrustConfirm && (
          <Button
            disabled={!userManagedValid || submitting || probing}
            onClick={() => void probeHostKey()}
          >
            {probing ? "Checking host key..." : "Continue"}
          </Button>
        )}
      </div>
    </>
  );
}
