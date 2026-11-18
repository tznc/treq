import type React from "react";
import { useEffect, useState } from "react";
import Layout from "@theme/Layout";
import Head from "@docusaurus/Head";
import { parsePrPath, prDeepLink, prGithubUrl, type PrRef } from "../lib/openLink";

/** A shared Treq PR link: opens the PR in the desktop app via `treq://`. */
export default function OpenPrPage(): React.JSX.Element {
  const [pr, setPr] = useState<PrRef | null | undefined>(undefined);

  useEffect(() => {
    const parsed = parsePrPath(window.location.pathname);
    setPr(parsed);
    if (parsed) window.location.href = prDeepLink(parsed);
  }, []);

  return (
    <Layout title="Open pull request in Treq" noFooter>
      <Head>
        <meta name="robots" content="noindex" />
      </Head>
      <main style={styles.container}>
        {pr === null ? (
          <p>This link doesn&apos;t name a pull request.</p>
        ) : pr ? (
          <div style={styles.card}>
            <h2>
              Opening {pr.owner}/{pr.repo}#{pr.number} in Treq…
            </h2>
            <p>
              <a className="button button--primary" href={prDeepLink(pr)}>Open in Treq</a>
            </p>
            <p>
              <a href={prGithubUrl(pr)}>View on GitHub</a> · <a href="/">Download Treq</a>
            </p>
          </div>
        ) : null}
      </main>
    </Layout>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", justifyContent: "center", alignItems: "center", minHeight: "60vh", padding: "2rem" },
  card: { textAlign: "center", maxWidth: "480px" },
};
