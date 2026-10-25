import type React from "react";
import { useEffect, useState } from "react";
import Layout from "@theme/Layout";
import Head from "@docusaurus/Head";
import BrowserOnly from "@docusaurus/BrowserOnly";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import { supabase } from "../../lib/supabase";

function ResetPasswordContent() {
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [hasSession, setHasSession] = useState(false);
  const [checking, setChecking] = useState(true);

  // Verify the user has a valid recovery session
  useEffect(() => {
    supabase.auth.getSession().then(({ data: { session } }) => {
      if (session) {
        setHasSession(true);
      }
      setChecking(false);
    });
  }, []);

  const handleResetPassword = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (password !== confirmPassword) {
      setError("Passwords do not match.");
      return;
    }

    if (password.length < 8) {
      setError("Password must be at least 8 characters.");
      return;
    }

    setLoading(true);
    const { error } = await supabase.auth.updateUser({ password });
    if (error) {
      setError(error.message);
    } else {
      setSuccess(true);
    }
    setLoading(false);
  };

  if (checking) {
    return (
      <div style={styles.container}>
        <div style={styles.card}>
          <div style={styles.spinner} />
          <p style={styles.text}>Verifying...</p>
        </div>
      </div>
    );
  }

  if (!hasSession) {
    return (
      <div style={styles.container}>
        <div style={styles.card}>
          <h2 style={styles.title}>Invalid or Expired Link</h2>
          <p style={styles.text}>
            This password reset link is invalid or has expired.
          </p>
          <a href="/sign-in" style={styles.link}>
            Request a new reset link
          </a>
        </div>
      </div>
    );
  }

  if (success) {
    return (
      <div style={styles.container}>
        <div style={styles.card}>
          <h2 style={styles.title}>Password Updated</h2>
          <p style={styles.text}>
            Your password has been reset successfully.
          </p>
          <a href="/dashboard" style={styles.primaryLink}>
            Go to Dashboard
          </a>
        </div>
      </div>
    );
  }

  return (
    <div style={styles.container}>
      <div style={styles.card}>
        <h2 style={styles.title}>Set a new password</h2>
        <p style={styles.subtitle}>Enter your new password below.</p>

        {error && <div style={styles.error}>{error}</div>}

        <form onSubmit={handleResetPassword}>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="New password (min. 8 characters)"
            required
            disabled={loading}
            minLength={8}
            style={styles.input}
          />
          <input
            type="password"
            value={confirmPassword}
            onChange={(e) => setConfirmPassword(e.target.value)}
            placeholder="Confirm new password"
            required
            disabled={loading}
            minLength={8}
            style={styles.input}
          />
          <button
            type="submit"
            disabled={loading || !password || !confirmPassword}
            style={styles.submitButton}
          >
            {loading ? "Updating..." : "Update Password"}
          </button>
        </form>
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
    width: "100%",
    maxWidth: "400px",
    padding: "2rem",
    borderRadius: "12px",
    border: "1px solid var(--ifm-color-emphasis-200)",
    backgroundColor: "var(--ifm-background-color)",
    textAlign: "center" as const,
  },
  title: {
    fontSize: "1.5rem",
    fontWeight: 700,
    marginBottom: "0.5rem",
  },
  subtitle: {
    fontSize: "0.9rem",
    color: "var(--ifm-color-emphasis-600)",
    marginBottom: "1.5rem",
  },
  text: {
    fontSize: "0.9rem",
    color: "var(--ifm-color-emphasis-600)",
    marginBottom: "0.5rem",
  },
  error: {
    padding: "0.75rem",
    borderRadius: "8px",
    backgroundColor: "rgba(239, 68, 68, 0.1)",
    color: "#ef4444",
    fontSize: "0.85rem",
    marginBottom: "1rem",
  },
  input: {
    width: "100%",
    padding: "0.75rem 1rem",
    borderRadius: "8px",
    border: "1px solid var(--ifm-color-emphasis-300)",
    backgroundColor: "var(--ifm-background-color)",
    color: "var(--ifm-font-color-base)",
    fontSize: "0.9rem",
    marginBottom: "0.75rem",
    boxSizing: "border-box" as const,
  },
  submitButton: {
    width: "100%",
    padding: "0.75rem 1rem",
    borderRadius: "8px",
    border: "none",
    background: "linear-gradient(135deg, var(--ifm-color-primary) 0%, var(--ifm-color-primary-dark) 100%)",
    color: "#fff",
    fontSize: "0.9rem",
    fontWeight: 600,
    cursor: "pointer",
    transition: "all 0.2s",
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
  link: {
    display: "inline-block",
    marginTop: "1rem",
    color: "var(--ifm-color-primary)",
    fontWeight: 500,
  },
  primaryLink: {
    display: "inline-block",
    marginTop: "1rem",
    padding: "0.6rem 1.5rem",
    borderRadius: "8px",
    background: "linear-gradient(135deg, var(--ifm-color-primary) 0%, var(--ifm-color-primary-dark) 100%)",
    color: "#fff",
    fontWeight: 600,
    textDecoration: "none",
  },
};

export default function ResetPasswordPage(): React.ReactNode {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as { pro?: boolean } | undefined;

  if (!flags?.pro) {
    return (
      <BrowserOnly>{() => { window.location.href = "/"; return null; }}</BrowserOnly>
    );
  }

  return (
    <Layout title="Reset Password" noFooter>
      <Head>
        <meta name="robots" content="noindex, nofollow" />
      </Head>
      <BrowserOnly>{() => <ResetPasswordContent />}</BrowserOnly>
    </Layout>
  );
}
