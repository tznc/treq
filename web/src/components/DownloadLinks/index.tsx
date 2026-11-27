import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import useIsBrowser from '@docusaurus/useIsBrowser';
import {trackEvent} from '@site/src/lib/analytics';
import {PLATFORM_NAMES, detectPlatform} from '@site/src/lib/downloads';

// Opens /download, which lists the installers. The server render and first
// client render say "Download Treq"; the visitor's OS swaps in after hydration.
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
  const isBrowser = useIsBrowser();
  const platform = isBrowser ? detectPlatform(navigator.userAgent) : null;
  return (
    <Link
      className={className ?? ''}
      to="/download"
      onClick={() => trackEvent('download_cta_click', {location})}>
      {icon}
      {label ?? (platform ? `Download for ${PLATFORM_NAMES[platform]}` : 'Download Treq')}
    </Link>
  );
}
