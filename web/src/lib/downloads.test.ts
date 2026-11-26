import {detectPlatform, listPlatforms} from './downloads';

const UA = {
  mac: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  windows: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  linux: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  android: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36',
  iphone: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
};

const asset = (name: string) => ({
  name,
  url: `https://github.com/tznc/treq/releases/download/v0.4.3/${name}`,
});

// The v0.4.3 release assets.
const release = [
  'treq-0.4.3-android-arm64.apk',
  'Treq-0.4.3-darwin-aarch64.app.tar.gz',
  'Treq-0.4.3-darwin-aarch64.dmg',
  'Treq-0.4.3-darwin-x64.app.tar.gz',
  'Treq-0.4.3-darwin-x64.dmg',
  'Treq-0.4.3-linux-amd64.AppImage',
  'Treq-0.4.3-linux-amd64.deb',
  'Treq-0.4.3-linux-x86_64.rpm',
  'Treq-0.4.3-windows-x64.exe',
  'Treq-0.4.3-windows-x64.msi',
  'treq-cli-0.4.3-linux-x86_64.tar.gz',
  'treq-cli-0.4.3-linux-x86_64.tar.gz.sha256',
].map(asset);

describe('detectPlatform', () => {
  test.each([
    ['mac', 'macos'],
    ['windows', 'windows'],
    ['linux', 'linux'],
    ['android', 'android'],
  ] as const)('%s user agent is %s', (ua, platform) => {
    expect(detectPlatform(UA[ua])).toBe(platform);
  });

  test('iOS and an empty user agent have no platform', () => {
    expect(detectPlatform(UA.iphone)).toBeNull();
    expect(detectPlatform('')).toBeNull();
  });
});

describe('listPlatforms', () => {
  test('groups installers by platform in a fixed order', () => {
    expect(listPlatforms(release, null)).toEqual([
      {
        id: 'macos',
        name: 'macOS',
        preview: false,
        installers: [
          {href: release[2]?.url, label: 'Apple silicon', format: '.dmg'},
          {href: release[4]?.url, label: 'Intel', format: '.dmg'},
        ],
      },
      {
        id: 'windows',
        name: 'Windows',
        preview: false,
        installers: [
          {href: release[8]?.url, label: 'Installer', format: '.exe'},
          {href: release[9]?.url, label: 'MSI package', format: '.msi'},
        ],
      },
      {
        id: 'linux',
        name: 'Linux',
        preview: false,
        installers: [
          {href: release[5]?.url, label: 'AppImage', format: '.AppImage'},
          {href: release[6]?.url, label: 'Debian and Ubuntu', format: '.deb'},
          {href: release[7]?.url, label: 'Fedora and RHEL', format: '.rpm'},
        ],
      },
      {
        id: 'android',
        name: 'Android',
        preview: true,
        installers: [{href: release[0]?.url, label: 'arm64', format: '.apk'}],
      },
    ]);
  });

  test('puts the current platform first', () => {
    expect(listPlatforms(release, 'linux').map((p) => p.id)).toEqual([
      'linux',
      'macos',
      'windows',
      'android',
    ]);
  });

  test('leaves out platforms the release has no installer for', () => {
    const macOnly = release.filter((a) => a.name.includes('darwin'));
    expect(listPlatforms(macOnly, 'windows').map((p) => p.id)).toEqual(['macos']);
    expect(listPlatforms([], 'macos')).toEqual([]);
  });
});
