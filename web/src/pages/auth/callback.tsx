import type React from "react";
import { useEffect, useState } from "react";
import Layout from "@theme/Layout";
import Head from "@docusaurus/Head";
import BrowserOnly from "@docusaurus/BrowserOnly";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import { supabase } from "../../lib/supabase";
import { authCallbackDestination } from "../../lib/utils";

function CallbackContent() {
  const [status, setStatus] = useState<"loading" | "desktop" | "error">("loading");
  const [error, setError] = useState<string | null>(null);
  const [desktopLink, setDesktopLink] = useState<string | null>(null);

  useEffect(() => {
    const handleCallback = async () => {
      try {
        // Check for recovery (password reset) flow via hash params
        const hashParams = new URLSearchParams(window.location.hash.substring(1));
        const type = hashParams.get("type");

        if (type === "recovery") {
          // Exchange hash tokens to establish session, then redirect to reset page
          const accessToken = hashParams.get("access_token");
          const refreshToken = hashParams.get("refresh_token");
          if (accessToken && refreshToken) {
            await supabase.auth.setSession({
              access_token: accessToken,
              refresh_token: refreshToken,
            });
          }
          window.location.href = "/auth/reset-password";
          return;
        }

        // Check for ?type=recovery in query params (from our redirectTo)
        const searchParams = new URLSearchParams(window.location.search);
        if (searchParams.get("type") === "recovery") {
          const accessToken = hashParams.get("access_token");
          const refreshToken = hashParams.get("refresh_token");
          if (accessToken && refreshToken) {
            await supabase.auth.setSession({
              access_token: accessToken,
              refresh_token: refreshToken,
            });
          }
          window.location.href = "/auth/reset-password";
          return;
        }

        // Normal auth flow — try to get existing session first
        let { data: { session } } = await supabase.auth.getSession();

        // If no session, try to exchange hash fragment tokens (OAuth callback)
        if (!session) {
          const accessToken = hashParams.get("access_token");
          const refreshToken = hashParams.get("refresh_token");

          if (accessToken && refreshToken) {
            const { error: sessionError } = await supabase.auth.setSession({
              access_token: accessToken,
              refresh_token: refreshToken,
            });
            if (sessionError) throw sessionError;

            const result = await supabase.auth.getSession();
            session = result.data.session;
          }
        }

        if (!session) {
          throw new Error("Failed to establish session");
        }

        const destination = authCallbackDestination(searchParams);
        if (destination.kind === "web") {
          window.location.replace(destination.path);
          return;
        }

        // Create a one-time token for desktop auth handoff
        const { data: token, error: tokenError } = await supabase.rpc("create_desktop_token");
        if (tokenError || !token) {
          throw new Error("Failed to create desktop token");
        }
        const link = `treq://auth/callback?token=${token}`;

        setDesktopLink(link);
        setStatus("desktop");

        // Try to open the deep link — will silently fail if protocol isn't registered
        window.location.href = link;
      } catch (err: unknown) {
        setError(err instanceof Error ? err.message : "Authentication failed");
        setStatus("error");
      }
    };

    handleCallback();
  }, []);

  if (status === "loading") {
    return (
      <div style={styles.container}>
        <div style={styles.card}>
          <div style={styles.spinner} />
          <p style={styles.text}>Completing sign in...</p>
        </div>
      </div>
    );
  }

  if (status === "desktop") {
    return (
      <div style={styles.container}>
        <div style={styles.card}>
          <div style={{ fontSize: "2rem", marginBottom: "1rem" }}>&#10003;</div>
          <h2 style={styles.title}>You&apos;re signed in</h2>
          <p style={styles.text}>
            Opening Treq desktop app...
          </p>
          {desktopLink && (
            <div style={styles.linkBox}>
              <p style={styles.subtext}>
                If the app didn&apos;t open, click below:
              </p>
              <a href={desktopLink} style={styles.openAppButton}>Open Treq</a>
            </div>
          )}
          <a href="/dashboard" style={styles.link}>
            Continue to Dashboard
          </a>
        </div>
      </div>
    );
  }

  return (
    <div style={styles.container}>
      <div style={styles.card}>
        <h2 style={styles.title}>Authentication Error</h2>
        <p style={{ ...styles.text, color: "#ef4444" }}>{error}</p>
        <a href="/sign-in" style={styles.link}>
          Try again
        </a>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    justifyContent: "center",
    alignItems: "center",
    minHeight: "60vh",
    padding: "2rem",
  },
  card: {
    textAlign: "center" as const,
    padding: "2rem",
    maxWidth: "400px",
  },
  spinner: {
    width: "32px",
    height: "32px",
    border: "3px solid var(--ifm-color-emphasis-200)",
    borderTopColor: "var(--ifm-color-primary)",
    borderRadius: "50%",
    animation: "spin 0.8s linear infinite",
    margin: "0 auto 1rem",
  },
  title: {
    fontSize: "1.25rem",
    fontWeight: 700,
    marginBottom: "0.75rem",
  },
  text: {
    fontSize: "0.9rem",
    color: "var(--ifm-color-emphasis-600)",
    marginBottom: "0.5rem",
  },
  subtext: {
    fontSize: "0.8rem",
    color: "var(--ifm-color-emphasis-500)",
    marginTop: "1rem",
  },
  link: {
    display: "inline-block",
    marginTop: "1rem",
    color: "var(--ifm-color-primary)",
    fontWeight: 500,
  },
  linkBox: {
    margin: "1rem 0",
    padding: "1rem",
    borderRadius: "8px",
    backgroundColor: "var(--ifm-color-emphasis-100)",
  },
  openAppButton: {
    display: "inline-block",
    marginTop: "0.5rem",
    padding: "0.6rem 1.5rem",
    borderRadius: "8px",
    background: "linear-gradient(135deg, var(--ifm-color-primary) 0%, var(--ifm-color-primary-dark) 100%)",
    color: "#fff",
    fontSize: "0.9rem",
    fontWeight: 600,
    textDecoration: "none",
  },
};

export default function AuthCallback(): React.ReactNode {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as { pro?: boolean } | undefined;

  if (!flags?.pro) {
    return (
      <BrowserOnly>{() => { window.location.href = "/"; return null; }}</BrowserOnly>
    );
  }

  return (
    <Layout title="Signing in..." noFooter>
      <Head>
        <meta name="robots" content="noindex, nofollow" />
      </Head>
      <BrowserOnly>{() => <CallbackContent />}</BrowserOnly>
    </Layout>
  );
}
