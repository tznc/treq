import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';
import pkg from '../package.json';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const featureFlags = pkg.featureFlags;
const shouldLoadGoogleTag = process.env.DOCUSAURUS_ENABLE_GTAG === 'true';

const config: Config = {
  plugins: [
    require.resolve('./plugins/rawMarkdownPlugin'),
    require.resolve('./plugins/latestReleasePlugin'),
    require.resolve('./plugins/jsonLdPlugin'),
    require.resolve('./plugins/skillsPlugin'),
    require.resolve('./plugins/landingScreenshotsPlugin'),
    [
      '@docusaurus/plugin-content-docs',
      {
        id: 'learn',
        path: 'learn',
        routeBasePath: 'learn',
        sidebarPath: './sidebarsLearn.ts',
        editUrl: 'https://github.com/tznc/treq/tree/main/web/',
      },
    ],
    [
      '@docusaurus/plugin-content-docs',
      {
        id: 'compare',
        path: 'compare',
        routeBasePath: 'compare',
        sidebarPath: './sidebarsCompare.ts',
        editUrl: 'https://github.com/tznc/treq/tree/main/web/',
      },
    ],
    function chunkSplittingPlugin() {
      return {
        name: 'chunk-splitting-plugin',
        configureWebpack(_config: object, isServer: boolean) {
          if (isServer) return {};
          return {
            optimization: {
              splitChunks: {
                chunks: 'all' as const,
                cacheGroups: {
                  // three.js — only loaded on rubber-duck, vibe-idea-generator, and roadmap pages
                  three: {
                    test: /[\\/]node_modules[\\/]three[\\/]/,
                    name: 'chunk-three',
                    chunks: 'all' as const,
                    priority: 40,
                    enforce: true,
                  },
                  // React Flow — only loaded on dag-visualizer page
                  reactFlow: {
                    test: /[\\/]node_modules[\\/]@xyflow[\\/]/,
                    name: 'chunk-react-flow',
                    chunks: 'all' as const,
                    priority: 40,
                    enforce: true,
                  },
                  // Dagre — only loaded on dag-visualizer page
                  dagre: {
                    test: /[\\/]node_modules[\\/]@dagrejs[\\/]/,
                    name: 'chunk-dagre',
                    chunks: 'all' as const,
                    priority: 40,
                    enforce: true,
                  },
                  // Rough.js — shared between branch-visualizer and vcs-simulator
                  roughjs: {
                    test: /[\\/]node_modules[\\/]roughjs[\\/]/,
                    name: 'chunk-roughjs',
                    chunks: 'all' as const,
                    priority: 40,
                    enforce: true,
                  },
                  // Supabase — only loaded on sign-in/dashboard/auth pages
                  supabase: {
                    test: /[\\/]node_modules[\\/]@supabase[\\/]/,
                    name: 'chunk-supabase',
                    chunks: 'all' as const,
                    priority: 40,
                    enforce: true,
                  },
                  // sql.js — only loaded on demand via SearchDbContext
                  sqljs: {
                    test: /[\\/]node_modules[\\/]sql\.js[\\/]/,
                    name: 'chunk-sqljs',
                    chunks: 'all' as const,
                    priority: 40,
                    enforce: true,
                  },
                },
              },
            },
          };
        },
      };
    },
  ],

  title: 'Treq',
  tagline: 'Isolates each agent and rebases stacked PRs when the base moves',
  favicon: 'img/favicon.svg',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
    faster: true, // Rspack/SWC/lightningcss build pipeline - smaller, faster bundles
  },

  // Set the production url of your site here
  url: 'https://treq.dev',
  // Set the /<baseUrl>/ pathname under which your site is served
  baseUrl: '/',

  // NOTE: trailingSlash intentionally left unset (Docusaurus default).
  // Setting it to true or false here breaks the site's relative doc links
  // (e.g. "./sibling-page" in /learn/**), which assume the current
  // (undefined) resolution behavior. The /page vs /page/ duplicate-URL
  // issue seen in GSC should instead be fixed at the hosting/CDN layer by
  // adding a single 301-redirect rule normalizing one form to the other.

  customFields: {
    featureFlags,
  },

  onBrokenLinks: 'throw',

  // Even if you don't use internationalization, you can use this field to set
  // useful metadata like html lang. For example, if your site is Chinese, you
  // may want to replace "en" with "zh-Hans".
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          // Please change this to your repo.
          // Remove this to remove the "edit this page" links.
          editUrl:
            'https://github.com/tznc/treq/tree/main/web/',
        },
        blog: false,
        theme: {
          customCss: ['./src/css/fonts.css', './src/css/custom.css'],
        },
        sitemap: {
          ignorePatterns: [
            '/dashboard',
            '/login',
            '/sign-in',
            '/auth/**',
            '/integrations/**',
          ],
        },
        ...(shouldLoadGoogleTag ? {
          gtag: {
            trackingID: 'G-V9MPP2ZWZF',
            anonymizeIP: true,
          },
        } : {}),
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/treq-social-card.png',
    colorMode: {
      respectPrefersColorScheme: true,
    },
    navbar: {
      hideOnScroll: false,
      logo: {
        alt: 'Treq Logo',
        src: 'assets/combined-horizontal.png',
      },
      items: [
        {
          type: 'dropdown',
          label: 'Discover',
          position: 'left',
          items: [
            {
              type: 'docSidebar',
              sidebarId: 'learnSidebar',
              docsPluginId: 'learn',
              label: 'Learn',
            },
            {
              to: '/tools',
              label: 'Tools',
            },
          ],
        },
        {
          type: 'dropdown',
          label: 'Product',
          position: 'left',
          items: [
            {
              type: 'docSidebar',
              sidebarId: 'docsSidebar',
              label: 'Documentation',
            },
            {
              to: '/roadmap',
              label: 'Roadmap',
            },
            {
              to: '/changelog',
              label: 'Changelog',
            },
            {
              type: 'docSidebar',
              sidebarId: 'compareSidebar',
              docsPluginId: 'compare',
              label: 'Compare',
            },
          ],
        },
        {
          to: '/skills',
          label: 'Skills',
          position: 'left',
        },
        {
          to: '/pricing',
          label: 'Pricing',
          position: 'left',
        },

        {
          type: 'search',
          position: 'right',
        },
        {
          type: 'custom-authLinks',
          position: 'right',
        },
        {
          type: 'html',
          position: 'right',
          value:
            '<a href="https://github.com/tznc/treq" target="_blank" rel="noopener noreferrer" class="navbar__link header-github-link" aria-label="GitHub repository"></a>',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Product',
          items: [
            {
              label: 'Installation',
              to: '/docs/getting-started/installation',
            },
            {
              label: 'Pricing',
              to: '/pricing',
            },
            {
              label: 'Roadmap',
              to: '/roadmap',
            },
            {
              label: 'Private alpha',
              to: '/alpha',
            },
            {
              label: 'Changelog',
              to: '/changelog',
            },
          ],
        },
        {
          title: 'Docs',
          items: [
            {
              label: 'Learn',
              to: '/learn',
            },
            {
              label: 'Concepts',
              to: '/learn/concepts',
            },
            {
              label: 'Security and Privacy',
              to: '/docs/security-and-privacy',
            },
            {
              label: 'Skills',
              to: '/skills',
            },
            {
              label: 'Compare',
              to: '/compare',
            },
          ],
        },
        {
          title: 'Company',
          items: [
            {
              label: 'GitHub',
              href: 'https://github.com/tznc/treq',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Treq.<br />Treq is licensed under Apache License 2.0.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
