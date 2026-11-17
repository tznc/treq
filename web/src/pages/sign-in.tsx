import type React from "react";
import { useState, useEffect } from "react";
import Layout from "@theme/Layout";
import BrowserOnly from "@docusaurus/BrowserOnly";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import { supabase } from "../lib/supabase";
import { getAuthCallbackUrl, safeRedirectPath, signInCallbackQuery } from "../lib/utils";

type View = "sign-in" | "sign-up" | "forgot-password";

function LoginContent() {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as
    | { pro?: boolean; emailSignup?: boolean }
    | undefined;
  const emailSignupEnabled = Boolean(flags?.emailSignup);

  const [loading, setLoading] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [view, setView] = useState<View>("sign-in");
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const params = new URLSearchParams(window.location.search);
  const source = params.get("source") || "";
  const requestedRedirect = params.get("redirect");
  const redirectPath = safeRedirectPath(requestedRedirect);

  const goToNextPage = () => {
    window.location.href =
      source === "desktop" ? "/auth/callback?source=desktop" : redirectPath;
  };

  // Keep users on sign-in when email signup is disabled
  useEffect(() => {
    if (!emailSignupEnabled && view === "sign-up") {
      setView("sign-in");
    }
  }, [emailSignupEnabled, view]);

  // Check if already logged in
  useEffect(() => {
    supabase.auth.getSession().then(({ data: { session } }) => {
      if (session) goToNextPage();
    });
  }, [source, redirectPath]);

  const callbackUrl = (query?: string) =>
    getAuthCallbackUrl({
      siteUrl: siteConfig.url,
      browserOrigin: window.location.origin,
      isProduction: process.env.NODE_ENV === "production",
      query,
    });
  const redirectTo = callbackUrl(signInCallbackQuery(source, requestedRedirect));

  const switchView = (v: View) => {
    if (v === "sign-up" && !emailSignupEnabled) return;
    setView(v);
    setError(null);
    setMessage(null);
  };

  const signInWithGoogle = async () => {
    setLoading(true);
    setError(null);
    const { error } = await supabase.auth.signInWithOAuth({
      provider: "google",
      options: { redirectTo },
    });
    if (error) {
      setError(error.message);
      setLoading(false);
    }
  };

  const signInWithGitHub = async () => {
    setLoading(true);
    setError(null);
    const { error } = await supabase.auth.signInWithOAuth({
      provider: "github",
      options: { redirectTo },
    });
    if (error) {
      setError(error.message);
      setLoading(false);
    }
  };

  const handleSignIn = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);
    const { error } = await supabase.auth.signInWithPassword({ email, password });
    if (error) {
      setError(error.message);
    } else {
      goToNextPage();
    }
    setLoading(false);
  };

  const handleSignUp = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!emailSignupEnabled) return;
    setLoading(true);
    setError(null);
    setMessage(null);

    const { data, error } = await supabase.auth.signUp({ email, password });
    if (error) {
      setError(error.message);
    } else if (data.session) {
      // Email confirmation disabled — user is signed in immediately
      goToNextPage();
    } else {
      // Email confirmation required
      setMessage("Account created! Check your email to confirm your account, then sign in.");
    }
    setLoading(false);
  };

  const handleForgotPassword = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);
    setMessage(null);

    const { error } = await supabase.auth.resetPasswordForEmail(email, {
      redirectTo: callbackUrl("type=recovery"),
    });
    if (error) {
      setError(error.message);
    } else {
      setMessage("Password reset email sent. Check your inbox.");
    }
    setLoading(false);
  };

  // --- Forgot password view ---
  if (view === "forgot-password") {
    return (
      <div style={styles.container}>
        <div style={styles.card}>
          <h2 style={styles.title}>Reset your password</h2>
          <p style={styles.subtitle}>
            Enter your email and we&apos;ll send you a link to reset your password.
          </p>

          {error && <div style={styles.error}>{error}</div>}
          {message && <div style={styles.success}>{message}</div>}

          <form onSubmit={handleForgotPassword}>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="Email address"
              required
              disabled={loading}
              style={styles.input}
            />
            <button
              type="submit"
              disabled={loading || !email}
              style={styles.submitButton}
            >
              {loading ? "Sending..." : "Send Reset Link"}
            </button>
          </form>

          <div style={styles.toggleContainer}>
            <button onClick={() => switchView("sign-in")} style={styles.linkButton}>
              Back to sign in
            </button>
          </div>
        </div>
      </div>
    );
  }

  // --- Sign in / Sign up view ---
  const isSignUp = emailSignupEnabled && view === "sign-up";

  return (
    <div style={styles.container}>
      <div style={styles.card}>
        <h2 style={styles.title}>{isSignUp ? "Create an account" : "Sign in to Treq"}</h2>
        <p style={styles.subtitle}>
          {source === "desktop"
            ? "Sign in to connect your desktop app"
            : "Manage your account and subscription"}
        </p>

        {error && <div style={styles.error}>{error}</div>}
        {message && <div style={styles.success}>{message}</div>}

        <div style={styles.oauthButtons}>
          <button
            onClick={signInWithGoogle}
            disabled={loading}
            style={styles.oauthButton}
          >
            <svg width="18" height="18" viewBox="0 0 18 18" xmlns="http://www.w3.org/2000/svg">
              <path d="M17.64 9.2c0-.637-.057-1.251-.164-1.84H9v3.481h4.844a4.14 4.14 0 0 1-1.796 2.716v2.259h2.908c1.702-1.567 2.684-3.875 2.684-6.615Z" fill="#4285F4"/>
              <path d="M9 18c2.43 0 4.467-.806 5.956-2.18l-2.908-2.259c-.806.54-1.837.86-3.048.86-2.344 0-4.328-1.584-5.036-3.711H.957v2.332A8.997 8.997 0 0 0 9 18Z" fill="#34A853"/>
              <path d="M3.964 10.71A5.41 5.41 0 0 1 3.682 9c0-.593.102-1.17.282-1.71V4.958H.957A8.997 8.997 0 0 0 0 9c0 1.452.348 2.827.957 4.042l3.007-2.332Z" fill="#FBBC05"/>
              <path d="M9 3.58c1.321 0 2.508.454 3.44 1.345l2.582-2.58C13.463.891 11.426 0 9 0A8.997 8.997 0 0 0 .957 4.958L3.964 7.29C4.672 5.163 6.656 3.58 9 3.58Z" fill="#EA4335"/>
            </svg>
            Continue with Google
          </button>

          <button
            onClick={signInWithGitHub}
            disabled={loading}
            style={styles.oauthButton}
          >
            <svg width="18" height="18" viewBox="0 0 16 16" fill="currentColor" xmlns="http://www.w3.org/2000/svg">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8Z"/>
            </svg>
            Continue with GitHub
          </button>
        </div>

        {emailSignupEnabled && (
          <>
            <div style={styles.divider}>
              <span style={styles.dividerText}>or</span>
            </div>

            <form onSubmit={isSignUp ? handleSignUp : handleSignIn}>
              <input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="Email address"
                required
                disabled={loading}
                style={styles.input}
              />
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={isSignUp ? "Password (min. 8 characters)" : "Password"}
                required
                disabled={loading}
                minLength={8}
                style={styles.input}
              />
              <button
                type="submit"
                disabled={loading || !email || !password}
                style={styles.submitButton}
              >
                {loading ? "Please wait..." : isSignUp ? "Create Account" : "Sign In"}
              </button>
            </form>

            {!isSignUp && (
              <div style={styles.forgotContainer}>
                <button onClick={() => switchView("forgot-password")} style={styles.linkButton}>
                  Forgot password?
                </button>
              </div>
            )}

            <div style={styles.toggleContainer}>
              <span style={styles.toggleText}>
                {isSignUp ? "Already have an account?" : "Don't have an account?"}
              </span>
              <button
                onClick={() => switchView(isSignUp ? "sign-in" : "sign-up")}
                style={styles.linkButton}
              >
                {isSignUp ? "Sign in" : "Sign up"}
              </button>
            </div>
          </>
        )}
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
  },
  title: {
    fontSize: "1.5rem",
    fontWeight: 700,
    marginBottom: "0.5rem",
    textAlign: "center" as const,
  },
  subtitle: {
    fontSize: "0.9rem",
    color: "var(--ifm-color-emphasis-600)",
    textAlign: "center" as const,
    marginBottom: "1.5rem",
  },
  error: {
    padding: "0.75rem",
    borderRadius: "8px",
    backgroundColor: "rgba(239, 68, 68, 0.1)",
    color: "#ef4444",
    fontSize: "0.85rem",
    marginBottom: "1rem",
    textAlign: "center" as const,
  },
  success: {
    padding: "0.75rem",
    borderRadius: "8px",
    backgroundColor: "rgba(16, 185, 129, 0.1)",
    color: "#10b981",
    fontSize: "0.85rem",
    marginBottom: "1rem",
    textAlign: "center" as const,
  },
  oauthButtons: {
    display: "flex",
    flexDirection: "column" as const,
    gap: "0.75rem",
  },
  oauthButton: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "0.75rem",
    width: "100%",
    padding: "0.75rem 1rem",
    borderRadius: "8px",
    border: "1px solid var(--ifm-color-emphasis-300)",
    backgroundColor: "var(--ifm-background-color)",
    color: "var(--ifm-font-color-base)",
    fontSize: "0.9rem",
    fontWeight: 500,
    cursor: "pointer",
    transition: "all 0.2s",
  },
  divider: {
    display: "flex",
    alignItems: "center",
    margin: "1.5rem 0",
    gap: "1rem",
  },
  dividerText: {
    flex: 1,
    textAlign: "center" as const,
    fontSize: "0.8rem",
    color: "var(--ifm-color-emphasis-500)",
    borderTop: "1px solid var(--ifm-color-emphasis-200)",
    lineHeight: "0",
    paddingTop: "0.5rem",
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
  forgotContainer: {
    display: "flex",
    justifyContent: "center",
    marginTop: "0.75rem",
  },
  toggleContainer: {
    display: "flex",
    justifyContent: "center",
    alignItems: "center",
    gap: "0.4rem",
    marginTop: "1.25rem",
  },
  toggleText: {
    fontSize: "0.85rem",
    color: "var(--ifm-color-emphasis-500)",
  },
  linkButton: {
    background: "none",
    border: "none",
    color: "var(--ifm-color-primary)",
    cursor: "pointer",
    fontSize: "0.85rem",
    fontWeight: 600,
    padding: 0,
  },
};

export default function LoginPage(): React.ReactNode {
  const { siteConfig } = useDocusaurusContext();
  const flags = siteConfig.customFields?.featureFlags as
    | { pro?: boolean; emailSignup?: boolean }
    | undefined;

  if (!flags?.pro) {
    return (
      <BrowserOnly>{() => { window.location.href = "/"; return null; }}</BrowserOnly>
    );
  }

  return (
    <Layout title="Sign In" description="Sign in to your Treq account">
      <BrowserOnly>{() => <LoginContent />}</BrowserOnly>
    </Layout>
  );
}
