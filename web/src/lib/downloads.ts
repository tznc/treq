export const DOWNLOAD_HREF = 'https://github.com/tznc/treq/releases/latest';

export type DownloadPlatform =
  | 'mac-arm64'
  | 'mac-x64'
  | 'windows'
  | 'linux'
  | 'release-page';

export type DownloadLink = {
  href: string;
  label: string;
  platform: DownloadPlatform;
};

export type ReleaseAsset = {name: string; url: string};

export const RELEASE_PAGE_LINK: DownloadLink = {
  href: DOWNLOAD_HREF,
  label: 'Download Treq',
  platform: 'release-page',
};

const ARM = /aarch64|arm64/i;

function asset(
  assets: ReleaseAsset[],
  ext: RegExp,
  platform: DownloadPlatform,
  label: string,
  arm = false,
): DownloadLink | undefined {
  const match = assets.find((a) => ext.test(a.name) && ARM.test(a.name) === arm);
  return match && {href: match.url, label, platform};
}

// Picks installers for the visitor's OS from the latest release's assets.
// Browsers don't reliably say whether a Mac is Apple silicon or Intel, so
// macOS gets the arm64 DMG with the x64 DMG as a secondary link. Any OS
// without a matching asset falls back to the release page.
export function pickDownloads(
  assets: ReleaseAsset[],
  userAgent: string,
): {primary: DownloadLink; secondary?: DownloadLink} {
  // iOS user agents also contain "Mac OS X", and Android's contain "Linux".
  if (/Android|iPhone|iPad|iPod/.test(userAgent)) {
    return {primary: RELEASE_PAGE_LINK};
  }
  let primary: DownloadLink | undefined;
  let secondary: DownloadLink | undefined;
  if (/Mac OS X/.test(userAgent)) {
    primary = asset(assets, /\.dmg$/, 'mac-arm64', 'Download for macOS', true);
    secondary = asset(assets, /\.dmg$/, 'mac-x64', 'Intel Mac');
  } else if (/Windows/.test(userAgent)) {
    primary =
      asset(assets, /setup\.exe$/i, 'windows', 'Download for Windows') ??
      asset(assets, /\.msi$/i, 'windows', 'Download for Windows');
  } else if (/Linux/.test(userAgent)) {
    primary = asset(assets, /\.AppImage$/i, 'linux', 'Download for Linux');
    secondary = asset(assets, /\.deb$/, 'linux', '.deb package');
  }
  return {primary: primary ?? RELEASE_PAGE_LINK, secondary};
}
