import type React from "react";
import { useCallback, useEffect, useState } from "react";
import {
  fetchLastFailure,
  fetchMachine,
  formatMachineTime,
  machineActions,
  MachineActionError,
  machineStatus,
  regionLabel,
  runMachineAction,
  sizeLabel,
  type Machine,
  type MachineAction,
  type StatusTone,
} from "../../lib/machines";

// The dashboard's Machines tab. It reuses the dashboard's style table, passed
// in as `styles`, so it looks like the other tabs.

type Styles = Record<string, React.CSSProperties>;

const BUSY_POLL_MS = 5000;

const NEEDS_PRO = "Cloud machines need Pro. You can still check the status of your machine or delete it.";

const PENDING_LABEL: Record<MachineAction, string> = {
  ensure: "Creating…",
  wake: "Starting…",
  delete: "Deleting…",
};

const TONE_STYLE: Record<StatusTone, React.CSSProperties> = {
  ok: { backgroundColor: "rgba(16,185,129,0.1)", color: "#10b981" },
  busy: { backgroundColor: "rgba(59,130,246,0.1)", color: "#3b82f6" },
  idle: { backgroundColor: "var(--ifm-color-emphasis-100)", color: "var(--dashboard-text-subtle)" },
  error: { backgroundColor: "rgba(239,68,68,0.1)", color: "#ef4444" },
};

const badge: React.CSSProperties = {
  padding: "0.2rem 0.6rem",
  borderRadius: "12px",
  fontSize: "0.8rem",
  fontWeight: 600,
};

const failureBox: React.CSSProperties = {
  marginTop: "1rem",
  padding: "0.6rem 0.75rem",
  borderRadius: "6px",
  backgroundColor: "rgba(239,68,68,0.08)",
  color: "#ef4444",
  fontSize: "0.85rem",
};

function Field({ label, children, styles }: { label: string; children: React.ReactNode; styles: Styles }) {
  return (
    <div role="group" aria-label={label} className="dashboard-field" style={styles.field}>
      <span style={styles.fieldLabel}>{label}</span>
      <div style={styles.fieldValue}>{children}</div>
    </div>
  );
}

export default function MachinesTab({
  isPro,
  onUpgrade,
  styles,
}: {
  isPro: boolean;
  onUpgrade: () => void;
  styles: Styles;
}) {
  // undefined while the first status loads.
  const [machine, setMachine] = useState<Machine | null | undefined>(undefined);
  const [failure, setFailure] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [needsPro, setNeedsPro] = useState<string | null>(null);
  const [pending, setPending] = useState<MachineAction | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  const load = useCallback(async () => {
    try {
      const latest = await fetchMachine();
      setMachine(latest);
      setFailure(latest ? await fetchLastFailure(latest.instance_id) : null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not load your machine.");
      setMachine((current) => current ?? null);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const status = machine ? machineStatus(machine.status) : null;
  useEffect(() => {
    if (status?.tone !== "busy" || pending) return;
    const timer = setTimeout(() => void load(), BUSY_POLL_MS);
    return () => clearTimeout(timer);
  }, [status?.tone, machine, pending, load]);

  const run = async (action: MachineAction) => {
    setPending(action);
    setError(null);
    setNeedsPro(null);
    try {
      await runMachineAction(action, machine ?? null);
      setConfirmingDelete(false);
    } catch (err) {
      if (err instanceof MachineActionError && err.needsPro) setNeedsPro(err.message);
      else setError(err instanceof Error ? err.message : "The request failed.");
    }
    await load();
    setPending(null);
  };

  if (machine === undefined) {
    return (
      <div>
        <h1 style={styles.pageTitle}>Machines</h1>
        <div style={styles.loadingText}>Loading machines…</div>
      </div>
    );
  }

  const actions = machineActions(machine, isPro);
  const upsell = needsPro ?? (isPro ? null : NEEDS_PRO);

  return (
    <div>
      <h1 style={styles.pageTitle}>Machines</h1>
      <div style={{ ...styles.card, ...(status?.tone === "error" ? styles.dangerCard : {}) }}>
        {machine && status ? (
          <>
            <div className="dashboard-row" style={styles.sectionHeader}>
              <span style={{ fontWeight: 600 }}>Treq Cloud machine</span>
              <span style={{ ...badge, ...TONE_STYLE[status.tone] }}>{status.label}</span>
            </div>
            <Field label="Size" styles={styles}>{sizeLabel(machine)}</Field>
            <Field label="Region" styles={styles}>{regionLabel(machine.region)}</Field>
            {machine.created_at && (
              <Field label="Created" styles={styles}>{formatMachineTime(machine.created_at)}</Field>
            )}
            {machine.updated_at && (
              <Field label="Updated" styles={styles}>{formatMachineTime(machine.updated_at)}</Field>
            )}
            {(status.tone === "error" || failure) && (
              <div role="alert" style={failureBox}>
                {status.tone === "error" && (
                  <div>{machine.status === "failed" ? "The machine failed." : "The machine is degraded."}</div>
                )}
                {failure && <div>Last error: {failure}</div>}
              </div>
            )}
          </>
        ) : (
          <p style={{ ...styles.billingNote, marginTop: 0 }}>You have no Treq Cloud machine.</p>
        )}

        {confirmingDelete && machine ? (
          <div role="alertdialog" aria-labelledby="delete-machine-heading" style={styles.billingActions}>
            <div style={{ width: "100%" }}>
              <strong id="delete-machine-heading">Delete this machine?</strong>
              <p style={styles.billingNote}>
                Its disk is deleted with it. Work that is not pushed is lost. This cannot be undone.
              </p>
            </div>
            <button
              type="button"
              onClick={() => setConfirmingDelete(false)}
              disabled={pending !== null}
              style={styles.secondaryButton}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => void run("delete")}
              disabled={pending !== null}
              style={{ ...styles.dangerButton, ...(pending ? styles.primaryButtonDisabled : {}) }}
            >
              {pending === "delete" ? PENDING_LABEL.delete : "Delete machine"}
            </button>
          </div>
        ) : (
          actions.length > 0 && (
            <div style={styles.billingActions}>
              {actions.includes("wake") && (
                <button type="button" onClick={() => void run("wake")} disabled={pending !== null} style={styles.primaryButton}>
                  {pending === "wake" ? PENDING_LABEL.wake : "Start"}
                </button>
              )}
              {actions.includes("ensure") && (
                <button type="button" onClick={() => void run("ensure")} disabled={pending !== null} style={styles.primaryButton}>
                  {pending === "ensure" ? PENDING_LABEL.ensure : machine ? "Retry" : "Create machine"}
                </button>
              )}
              {actions.includes("delete") && (
                <button
                  type="button"
                  onClick={() => setConfirmingDelete(true)}
                  disabled={pending !== null}
                  style={styles.dangerButton}
                >
                  Delete…
                </button>
              )}
            </div>
          )
        )}

        {error && (
          <div role="alert" style={styles.errorText}>
            {error}
          </div>
        )}

        {upsell && (
          <div style={{ fontSize: "0.85rem", marginTop: "1rem" }}>
            <div>
              {upsell}
              <span style={styles.featureProBadge}>PRO</span>
            </div>
            <button type="button" onClick={onUpgrade} style={{ ...styles.primaryButton, marginTop: "0.5rem" }}>
              Upgrade to Pro
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
