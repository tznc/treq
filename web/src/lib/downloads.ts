export const DOWNLOAD_HREF = 'https://github.com/tznc/treq/releases/latest';

export type ReleaseAsset = {name: string; url: string};

const ARM = /aarch64|arm64/i;

export type PlatformId = 'macos' | 'windows' | 'linux' | 'android';

export const PLATFORM_NAMES: Record<PlatformId, string> = {
  macos: 'macOS',
  windows: 'Windows',
  linux: 'Linux',
  android: 'Android',
};

export type InstallerLink = {href: string; label: string; format: string};

export type PlatformDownloads = {
  id: PlatformId;
  name: string;
  // An early build, labelled as such on the download page.
  preview: boolean;
  installers: InstallerLink[];
};

// iOS user agents also contain "Mac OS X", and Android's contain "Linux".
export function detectPlatform(userAgent: string): PlatformId | null {
  if (/Android/.test(userAgent)) return 'android';
  if (/iPhone|iPad|iPod/.test(userAgent)) return null;
  if (/Mac OS X/.test(userAgent)) return 'macos';
  if (/Windows/.test(userAgent)) return 'windows';
  if (/Linux/.test(userAgent)) return 'linux';
  return null;
}

const PLATFORMS: {
  id: PlatformId;
  preview?: boolean;
  installers: {ext: RegExp; format: string; label: string; arm?: boolean}[];
}[] = [
  {
    id: 'macos',
    installers: [
      {ext: /\.dmg$/i, format: '.dmg', label: 'Apple silicon', arm: true},
      {ext: /\.dmg$/i, format: '.dmg', label: 'Intel', arm: false},
    ],
  },
  {
    id: 'windows',
    installers: [
      {ext: /\.exe$/i, format: '.exe', label: 'Installer'},
      {ext: /\.msi$/i, format: '.msi', label: 'MSI package'},
    ],
  },
  {
    id: 'linux',
    installers: [
      {ext: /\.AppImage$/i, format: '.AppImage', label: 'AppImage'},
      {ext: /\.deb$/i, format: '.deb', label: 'Debian and Ubuntu'},
      {ext: /\.rpm$/i, format: '.rpm', label: 'Fedora and RHEL'},
    ],
  },
  {
    id: 'android',
    preview: true,
    installers: [{ext: /\.apk$/i, format: '.apk', label: 'arm64'}],
  },
];

// Groups the release's installers by platform, the current platform first.
// A platform with no installer in the release is left out.
export function listPlatforms(
  assets: ReleaseAsset[],
  current: PlatformId | null,
): PlatformDownloads[] {
  const platforms = PLATFORMS.map(({id, preview = false, installers}) => ({
    id,
    name: PLATFORM_NAMES[id],
    preview,
    installers: installers.flatMap(({ext, format, label, arm}) => {
      const match = assets.find(
        (a) => ext.test(a.name) && (arm === undefined || ARM.test(a.name) === arm),
      );
      return match ? [{href: match.url, label, format}] : [];
    }),
  })).filter((p) => p.installers.length > 0);
  return [
    ...platforms.filter((p) => p.id === current),
    ...platforms.filter((p) => p.id !== current),
  ];
}
