import {useRef, type ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Head from '@docusaurus/Head';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import useBaseUrl from '@docusaurus/useBaseUrl';
import {DownloadButton, OtherDownloads} from '@site/src/components/DownloadLinks';
import {DOWNLOAD_HREF} from '@site/src/lib/downloads';
import {CloseRail, HeroLines, RebaseDiagram} from '@site/src/components/landing/BranchLines';

import styles from './index.module.css';

function LandingShot({
  file,
  alt,
  width,
  height,
}: {
  file: string;
  alt: string;
  width: number;
  height: number;
}): ReactNode {
  const src = useBaseUrl(`/img/landing/${file}`);
  return <img src={src} alt={alt} width={width} height={height} loading="lazy" decoding="async" />;
}

function DownloadIcon(): ReactNode {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3v12m-5-5 5 5 5-5M4 20h16" />
    </svg>
  );
}

function ArrowIcon(): ReactNode {
  return (
    <svg width="14" height="10" viewBox="0 0 14 10" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
      <path d="M0 5h12M8 1l4 4-4 4" />
    </svg>
  );
}

function Hero(): ReactNode {
  const codeScreenshot = useBaseUrl('/img/code.png');
  const heroRef = useRef<HTMLElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);
  return (
    <section ref={heroRef} className={styles.hero} aria-label="Hero">
      <Head>
        <link rel="preload" as="image" href={codeScreenshot} fetchPriority="high" />
      </Head>
      <HeroLines heroRef={heroRef} textRef={textRef} frameRef={frameRef} />
      <div className={styles.wrap}>
        <div className={clsx(styles.grid, styles.heroGrid)}>
          <div ref={textRef} className={clsx(styles.heroText, styles.enter)}>
            <span className={styles.label}>Stacking agent development environment · Apache 2.0</span>
            <Heading as="h1">
              One workspace per agent. <span className={styles.mute}>Rebased when main moves.</span>
            </Heading>
            <p className={styles.lead}>
              Open a Git repo and give each agent an isolated checkout. When the target branch moves,
              Treq rebases the dependent workspaces.
            </p>
            <div className={styles.buttons}>
              <DownloadButton location="hero" className={clsx(styles.btn, styles.primary)} icon={<DownloadIcon />} />
              <Link className={clsx(styles.btn, styles.secondary)} to="/docs/getting-started/installation">
                Read the docs
              </Link>
            </div>
            <OtherDownloads location="hero" className={styles.heroOther} />
            <div className={styles.platforms}>
              <span>Available for</span>
              <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" fill="currentColor" viewBox="0 0 16 16" aria-label="macOS">
                <path d="M11.182.008C11.148-.03 9.923.023 8.857 1.18c-1.066 1.156-.902 2.482-.878 2.516s1.52.087 2.475-1.258.762-2.391.728-2.43m3.314 11.733c-.048-.096-2.325-1.234-2.113-3.422s1.675-2.789 1.698-2.854-.597-.79-1.254-1.157a3.7 3.7 0 0 0-1.563-.434c-.108-.003-.483-.095-1.254.116-.508.139-1.653.589-1.968.607-.316.018-1.256-.522-2.267-.665-.647-.125-1.333.131-1.824.328-.49.196-1.422.754-2.074 2.237-.652 1.482-.311 3.83-.067 4.56s.625 1.924 1.273 2.796c.576.984 1.34 1.667 1.659 1.899s1.219.386 1.843.067c.502-.308 1.408-.485 1.766-.472.357.013 1.061.154 1.782.539.571.197 1.111.115 1.652-.105.541-.221 1.324-1.059 2.238-2.758q.52-1.185.473-1.282"/>
                <path d="M11.182.008C11.148-.03 9.923.023 8.857 1.18c-1.066 1.156-.902 2.482-.878 2.516s1.52.087 2.475-1.258.762-2.391.728-2.43"/>
              </svg>
              <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" fill="currentColor" viewBox="0 0 16 16" aria-label="Windows">
                <path d="M.5 2.2 6.8 1.3v6.1H.5V2.2Zm7.2-1L15.5.1v7.3H7.7V1.2ZM.5 8.3h6.3v6.2l-6.3-.9V8.3Zm7.2 0h7.8v7.4l-7.8-1.1V8.3Z"/>
              </svg>
              <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" fill="currentColor" viewBox="0 0 16 16" aria-label="Linux">
                <path d="M8.2.2c-1.8 0-2.8 1.6-2.8 3.7 0 .8.1 1.4-.2 2.1-.3.6-1 1.1-1.5 1.8-.6.8-1 1.8-1.2 2.8-.2 1.1.2 1.8.8 1.9.5.1 1-.3 1.4-.8.7 1 1.8 1.6 3.3 1.6 1.4 0 2.6-.6 3.3-1.6.4.5.9.9 1.4.8.6-.1 1-1 .7-2.1-.2-1-.6-1.9-1.2-2.7-.5-.7-1.2-1.2-1.5-1.8-.3-.7-.1-1.3-.1-2C10.7 1.7 9.8.2 8.2.2ZM6.9 3.4c.3 0 .5.3.5.7s-.2.7-.5.7-.5-.3-.5-.7.2-.7.5-.7Zm2.3 0c.3 0 .5.3.5.7s-.2.7-.5.7-.5-.3-.5-.7.2-.7.5-.7ZM8 4.7c.5 0 .9.2.9.5s-.4.5-.9.5-.9-.2-.9-.5.4-.5.9-.5Zm-2.9 5.8c.6.4 1.6.7 2.9.7 1.2 0 2.2-.3 2.9-.7-.4 1.1-1.4 1.8-2.9 1.8s-2.5-.7-2.9-1.8Z"/>
              </svg>
            </div>
            <p className={styles.works}>
              Works with <b>Claude Code, Codex, Cursor, Copilot, Devin and Gemini</b>
            </p>
          </div>
          <div className={styles.heroArt}>
            <div ref={frameRef} className={styles.lit}>
              <div className={styles.win}>
                <img
                  src={codeScreenshot}
                  alt="Treq with the feat/empty-event-message workspace open: its stack on feat/event-ingest, the commit list and two agent terminals"
                  width={2880}
                  height={1800}
                  loading="eager"
                  decoding="async"
                  fetchPriority="high"
                />
              </div>
              <div className={styles.loop} aria-hidden="true">
                <div className={styles.loopRow}>
                  <span>Rebased 2 workspaces onto main</span>
                  <span className={styles.loopDone}>done</span>
                </div>
                <div className={styles.loopBar}>
                  <i />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function SectionHead({id, title, children}: {id: string; title: string; children?: ReactNode}): ReactNode {
  return (
    <div className={styles.head}>
      <h2 id={id}>{title}</h2>
      {children ? <div className={styles.side}>{children}</div> : null}
    </div>
  );
}

function ReviewSection(): ReactNode {
  return (
    <section className={styles.screen} aria-labelledby="review">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <SectionHead id="review" title="Read the whole change, or one commit, on the same screen">
            <p>
              File navigation, the diff and inline comments stay together. Line comments stay local. Send
              them to an agent as Plan or Edit.
            </p>
            <Link className={styles.textLink ?? ''} to="/docs/concepts/changes-and-reviews">
              Changes and reviews
            </Link>
          </SectionHead>
          <div className={styles.shot}>
            <div className={clsx(styles.lit, styles.reviewFrame)}>
              <div className={styles.win}>
                <LandingShot
                  file="review.png"
                  alt="Treq Changes tab with a resolved GitHub review thread and a local comment that says remove this"
                  width={2240}
                  height={1518}
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function RebaseSection(): ReactNode {
  return (
    <section className={styles.screen} aria-labelledby="rebase">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <SectionHead id="rebase" title="When the target branch moves, dependent workspaces rebase">
            <p>
              Git worktrees cannot restack a chain of branches for you. Treq uses Jujutsu for that rebase.
              You still push and pull with Git, and you never have to learn Jujutsu.
            </p>
            <Link className={styles.textLink ?? ''} to="/docs/concepts/workspaces">
              How workspaces work
            </Link>
          </SectionHead>
          <RebaseDiagram />
          <div className={styles.stackShot}>
            <div className={styles.lit}>
              <div className={styles.win}>
                <LandingShot file="sidebar.png" alt="Sidebar showing stacked workspace hierarchy" width={560} height={1800} />
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function AgentPrompt({branch, prompt}: {branch: string; prompt: string}): ReactNode {
  return (
    <div className={styles.prompt} role="img" aria-label={`Agent prompt: ${prompt}`}>
      <div className={styles.promptHead}>
        Start a new agent session<span aria-hidden="true">×</span>
      </div>
      <span className={styles.promptBranch}>{branch}</span>
      <div className={styles.promptInput}>
        <span className={styles.promptText}>{prompt}</span>
        <div className={styles.promptFoot}>
          <span className={styles.add}>+ Add</span>
          <span className={styles.select}>Local</span>
          <span className={styles.select}>Claude</span>
          <span className={styles.send}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="#071A33" aria-hidden="true">
              <path d="M3 20 21 12 3 4v6l12 2-12 2z" />
            </svg>
          </span>
        </div>
      </div>
    </div>
  );
}

const DELEGATE = [
  {
    title: 'Agents start more agents',
    body: 'Tell one agent to split the work across three agents in three workspaces. Each agent gets its own checkout and can keep going in parallel.',
    branch: 'feat/keyvalues-cache',
    prompt: 'Split the work across 3 agents in 3 different workspaces',
  },
  {
    title: 'Move changes between workspaces',
    body: 'Agents can move commits, working-copy files and hunks through the treq CLI, then continue the fix in the new workspace.',
    branch: 'feat/empty-event-message',
    prompt: 'Move my changes to a new workspace and continue the fix there',
  },
  {
    title: 'Break up a large workspace',
    body: 'Tell an agent to break this workspace into smaller stacked or separate workspaces. Each slice gets its own checkout.',
    branch: 'feat/empty-event-message',
    prompt: 'Break up this workspace into smaller stacked or separate workspaces',
  },
];

function DelegateSection(): ReactNode {
  return (
    <section className={styles.screen} aria-labelledby="delegate">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <SectionHead id="delegate" title="Delegate workspaces. Let agents move work and start more agents.">
            <p>Agents run the same treq commands you do. Ask in plain words and the workspaces appear in the sidebar.</p>
          </SectionHead>
          <div className={clsx(styles.cards, styles.three)}>
            {DELEGATE.map((card) => (
              <article key={card.title} className={styles.card}>
                <div className={clsx(styles.lit, styles.whole)}>
                  <AgentPrompt branch={card.branch} prompt={card.prompt} />
                </div>
                <div className={styles.copy}>
                  <Heading as="h3">{card.title}</Heading>
                  <p>{card.body}</p>
                </div>
              </article>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

const FACTS = [
  {value: 'Local', label: 'Diffs, comments and terminal metadata stay on your machine. The desktop app does not upload your code.'},
  {value: 'Apache 2.0', label: 'The desktop app is open source. You can read every command it runs.'},
  {value: 'Git compatible', label: 'Jujutsu under the hood. Treq uses the jj-lib Rust crate to rebase workspace branches when their targets move.'},
  {value: 'Cross-platform', label: 'Desktop builds for macOS, Windows and Linux ship on GitHub Releases. Open a Git repository you already have.'},
  {value: 'No telemetry', label: 'The app does not send feature usage, crash reports or performance metrics. Docs-site analytics is separate.'},
];

function FactsSection(): ReactNode {
  return (
    <section className={styles.screen} aria-labelledby="facts">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <SectionHead id="facts" title="The constraints that stay on your machine" />
          <div className={styles.facts}>
            {FACTS.map((fact) => (
              <div key={fact.value} className={styles.fact}>
                <b>{fact.value}</b>
                <p>{fact.label}</p>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

function IsolationSection(): ReactNode {
  const shots = [
    {branch: 'feat/keyvalues-cache', file: 'isolation-left.png', alt: 'Changes in feat/keyvalues-cache with an uncommitted cache.ts edit'},
    {branch: 'feat/empty-event-message', file: 'isolation-right.png', alt: 'Changes in feat/empty-event-message with a different uncommitted edit'},
  ];
  return (
    <section className={styles.screen} aria-labelledby="isolation">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <SectionHead id="isolation" title="An uncommitted change in one workspace never shows up in another">
            <p>Agents write in parallel without sharing your main checkout. Two workspaces, two different edits, no overlap.</p>
            <Link className={styles.textLink ?? ''} to="/docs/concepts/workspaces">
              Isolated workspaces
            </Link>
          </SectionHead>
          <div className={styles.pair}>
            {shots.map((shot) => (
              <div key={shot.file} className={clsx(styles.lit, styles.pairFrame)}>
                <div className={styles.win}>
                  <div className={styles.winbar}>
                    <i />
                    <i />
                    <i />
                    {shot.branch}
                  </div>
                  <LandingShot file={shot.file} alt={shot.alt} width={2240} height={1518} />
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

function MoreSection(): ReactNode {
  return (
    <section className={styles.screen} aria-labelledby="more">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <SectionHead id="more" title="Conflicts, the agent CLI, files and GitHub, next to the workspace">
            <p>The rest of the work stays in the same window as the diff.</p>
            <Link className={styles.textLink ?? ''} to="/docs/getting-started/installation">
              Read the docs
            </Link>
          </SectionHead>
          <div className={clsx(styles.cards, styles.two)}>
            <article className={styles.card}>
              <div className={clsx(styles.lit, styles.whole)}>
                <div className={styles.win}>
                  <LandingShot file="conflict-inplace.png" alt="Resolve commit conflicts in place dialog" width={1162} height={508} />
                </div>
              </div>
              <div className={styles.copy}>
                <Heading as="h3">Conflict resolution</Heading>
                <p>
                  Send a conflict to an agent from Changes with Plan or Edit. On the Commits tab, Resolve
                  conflicts starts an agent that rewrites the conflicted commit in place.
                </p>
              </div>
            </article>
            <article className={styles.card}>
              <div className={clsx(styles.lit, styles.whole)}>
                <AgentPrompt branch="feat/empty-event-message" prompt="create a workspace for this fix" />
              </div>
              <div className={styles.copy}>
                <Heading as="h3">Agent CLI</Heading>
                <p>
                  Workspace management can be delegated to agents through the treq CLI. Ask an agent to
                  create a workspace for this fix and it runs the same commands you would.
                </p>
              </div>
            </article>
            <article className={styles.card}>
              <div className={clsx(styles.lit, styles.cardFrame)}>
                <div className={styles.win}>
                  <LandingShot file="send-lightbox.png" alt="Asset lightbox open after clicking the thumbnail" width={2800} height={1800} />
                </div>
              </div>
              <div className={styles.copy}>
                <Heading as="h3">Send files from the agent</Heading>
                <p>
                  Tell an agent to send you the screenshots and the updated research draft. A thumbnail lands
                  in that terminal. Click it to open the file.
                </p>
              </div>
            </article>
            <article className={styles.card}>
              <div className={clsx(styles.lit, styles.cardFrame)}>
                <div className={styles.win}>
                  <LandingShot file="github.png" alt="GitHub pull request list in Treq" width={2240} height={1800} />
                </div>
              </div>
              <div className={styles.copy}>
                <Heading as="h3">GitHub issues and pull requests</Heading>
                <p>
                  Manage issues and pull requests in the app. Start an agent from an issue. CI for the branch
                  stays next to the workspace.
                </p>
              </div>
            </article>
          </div>
        </div>
      </div>
    </section>
  );
}

const RESOURCES = [
  {kind: 'Docs', title: 'Install Treq and open a repo', body: 'Download the app for macOS, Windows or Linux, point it at a Git repository, and create your first workspace.', to: '/docs/getting-started/installation', go: 'Installation'},
  {kind: 'Learn', title: 'Stacked PRs, worktrees and agents', body: 'Concept articles that explain the workflow Treq is built around.', to: '/learn', go: 'Learn'},
  {kind: 'Skills', title: 'Agent skills catalog', body: 'Reusable instructions you can drop into Claude, Codex, Cursor or Copilot sessions.', to: '/skills', go: 'Skills'},
  {kind: 'Roadmap', title: 'What ships next', body: 'Public milestones for the desktop app and Pro.', to: '/roadmap', go: 'Roadmap'},
];

function ResourcesSection(): ReactNode {
  return (
    <section className={styles.screen} aria-labelledby="resources">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <SectionHead id="resources" title="Install, then read how the workflow works" />
          <div className={clsx(styles.cards, styles.four)}>
            {RESOURCES.map((r) => (
              <Link key={r.to} className={styles.res ?? ''} to={r.to}>
                <span className={styles.label}>{r.kind}</span>
                <Heading as="h3">{r.title}</Heading>
                <p>{r.body}</p>
                <span className={styles.go}>
                  {r.go} <ArrowIcon />
                </span>
              </Link>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

function ClosingCTA(): ReactNode {
  return (
    <section className={clsx(styles.screen, styles.close)} aria-labelledby="download">
      <div className={styles.wrap}>
        <div className={styles.grid}>
          <h2 id="download">Download Treq.</h2>
          <p className={styles.closeSub}>The desktop app is Apache 2.0. Treq was used to build Treq.</p>
          <div className={styles.closeRail}>
            <CloseRail />
          </div>
          <DownloadButton location="closing" className={clsx(styles.btn, styles.primary, styles.big)} />
          <div className={styles.after}>
            <Link className={styles.textLink ?? ''} to="/pricing">
              See pricing
            </Link>
            <OtherDownloads location="closing" />
          </div>
        </div>
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
  return (
    <Layout
      title="Isolated workspaces and stacked PRs for coding agents"
      description="Stacking Agent Development Environment that isolates each agent and rebases stacked PRs when the base moves.">
      <Head>
        {/* Brand first: "treq" also names a Python library, and "treq ai" is the query that gets clicks. */}
        <title>Treq: isolated workspaces and stacked PRs for AI coding agents</title>
        <script type="application/ld+json">{JSON.stringify(SOFTWARE_APP_SCHEMA)}</script>
      </Head>
      <Hero />
      <main>
        <ReviewSection />
        <RebaseSection />
        <DelegateSection />
        <FactsSection />
        <IsolationSection />
        <MoreSection />
        <ResourcesSection />
        <ClosingCTA />
      </main>
    </Layout>
  );
}
