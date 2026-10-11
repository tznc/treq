import React, { useEffect, useState } from "react";
import Layout from "@theme/Layout";
import BrowserOnly from "@docusaurus/BrowserOnly";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import { functionErrorMessage } from "../../../lib/billing";
import { proRequiredMessage } from "../../../lib/pro-required";
import { supabase } from "../../../lib/supabase";
import { signInHrefFor } from "../../../lib/utils";

type State =
  | "loading"
  | "success"
  | "choose_organization"
  | "error"
  | "needs_pro"
  | "unauthenticated";

function CallbackContent() {
  const [state, setState] = useState<State>("loading");
  const [message, setMessage] = useState("");

  useEffect(() => {
    const run = async () => {
      const params = new URLSearchParams(window.location.search);
      const installationId = params.get("installation_id");
      const installState = params.get("state");
      // GitHub adds `code` when the App requests user authorization during
      // installation. The function exchanges it to confirm the GitHub user
      // can access installation_id, and refuses without it.
      const code = params.get("code");

      if (!installationId) {
        setState("error");
        setMessage("No installation_id in URL.");
        return;
      }

      const {
        data: { session },
      } = await supabase.auth.getSession();

      if (!session) {
        setState("unauthenticated");
        setMessage(
          "Please sign in, then restart the installation from the dashboard."
        );
        return;
      }

      if (!installState) {
        // Returning from "manage access" on an already-linked installation:
        // there is nothing to link without a fresh install intent.
        window.location.href = "/dashboard?tab=integrations";
        return;
      }

      // Linking is completed server-side: the single-use state must match an
      // unexpired intent created by this user, and the installation must be
      // one the authorizing GitHub user can access.
      const { data, error } = await supabase.functions.invoke(
        "complete-github-installation",
        {
          body: {
            installation_id: parseInt(installationId, 10),
            state: installState,
            code,
          },
        }
      );

      const needsPro = await proRequiredMessage(error);
      if (needsPro) {
        setState("needs_pro");
        setMessage(needsPro);
        return;
      }
      if (error || data?.error) {
        setState("error");
        setMessage(
          data?.error ??
            (await functionErrorMessage(error, error?.message ?? "Unknown error"))
        );
        return;
      }

      // A GitHub organization's installation that no Treq organization owns
      // yet: an owner chooses one on the Team tab.
      const chooseOrganization =
        data?.account_type === "Organization" && !data?.organization_id;
      setState(chooseOrganization ? "choose_organization" : "success");

      // Redirect to dashboard after a brief pause
      setTimeout(
        () => {
          window.location.href = chooseOrganization
            ? "/dashboard?tab=team"
            : "/dashboard?tab=integrations";
        },
        chooseOrganization ? 4000 : 2000
      );
    };

    run();
  }, []);

  return (
    <div style={styles.container}>
      {state === "loading" && (
        <>
          <div style={styles.spinner} />
          <p style={styles.text}>Connecting GitHub App…</p>
        </>
      )}

      {state === "success" && (
        <>
          <div style={styles.icon}>✓</div>
          <p style={styles.text}>
            GitHub App connected successfully! Redirecting to your dashboard…
          </p>
        </>
      )}

      {state === "choose_organization" && (
        <>
          <div style={styles.icon}>✓</div>
          <p style={styles.text}>
            GitHub App connected. It belongs to a GitHub organization, so choose
            which Treq organization owns it by attaching it on the Team tab.
            Until then it stays linked to your account.
          </p>
          <a href="/dashboard?tab=team" style={styles.link}>
            Go to the Team tab
          </a>
        </>
      )}

      {state === "error" && (
        <>
          <div style={{ ...styles.icon, color: "#ef4444" }}>✕</div>
          <p style={{ ...styles.text, color: "#ef4444" }}>
            Connection failed: {message}
          </p>
          <a href="/dashboard?tab=integrations" style={styles.link}>
            Return to dashboard
          </a>
        </>
      )}

      {state === "needs_pro" && (
        <>
          <p style={styles.text}>{message}</p>
          <a href="/dashboard?tab=subscription" style={styles.link}>
            Upgrade to Pro
          </a>
        </>
      )}

      {state === "unauthenticated" && (
        <>
          <p style={styles.text}>You need to be signed in to link the GitHub App.</p>
          <a
            href={signInHrefFor(window.location.pathname + window.location.search)}
            style={styles.link}
          >
            Sign in
          </a>
        </>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    minHeight: "60vh",
    gap: "1rem",
  },
  spinner: {
    width: "32px",
    height: "32px",
    border: "3px solid var(--ifm-color-emphasis-200)",
    borderTopColor: "var(--ifm-color-primary)",
    borderRadius: "50%",
    animation: "spin 0.8s linear infinite",
  },
  icon: {
    fontSize: "3rem",
    color: "#10b981",
    fontWeight: 700,
  },
  text: {
    fontSize: "1rem",
    textAlign: "center",
    maxWidth: "480px",
  },
  link: {
    color: "var(--ifm-color-primary)",
    textDecoration: "underline",
    cursor: "pointer",
  },
};

export default function GitHubCallbackPage(): React.ReactNode {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as { pro?: boolean } | undefined;

  if (!flags?.pro) {
    return (
      <BrowserOnly>
        {() => {
          window.location.href = "/";
          return null;
        }}
      </BrowserOnly>
    );
  }

  return (
    <Layout title="GitHub Integration" description="Connecting your GitHub App">
      <BrowserOnly>{() => <CallbackContent />}</BrowserOnly>
    </Layout>
  );
}
