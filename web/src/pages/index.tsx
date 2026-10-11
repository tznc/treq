import {useState, type ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Head from '@docusaurus/Head';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import {DownloadButton, OtherDownloads} from '@site/src/components/DownloadLinks';
import {DOWNLOAD_HREF} from '@site/src/lib/downloads';

import useBaseUrl from '@docusaurus/useBaseUrl';
import styles from './index.module.css';
import {RebaseGraphic, WorkspaceTreeGraphic} from '../components/landing/ProductGraphics';


function LandingShot({
  file,
  className,
  alt,
  width,
  height,
}: {
  file: string;
  className?: string;
  alt: string;
  width: number;
  height: number;
}) {
  const src = useBaseUrl(`/img/landing/${file}`);
  return (
    <img
      className={className}
      src={src}
      alt={alt}
      width={width}
      height={height}
      loading="lazy"
      decoding="async"
    />
  );
}

function DownloadIcon() {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
      <polyline points="7 10 12 15 17 10"/>
      <line x1="12" y1="15" x2="12" y2="3"/>
    </svg>
  );
}

function HomepageHeader() {
  const codeScreenshot = useBaseUrl('/img/code.png');
  return (
    <header className={styles.heroBanner} aria-label="Hero">
      <Head>
        <link rel="preload" as="image" href={codeScreenshot} fetchPriority="high" />
      </Head>
      <div className={styles.heroContainer}>
        <Heading as="h1" className={styles.heroTitle}>
          <span className={styles.heroAccent}>Stacking Agent Development Environment</span>
          {' '}that isolates each agent and rebases stacked PRs when the base moves
        </Heading>
        <p className={styles.heroLead}>
          Open a Git repo. Give each agent its own workspace. Treq rebases
          dependent workspaces when the target branch moves.
        </p>
        <div className={styles.buttons}>
          <DownloadButton
            location="hero"
            className={clsx('button', styles.primaryButton)}
            icon={<DownloadIcon />}
          />
          <Link
            className={clsx('button', styles.secondaryButton)}
            to="/docs/getting-started/installation">
            Read the docs
          </Link>
        </div>
        <OtherDownloads location="hero" className={styles.heroOtherDownloads} />
        <div className={styles.platformsSupported}>
          <span>Available for</span>
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" fill="currentColor" className={styles.platformIcon} viewBox="0 0 16 16" aria-label="macOS">
            <path d="M11.182.008C11.148-.03 9.923.023 8.857 1.18c-1.066 1.156-.902 2.482-.878 2.516s1.52.087 2.475-1.258.762-2.391.728-2.43m3.314 11.733c-.048-.096-2.325-1.234-2.113-3.422s1.675-2.789 1.698-2.854-.597-.79-1.254-1.157a3.7 3.7 0 0 0-1.563-.434c-.108-.003-.483-.095-1.254.116-.508.139-1.653.589-1.968.607-.316.018-1.256-.522-2.267-.665-.647-.125-1.333.131-1.824.328-.49.196-1.422.754-2.074 2.237-.652 1.482-.311 3.83-.067 4.56s.625 1.924 1.273 2.796c.576.984 1.34 1.667 1.659 1.899s1.219.386 1.843.067c.502-.308 1.408-.485 1.766-.472.357.013 1.061.154 1.782.539.571.197 1.111.115 1.652-.105.541-.221 1.324-1.059 2.238-2.758q.52-1.185.473-1.282"/>
            <path d="M11.182.008C11.148-.03 9.923.023 8.857 1.18c-1.066 1.156-.902 2.482-.878 2.516s1.52.087 2.475-1.258.762-2.391.728-2.43"/>
          </svg>
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" fill="currentColor" className={styles.platformIcon} viewBox="0 0 16 16" aria-label="Windows">
            <path d="M.5 2.2 6.8 1.3v6.1H.5V2.2Zm7.2-1L15.5.1v7.3H7.7V1.2ZM.5 8.3h6.3v6.2l-6.3-.9V8.3Zm7.2 0h7.8v7.4l-7.8-1.1V8.3Z"/>
          </svg>
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" fill="currentColor" className={styles.platformIcon} viewBox="0 0 16 16" aria-label="Linux">
            <path d="M8.2.2c-1.8 0-2.8 1.6-2.8 3.7 0 .8.1 1.4-.2 2.1-.3.6-1 1.1-1.5 1.8-.6.8-1 1.8-1.2 2.8-.2 1.1.2 1.8.8 1.9.5.1 1-.3 1.4-.8.7 1 1.8 1.6 3.3 1.6 1.4 0 2.6-.6 3.3-1.6.4.5.9.9 1.4.8.6-.1 1-1 .7-2.1-.2-1-.6-1.9-1.2-2.7-.5-.7-1.2-1.2-1.5-1.8-.3-.7-.1-1.3-.1-2C10.7 1.7 9.8.2 8.2.2ZM6.9 3.4c.3 0 .5.3.5.7s-.2.7-.5.7-.5-.3-.5-.7.2-.7.5-.7Zm2.3 0c.3 0 .5.3.5.7s-.2.7-.5.7-.5-.3-.5-.7.2-.7.5-.7ZM8 4.7c.5 0 .9.2.9.5s-.4.5-.9.5-.9-.2-.9-.5.4-.5.9-.5Zm-2.9 5.8c.6.4 1.6.7 2.9.7 1.2 0 2.2-.3 2.9-.7-.4 1.1-1.4 1.8-2.9 1.8s-2.5-.7-2.9-1.8Z"/>
          </svg>
        </div>
        <div className={styles.heroVisual}>
          <img
            className={styles.heroScreenshot}
            src={codeScreenshot}
            alt="Treq code overview screenshot"
            width={2880}
            height={1800}
            loading="eager"
            decoding="async"
            fetchPriority="high"
          />
        </div>
      </div>
    </header>
  );
}

function AgentBar(): ReactNode {
  return (
    <section className={styles.logoBar} aria-label="Supported agents">
      <span className={styles.logoBarLabel}>Works with</span>
      <div className={styles.logoBarLogos}>
        <span>Claude Code</span>
        <span>Codex</span>
        <span>Cursor</span>
      </div>
    </section>
  );
}

function HighlightGrid(): ReactNode {
  return (
    <section className={styles.highlightSection} aria-label="Product highlights">
      <div className={styles.highlightGrid}>
        <article className={styles.highlightTint}>
          <Heading as="h2" className={styles.highlightTitle}>
            Review the diff on your machine. Send it back as Plan or Edit.
          </Heading>
          <p>
            The workspace tabs are Code, Commits, and Changes. Line comments stay
            local. Send them to an agent as Plan or Edit.
          </p>
          <Link className={styles.highlightButton} to="/docs/concepts/changes-and-reviews">
            How reviews work
          </Link>
        </article>
        <article className={clsx(styles.highlightPlain, styles.highlightWide)}>
          <Heading as="h2" className={styles.highlightTitle}>
            When the target branch moves, dependent workspaces rebase.
          </Heading>
          <p>
            Git worktrees cannot restack a chain of branches for you. Treq uses
            Jujutsu for that rebase. You still push and pull with Git. You never
            have to learn Jujutsu.
          </p>
          <Link className={styles.highlightGhost} to="/docs/concepts/workspaces">
            How workspaces work
          </Link>
          <div className={styles.rebaseVisual}>
            <RebaseGraphic />
            <LandingShot
              className={clsx(styles.highlightCardImage, styles.sidebarCrop)}
              file="sidebar.png"
              alt="Sidebar showing stacked workspace hierarchy"
              width={560}
              height={1800}
            />
          </div>
        </article>
        <article className={styles.highlightDark}>
          <div className={styles.highlightDarkCopy}>
            <Heading as="h2" className={styles.highlightTitle}>
              Split a feature into a stack. Keep the chain current.
            </Heading>
            <p>
              Stack creates a workspace that targets another workspace. When you
              land the base, Treq rebases the rest. Merge locally with familiar
              and stack-native merge strategies.
            </p>
            <Link className={styles.highlightButtonLight} to="/docs/tutorials/merging-workspaces">
              Merging workspaces
            </Link>
          </div>
        </article>
      </div>
    </section>
  );
}

const SPAWN_CARD = {
  title: 'Agents start more agents',
  body: 'Tell one agent to split the work across three agents in three workspaces. Each agent gets its own checkout and can keep going in parallel.',
  file: 'spawn-before.png',
  alt: 'Agent prompt: Split the work across 3 agents in 3 different workspaces',
  afterFile: 'spawn-after.png',
  afterAlt: 'Sidebar after three new agent workspaces appear',
};

const DELEGATE_ROW_CARDS = [
  {
    title: 'Move changes between workspaces',
    body: 'Tell an agent to move your changes to a new workspace and continue the fix there. Agents can move commits, working-copy files, and hunks through the treq CLI.',
    file: 'move-changes.png',
    alt: 'Agent prompt: Move my changes to a new workspace and continue the fix there',
  },
  {
    title: 'Break up a large workspace',
    body: 'Tell an agent to break this workspace into smaller stacked or separate workspaces. Each slice gets its own checkout.',
    file: 'break-workspace.png',
    alt: 'Agent prompt: Break up this workspace into smaller stacked or separate workspaces',
  },
];

function DelegateCard({
  card,
  className,
}: {
  card: {
    title: string;
    body: string;
    file: string;
    alt: string;
    afterFile?: string;
    afterAlt?: string;
  };
  className?: string;
}): ReactNode {
  if (card.afterFile) {
    return (
      <article className={clsx(styles.carouselCard, className)}>
        <div className={styles.spawnLayout}>
          <div className={styles.spawnCopyCol}>
            <Heading as="h3" className={styles.carouselCardTitle}>{card.title}</Heading>
            <p>{card.body}</p>
            <figure>
              <figcaption>Before</figcaption>
              <LandingShot
                className={styles.containShot}
                file={card.file}
                alt={card.alt}
                width={1346}
                height={224}
              />
            </figure>
          </div>
          <figure className={styles.spawnAfterCol}>
            <figcaption>After</figcaption>
            <LandingShot
              className={styles.containShot}
              file={card.afterFile}
              alt={card.afterAlt ?? ''}
              width={560}
              height={1800}
            />
          </figure>
        </div>
      </article>
    );
  }

  return (
    <article className={clsx(styles.carouselCard, className)}>
      <LandingShot
        className={styles.containShot}
        file={card.file}
        alt={card.alt}
        width={1346}
        height={224}
      />
      <Heading as="h3" className={styles.carouselCardTitle}>{card.title}</Heading>
      <p>{card.body}</p>
    </article>
  );
}

function SolutionsCarousel(): ReactNode {
  return (
    <section className={styles.carouselSection} aria-label="Solutions">
      <div className={styles.carouselHeader}>
        <Heading as="h2" className={styles.carouselHeading}>
          Delegate workspaces. Let agents move work and start more agents.
        </Heading>
      </div>
      <div className={styles.carouselStack}>
        <DelegateCard card={SPAWN_CARD} className={styles.carouselHero} />
        <div className={styles.carouselRow}>
          {DELEGATE_ROW_CARDS.map((card) => (
            <DelegateCard key={card.title} card={card} />
          ))}
        </div>
      </div>
    </section>
  );
}

const FACTS = [
  {
    value: 'Local',
    label: 'Diffs, comments, and terminal metadata stay on your machine. The desktop app does not upload your code.',
  },
  {
    value: 'Apache 2.0',
    label: 'The desktop app is open source. You can read every command it runs.',
  },
  {
    value: 'Git compatible',
    label: 'Jujutsu under the hood. Treq uses the jj-lib Rust crate to automatically rebase workspace branches when targets move.',
  },
  {
    value: 'Cross-platform',
    label: 'Desktop builds for macOS, Windows, and Linux ship on GitHub Releases. Open a Git repository you already have.',
  },
  {
    value: 'No telemetry',
    label: 'The app does not send feature usage, crash reports, or performance metrics. Docs-site analytics is separate.',
  },
];

function StatsSection(): ReactNode {
  const [active, setActive] = useState(0);
  return (
    <section className={styles.statsSection} aria-label="Product facts">
      <div className={styles.statsSplit}>
        <div className={styles.statsNumbers} role="list">
          {FACTS.map((fact, i) => (
            <button
              key={fact.value}
              type="button"
              role="listitem"
              className={clsx(styles.statsNumber, i === active && styles.statsNumberActive)}
              onClick={() => setActive(i)}>
              {fact.value}
            </button>
          ))}
        </div>
        <div className={styles.statsCopy}>
          <p className={styles.kicker}>Facts</p>
          <Heading as="h2" className={styles.statsCopyHeading}>
            The constraints that stay on your machine
          </Heading>
          <p>{FACTS[active].label}</p>
        </div>
      </div>
    </section>
  );
}

function ShowcaseSection(): ReactNode {
  return (
    <section className={styles.showcaseSection} aria-label="Review showcase">
      <p className={styles.showcaseKicker}>Changes</p>
      <Heading as="h2" className={styles.showcaseHeading}>
        Read the whole change, or one commit, on the same screen
      </Heading>
      <p className={styles.showcaseLead}>
        File navigation, the diff, and inline comments stay together. When a
        workspace has an open GitHub pull request, Changes can also show GitHub
        review threads. Quoting a thread does not reply on GitHub.
      </p>
      <Link className={styles.showcaseCta} to="/docs/concepts/changes-and-reviews">
        Changes and reviews
      </Link>
      <LandingShot
        className={styles.showcaseImage}
        file="review.png"
        alt="Treq Changes tab with a resolved GitHub review thread and a local comment that says remove this"
        width={2320}
        height={1580}
      />
    </section>
  );
}

function ProofSection(): ReactNode {
  return (
    <section className={styles.proofSection} aria-label="How isolation works">
      <article className={styles.proofDark}>
        <p>
          An uncommitted change in one workspace never shows up in another.
          Agents can write in parallel without sharing your main checkout.
        </p>
        <p className={styles.proofAttr}>From the Workspaces docs</p>
        <div className={styles.proofShots}>
          <LandingShot
            className={styles.proofImage}
            file="isolation-left.png"
            alt="Changes in feat/keyvalues-cache with an uncommitted cache.ts edit"
            width={2320}
            height={1580}
          />
          <LandingShot
            className={styles.proofImage}
            file="isolation-right.png"
            alt="Changes in feat/empty-event-message with a different uncommitted Home.tsx edit"
            width={2320}
            height={1580}
          />
        </div>
      </article>
      <article className={styles.proofLight}>
        <p className={styles.proofStat}>0</p>
        <p className={styles.proofStatLabel}>telemetry events from the desktop app</p>
      </article>
    </section>
  );
}

function FeaturesSection(): ReactNode {
  return (
    <section className={styles.featuresSection} aria-label="Features">
      <div className={styles.featuresContainer}>
        <div className={styles.sectionHeader}>
          <p className={styles.kicker}>Product</p>
          <Heading as="h2" className={styles.featuresHeading}>
            Features
          </Heading>
          <p className={styles.featuresSubheading}>
            Review locally. Rebase the stack. Let agents open workspaces through
            the CLI.
          </p>
        </div>

        <div className={styles.featureRow}>
          <div className={styles.featureText}>
            <Heading as="h3" className={styles.featureHeading}>
              Code reviews
            </Heading>
            <p className={styles.featureDescription}>
              Inspect every change before it leaves the machine. Read diffs like a
              GitHub pull request, annotate line ranges, and send comments back with
              Plan or Edit.
            </p>
          </div>
          <div className={styles.featureScreenshot}>
            <LandingShot
              className={styles.featureImage}
              file="code-reviews.png"
              alt="Treq code review screenshot showing comments sent to Claude"
              width={2320}
              height={1580}
            />
          </div>
        </div>

        <div className={clsx(styles.featureRow, styles.featureRowReverse)}>
          <div className={styles.featureText}>
            <Heading as="h3" className={styles.featureHeading}>
              Isolated workspaces
            </Heading>
            <p className={styles.featureDescription}>
              Each agent gets its own checkout. An uncommitted change in one
              workspace never shows up in another.
            </p>
          </div>
          <div className={styles.featureScreenshot}>
            <WorkspaceTreeGraphic />
          </div>
        </div>

        <div className={styles.featureRow}>
          <div className={styles.featureText}>
            <Heading as="h3" className={styles.featureHeading}>
              Auto-rebase
            </Heading>
            <p className={styles.featureDescription}>
              When the base moves, Treq rebases dependent workspaces.
            </p>
          </div>
          <div className={styles.featureScreenshot}>
            <RebaseGraphic />
          </div>
        </div>

        <div className={clsx(styles.featureRow, styles.featureRowReverse)}>
          <div className={styles.featureText}>
            <Heading as="h3" className={styles.featureHeading}>
              Conflict resolution
            </Heading>
            <p className={styles.featureDescription}>
              Send a conflict to an agent from Changes with Plan or Edit. The
              agent can land a new commit that resolves the markers. On the
              Commits tab, Resolve conflicts starts an agent that rewrites the
              conflicted commit in place. That path does not add a follow-up
              commit.
            </p>
          </div>
          <div className={styles.featureScreenshot}>
            <div className={styles.beforeAfter}>
              <figure>
                <figcaption>New commit</figcaption>
                <LandingShot
                  className={styles.featureImage}
                  file="conflict-new-commit.png"
                  alt="Changes tab Resolve conflicts popover with Plan and Edit"
                  width={2320}
                  height={1588}
                />
              </figure>
              <figure>
                <figcaption>In place</figcaption>
                <LandingShot
                  className={styles.featureImage}
                  file="conflict-inplace.png"
                  alt="Resolve commit conflicts inplace dialog"
                  width={1212}
                  height={520}
                />
              </figure>
            </div>
          </div>
        </div>

        <div className={styles.featureRow}>
          <div className={styles.featureText}>
            <Heading as="h3" className={styles.featureHeading}>
              Agent CLI
            </Heading>
            <p className={styles.featureDescription}>
              Workspace management can be delegated to agents through the treq
              CLI. Prompt an agent to create a workspace for this fix and it can
              run the same commands you would.
            </p>
          </div>
          <div className={styles.featureScreenshot}>
            <LandingShot
              className={styles.featureImage}
              file="prompt.png"
              alt="Agent prompt asking to create a workspace for this fix"
              width={1346}
              height={224}
            />
          </div>
        </div>

        <div className={clsx(styles.featureRow, styles.featureRowReverse)}>
          <div className={styles.featureText}>
            <Heading as="h3" className={styles.featureHeading}>
              Send files from the agent
            </Heading>
            <p className={styles.featureDescription}>
              Tell an agent to send you the screenshots and the updated research
              draft. A thumbnail lands in that terminal. Click it to open the file.
            </p>
          </div>
          <div className={styles.featureScreenshot}>
            <div className={styles.beforeAfter}>
              <figure>
                <figcaption>Before</figcaption>
                <LandingShot
                  className={styles.featureImage}
                  file="send-thumb.png"
                  alt="Terminal pane with an asset thumbnail from treq send"
                  width={2320}
                  height={720}
                />
              </figure>
              <figure>
                <figcaption>After</figcaption>
                <LandingShot
                  className={styles.featureImage}
                  file="send-lightbox.png"
                  alt="Asset lightbox open after clicking the thumbnail"
                  width={2880}
                  height={1800}
                />
              </figure>
            </div>
          </div>
        </div>

        <div className={styles.featureRow}>
          <div className={styles.featureText}>
            <Heading as="h3" className={styles.featureHeading}>
              GitHub issues and pull requests
            </Heading>
            <p className={styles.featureDescription}>
              Manage issues and pull requests in the app. Start an agent from an
              issue. CI for the branch stays next to the workspace.
            </p>
          </div>
          <div className={styles.featureScreenshot}>
            <div className={styles.githubShotPair}>
              <LandingShot
                className={styles.featureImage}
                file="github-issues.png"
                alt="GitHub issues list in Treq"
                width={2320}
                height={1800}
              />
              <LandingShot
                className={styles.featureImage}
                file="github.png"
                alt="GitHub pull request list in Treq"
                width={2320}
                height={1800}
              />
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function ResourcesSection(): ReactNode {
  return (
    <section className={styles.resourcesSection} aria-label="Resources">
      <div className={styles.sectionInner}>
        <p className={styles.kicker}>Resources</p>
        <Heading as="h2" className={styles.sectionHeading}>
          Install, then read how the workflow works
        </Heading>
        <div className={styles.resourceGrid}>
          <Link className={styles.resourceCard} to="/docs/getting-started/installation">
            <span className={styles.resourceKind}>Docs</span>
            <Heading as="h3" className={styles.resourceTitle}>Install Treq and open a repo</Heading>
            <p>Download the app for macOS, Windows, or Linux, point it at a Git repository, and create your first workspace.</p>
          </Link>
          <Link className={styles.resourceCard} to="/learn">
            <span className={styles.resourceKind}>Learn</span>
            <Heading as="h3" className={styles.resourceTitle}>Stacked PRs, worktrees, and agents</Heading>
            <p>Concept articles that explain the workflow Treq is built around.</p>
          </Link>
          <Link className={styles.resourceCard} to="/skills">
            <span className={styles.resourceKind}>Skills</span>
            <Heading as="h3" className={styles.resourceTitle}>Agent skills catalog</Heading>
            <p>Reusable instructions you can drop into Claude, Codex, or Cursor sessions.</p>
          </Link>
          <Link className={styles.resourceCard} to="/roadmap">
            <span className={styles.resourceKind}>Roadmap</span>
            <Heading as="h3" className={styles.resourceTitle}>What ships next</Heading>
            <p>Public milestones for the desktop app and Pro.</p>
          </Link>
        </div>
      </div>
    </section>
  );
}

function ClosingCTA(): ReactNode {
  return (
    <section className={styles.closingCTA} aria-label="Download">
      <div className={styles.closingCTAContainer}>
        <Heading as="h2" className={styles.closingCTAHeading}>
          Download Treq.
        </Heading>
        <p className={styles.closingCTASubheading}>
          The desktop app is Apache 2.0. Treq was used to build Treq.
        </p>
        <div className={styles.closingCTAButtons}>
          <DownloadButton location="closing" className={styles.closingCTAButton} />
          <Link className={styles.closingCTASecondary} to="/pricing">
            See pricing
          </Link>
        </div>
        <OtherDownloads
          location="closing"
          className={styles.closingCTAOtherDownloads}
        />
      </div>
    </section>
  );
}

const SOFTWARE_APP_SCHEMA = {
  '@context': 'https://schema.org',
  '@type': 'SoftwareApplication',
  name: 'Treq',
  description:
    'Stacking Agent Development Environment that isolates each agent and rebases stacked PRs when the base moves.',
  url: 'https://treq.dev',
  applicationCategory: 'DeveloperApplication',
  operatingSystem: ['macOS', 'Windows', 'Linux'],
  offers: {
    '@type': 'Offer',
    price: '0',
    priceCurrency: 'USD',
  },
  downloadUrl: DOWNLOAD_HREF,
  codeRepository: 'https://github.com/tznc/treq',
  license: 'https://www.apache.org/licenses/LICENSE-2.0',
};

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title={siteConfig.title}
      description="Stacking Agent Development Environment that isolates each agent and rebases stacked PRs when the base moves.">
      <Head>
        <script type="application/ld+json">{JSON.stringify(SOFTWARE_APP_SCHEMA)}</script>
      </Head>
      <HomepageHeader />
      <AgentBar />
      <HighlightGrid />
      <SolutionsCarousel />
      <StatsSection />
      <ShowcaseSection />
      <ProofSection />
      <FeaturesSection />
      <ResourcesSection />
      <ClosingCTA />
    </Layout>
  );
}
