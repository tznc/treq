import type { ManagedInstanceState } from "../../lib/api-types-remote";

export const STAGE_LABELS: Record<ManagedInstanceState, string> = {
  unprovisioned: "Not provisioned",
  provisioning: "Creating cloud workspace...",
  bootstrapping: "Installing Treq, JJ, Git, and agents...",
  installing_access: "Configuring access...",
  verifying: "Verifying readiness...",
  ready: "Ready",
  suspended: "Suspended (idle)",
  waking: "Waking...",
  reprovisioning: "Reprovisioning...",
  degraded: "Degraded",
  failed: "Failed",
  deleting: "Deleting...",
  deleted: "Deleted",
};

export interface UserManagedFormValues {
  display_name: string;
  hostname: string;
  port: number;
  username: string;
  host_key_fingerprint: string;
  auth_identity_reference: string;
  alias: string | null;
}
