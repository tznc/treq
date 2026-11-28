import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Heading from '@theme/Heading';
import Layout from '@theme/Layout';
import useBaseUrl from '@docusaurus/useBaseUrl';
import {usePluginData} from '@docusaurus/useGlobalData';
import useIsBrowser from '@docusaurus/useIsBrowser';
import {trackEvent} from '@site/src/lib/analytics';
import {
  DOWNLOAD_HREF,
  detectPlatform,
  listPlatforms,
  type InstallerLink,
  type PlatformDownloads,
  type ReleaseAsset,
} from '@site/src/lib/downloads';
import styles from './download.module.css';

type LatestReleaseData =
  | {release: {version: string; assets: ReleaseAsset[]} | null}
  | undefined;

function PlatformIcon({id}: {id: PlatformDownloads['id']}): ReactNode {
  const src = useBaseUrl(`/img/platforms/${id}.svg`);
  return (
    <img
      className={clsx(styles.icon, id === 'macos' && styles.ink)}
      src={src}
      alt=""
      width={40}
      height={40}
    />
  );
}

function DownloadGlyph(): ReactNode {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3v12m-5-5 5 5 5-5M4 20h16" />
    </svg>
  );
}

function Installer({
  platform,
  installer,
  className,
}: {
  platform: PlatformDownloads;
  installer: InstallerLink;
  className: string | undefined;
}): ReactNode {
  return (
    <a
      className={className}
      href={installer.href}
      onClick={() =>
        trackEvent('download_click', {
          location: 'download_page',
          platform: platform.id,
          format: installer.format,
        })
      }>
      <DownloadGlyph />
      <span>{installer.label}</span>
      <span className={styles.format}>{installer.format}</span>
    </a>
  );
}

function Focus({
  platform,
  detected,
}: {
  platform: PlatformDownloads;
  detected: boolean;
}): ReactNode {
  return (
    <section className={styles.focus} aria-labelledby="focus-name">
      <div className={styles.focusHead}>
        <PlatformIcon id={platform.id} />
        <h2 id="focus-name" className={styles.focusName}>
          Download for {platform.name}
        </h2>
        {platform.preview ? <span className={styles.preview}>Preview</span> : null}
        {detected ? <span className={styles.tag}>Your system</span> : null}
      </div>
      <div className={styles.focusRows}>
        {platform.installers.map((installer, i) => (
          <Installer
            key={installer.href}
            platform={platform}
            installer={installer}
            className={clsx(
              'button button--lg',
              i === 0 ? 'button--primary' : 'button--secondary',
              styles.row,
            )}
          />
        ))}
      </div>
    </section>
  );
}

function Others({platforms}: {platforms: PlatformDownloads[]}): ReactNode {
  return (
    <section className={styles.others} aria-labelledby="others">
      <h2 id="others" className={styles.othersTitle}>
        Other platforms
      </h2>
      <ul className={styles.cards}>
        {platforms.map((platform) => (
          <li key={platform.id} className={styles.card}>
            <div className={styles.cardHead}>
              <PlatformIcon id={platform.id} />
              <h3 className={styles.cardName}>{platform.name}</h3>
              {platform.preview ? (
                <span className={styles.preview}>Preview</span>
              ) : null}
            </div>
            {platform.installers.map((installer) => (
              <Installer
                key={installer.href}
                platform={platform}
                installer={installer}
                className={styles.link}
              />
            ))}
          </li>
        ))}
      </ul>
    </section>
  );
}

export default function DownloadPage(): ReactNode {
  const isBrowser = useIsBrowser();
  const data = usePluginData('latest-release-plugin') as LatestReleaseData;
  const release = data?.release ?? null;
  // The server render has no user agent, so it shows macOS first. The
  // visitor's platform moves to the front after hydration.
  const current = isBrowser ? detectPlatform(navigator.userAgent) : null;
  const [focus, ...others] = listPlatforms(release?.assets ?? [], current);

  return (
    <Layout
      title="Download"
      description="Download Treq for macOS, Windows, Linux and Android. The desktop app is free and open source under Apache 2.0.">
      <main className={styles.page}>
        <div className={clsx(styles.wrap, styles.grid)}>
          <header className={clsx(styles.intro, styles.enter)}>
            <p className={styles.label}>
              {release ? `Version ${release.version} · ` : ''}Apache 2.0
            </p>
            <Heading as="h1" className={styles.title}>
              Download Treq
            </Heading>
            <p className={styles.lead}>
              Give each coding agent its own workspace. Treq rebases the
              dependent workspaces when the target branch moves.
            </p>
            <p className={styles.links}>
              <Link to="/changelog">Read the release notes</Link>
              <a href={DOWNLOAD_HREF} target="_blank" rel="noopener noreferrer">
                See every file on GitHub
              </a>
            </p>
          </header>
          {focus ? (
            <Focus platform={focus} detected={focus.id === current} />
          ) : (
            <section className={styles.focus} aria-label="Download Treq">
              <a className="button button--primary button--lg" href={DOWNLOAD_HREF}>
                Download from GitHub
              </a>
            </section>
          )}
          {others.length > 0 ? <Others platforms={others} /> : null}
        </div>
      </main>
    </Layout>
  );
}
