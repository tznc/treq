import { test, expect } from '@playwright/test';
import { DOWNLOAD_HREF, pickDownloads } from '../src/lib/downloads';

const UA = {
  mac: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  windows: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  linux: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  android: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36',
  iphone: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
};

const asset = (name: string) => ({
  name,
  url: `https://github.com/tznc/treq/releases/download/v0.3.0/${name}`,
});

const macOnly = [
  asset('treq_0.3.0_aarch64.dmg'),
  asset('treq_0.3.0_x64.dmg'),
  asset('treq_aarch64.app.tar.gz'),
  asset('treq_x64.app.tar.gz'),
];

const allPlatforms = [
  ...macOnly,
  asset('treq_0.3.0_x64_en-US.msi'),
  asset('treq_0.3.0_x64_en-US.msi.sig'),
  asset('treq_0.3.0_x64-setup.exe'),
  asset('treq_0.3.0_amd64.AppImage'),
  asset('treq_0.3.0_amd64.deb'),
];

test.describe('pickDownloads', () => {
  test('macOS gets the arm64 DMG with an Intel DMG link', () => {
    const { primary, secondary } = pickDownloads(macOnly, UA.mac);
    expect(primary).toEqual({ href: macOnly[0].url, label: 'Download for macOS', platform: 'mac-arm64' });
    expect(secondary).toEqual({ href: macOnly[1].url, label: 'Intel Mac', platform: 'mac-x64' });
  });

  test('Windows prefers the setup.exe over the msi', () => {
    expect(pickDownloads(allPlatforms, UA.windows).primary.href).toMatch(/_x64-setup\.exe$/);
    const msiOnly = allPlatforms.filter((a) => !a.name.endsWith('.exe'));
    expect(pickDownloads(msiOnly, UA.windows).primary.href).toMatch(/_x64_en-US\.msi$/);
  });

  test('Linux gets the AppImage with a deb link', () => {
    const { primary, secondary } = pickDownloads(allPlatforms, UA.linux);
    expect(primary.href).toMatch(/_amd64\.AppImage$/);
    expect(secondary?.href).toMatch(/_amd64\.deb$/);
  });

  test('falls back to the release page without a matching asset', () => {
    expect(pickDownloads(macOnly, UA.windows)).toEqual({
      primary: { href: DOWNLOAD_HREF, label: 'Download Treq', platform: 'release-page' },
      secondary: undefined,
    });
    expect(pickDownloads(macOnly, UA.linux).primary.href).toBe(DOWNLOAD_HREF);
    expect(pickDownloads([], UA.mac).primary.href).toBe(DOWNLOAD_HREF);
    expect(pickDownloads(allPlatforms, '').primary.href).toBe(DOWNLOAD_HREF);
  });

  test('mobile visitors get the release page', () => {
    expect(pickDownloads(allPlatforms, UA.android).primary.href).toBe(DOWNLOAD_HREF);
    expect(pickDownloads(allPlatforms, UA.iphone).primary.href).toBe(DOWNLOAD_HREF);
  });
});

test.describe('Landing page download links on macOS', () => {
  test.use({ userAgent: UA.mac });

  // Asset URLs come from the live releases API, which reports the repo's
  // canonical owner rather than an alias, so the owner is not pinned.
  test('link the latest release DMGs directly', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('link', { name: 'Download for macOS' }).first())
      .toHaveAttribute('href', /^https:\/\/github\.com\/[^/]+\/treq\/releases\/download\/v[\d.]+\/[^/]*aarch64[^/]*\.dmg$/);
    await expect(page.getByRole('link', { name: 'Intel Mac' }).first())
      .toHaveAttribute('href', /^https:\/\/github\.com\/[^/]+\/treq\/releases\/download\/v[\d.]+\/[^/]*x64[^/]*\.dmg$/);
    await expect(page.getByRole('link', { name: 'Other platforms' }).first())
      .toHaveAttribute('href', DOWNLOAD_HREF);
  });
});
