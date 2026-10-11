import type {ReactNode} from 'react';
import clsx from 'clsx';
import {usePluginData} from '@docusaurus/useGlobalData';
import useIsBrowser from '@docusaurus/useIsBrowser';
import {
  RELEASE_PAGE_LINK,
  pickDownloads,
  type DownloadLink,
  type ReleaseAsset,
} from '@site/src/lib/downloads';
import styles from './styles.module.css';

type LatestReleaseData = {release: {assets: ReleaseAsset[]} | null} | undefined;

// The server render and first client render use the release page link. The
// OS-specific installer link swaps in after hydration.
function useDownloads() {
  const isBrowser = useIsBrowser();
  const data = usePluginData('latest-release-plugin') as LatestReleaseData;
  return pickDownloads(
    data?.release?.assets ?? [],
    isBrowser ? navigator.userAgent : '',
  );
}

// gtag only exists when the site is built with DOCUSAURUS_ENABLE_GTAG=true.
function trackDownload(location: string, platform: DownloadLink['platform']) {
  if (typeof window.gtag === 'function') {
    window.gtag('event', 'download_click', {location, platform});
  }
}

function DownloadAnchor({
  link,
  location,
  className,
  children,
}: {
  link: DownloadLink;
  location: string;
  className?: string;
  children: ReactNode;
}): ReactNode {
  // Installers download in place. The release page opens in a new tab.
  const newTab =
    link.platform === 'release-page'
      ? {target: '_blank', rel: 'noopener noreferrer'}
      : {};
  return (
    <a
      className={className}
      href={link.href}
      onClick={() => trackDownload(location, link.platform)}
      {...newTab}>
      {children}
    </a>
  );
}

export function DownloadButton({
  location,
  className,
  label,
  icon,
}: {
  location: string;
  className?: string;
  label?: string;
  icon?: ReactNode;
}): ReactNode {
  const {primary} = useDownloads();
  return (
    <DownloadAnchor link={primary} location={location} className={className}>
      {icon}
      {label ?? primary.label}
    </DownloadAnchor>
  );
}

export function OtherDownloads({
  location,
  className,
}: {
  location: string;
  className?: string;
}): ReactNode {
  const {secondary} = useDownloads();
  return (
    <p className={clsx(styles.other, className)}>
      {secondary ? (
        <>
          <DownloadAnchor link={secondary} location={location}>
            {secondary.label}
          </DownloadAnchor>
          {' · '}
        </>
      ) : null}
      <DownloadAnchor link={RELEASE_PAGE_LINK} location={location}>
        Other platforms
      </DownloadAnchor>
    </p>
  );
}
