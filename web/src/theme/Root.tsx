import type React from 'react';
import BrowserOnly from '@docusaurus/BrowserOnly';
import Head from '@docusaurus/Head';
import { AuthProvider } from '@site/src/components/AuthProvider';
import { SearchDbProvider } from '@site/src/utils/SearchDbContext';
import interLatin400 from '@site/static/fonts/inter-400-latin.woff2';

export default function Root({ children }: { children: React.ReactNode }) {
  return (
    <>
      <Head>
        {/* Preload only the above-the-fold font: the regular sans Latin
            subset used by the navbar and body copy on every page. Importing
            the asset (rather than hardcoding its static path) keeps this
            href in sync with the hashed URL the CSS @font-face resolves to. */}
        <link
          rel="preload"
          as="font"
          type="font/woff2"
          href={interLatin400}
          crossOrigin="anonymous"
        />
      </Head>
      <AuthProvider>
        <BrowserOnly fallback={<>{children}</>}>
          {() => <SearchDbProvider>{children}</SearchDbProvider>}
        </BrowserOnly>
      </AuthProvider>
    </>
  );
}
