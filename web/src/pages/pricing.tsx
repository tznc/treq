import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Head from '@docusaurus/Head';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import {DOWNLOAD_HREF} from '@site/src/lib/downloads';
import {FAQ_ITEMS, PRICING_SCHEMA} from '@site/src/lib/pricing';

import styles from './pricing.module.css';

type PlanFeature = {
  text: ReactNode;
  key: string;
};

const FREE_FEATURES: PlanFeature[] = [
  {key: 'desktop', text: 'Full desktop app'},
  {
    key: 'github',
    text: (
      <>
        GitHub through your local <code>gh</code> CLI, for any repository
      </>
    ),
  },
  {key: 'linear', text: 'Linear through your personal API key'},
];

const PRO_FEATURES: PlanFeature[] = [
  {key: 'free', text: 'Everything in Free'},
  {key: 'github-app', text: 'Treq GitHub App'},
  {key: 'merge-queue', text: 'Merge queue (private alpha)'},
  {key: 'cloud', text: 'Managed cloud workspace (private alpha)'},
  {key: 'linear-oauth', text: 'Linear OAuth'},
];

const TEAM_FEATURES: PlanFeature[] = [
  {key: 'pro', text: 'Everything in Pro for every member'},
  {key: 'installation', text: 'Shared GitHub App installation'},
  {key: 'merge-queue', text: 'Shared merge queue (private alpha)'},
  {key: 'one-bill', text: 'One bill for the whole team'},
];

type ComparisonCell = {
  included: boolean;
  detail?: string;
};

type ComparisonRow = {
  feature: string;
  free: ComparisonCell;
  pro: ComparisonCell;
  team: ComparisonCell;
};

const COMPARISON_ROWS: ComparisonRow[] = [
  {
    feature: 'Desktop app',
    free: {included: true},
    pro: {included: true},
    team: {included: true},
  },
  {
    feature: 'GitHub',
    free: {included: true, detail: 'Local gh CLI'},
    pro: {included: true, detail: 'Treq GitHub App'},
    team: {included: true, detail: 'Shared GitHub App installation'},
  },
  {
    feature: 'Merge queue',
    free: {included: false},
    pro: {included: true, detail: 'Private alpha'},
    team: {included: true, detail: 'Shared, private alpha'},
  },
  {
    feature: 'Managed cloud workspace',
    free: {included: false},
    pro: {included: true, detail: 'Private alpha'},
    team: {included: true, detail: 'Private alpha, for each member'},
  },
  {
    feature: 'Linear',
    free: {included: true, detail: 'Personal API key'},
    pro: {included: true, detail: 'API key or OAuth'},
    team: {included: true, detail: 'API key or OAuth'},
  },
  {
    feature: 'Price',
    free: {included: true, detail: 'US$0'},
    pro: {included: true, detail: 'US$15 per user per month'},
    team: {included: true, detail: 'US$199 per month for 5 members, US$8 per extra member'},
  },
  {
    feature: 'Free trial',
    free: {included: false},
    pro: {included: true, detail: '14 days, card required'},
    team: {included: false},
  },
];

function FeatureList({features}: {features: PlanFeature[]}): ReactNode {
  return (
    <ul className={styles.featureList}>
      {features.map((feature) => (
        <li
          key={feature.key}
          className={clsx(styles.featureItem, styles.featureIncluded)}
        >
          <span className={styles.featureMark} aria-hidden="true">
            ✓
          </span>
          <span className={styles.featureText}>
            <span>{feature.text}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}

function PlansSection(): ReactNode {
  return (
    <section className={styles.plans} aria-label="Plans">
      <article className={styles.plan} aria-labelledby="plan-free">
        <Heading as="h2" id="plan-free" className={styles.planName}>
          Free
        </Heading>
        <p className={styles.planSeats}>Open source</p>
        <p className={styles.planPrice}>
          <span className={styles.priceAmount}>$0</span>
        </p>
        <FeatureList features={FREE_FEATURES} />
        <Link
          className={clsx('button', styles.planButton, styles.planButtonSecondary)}
          href={DOWNLOAD_HREF}
        >
          Download Treq
        </Link>
      </article>

      <article
        className={clsx(styles.plan, styles.planPro)}
        aria-labelledby="plan-pro"
      >
        <Heading as="h2" id="plan-pro" className={styles.planName}>
          Pro
        </Heading>
        <p className={styles.planSeats}>14-day free trial, card required</p>
        <p className={styles.planPrice}>
          <span className={styles.priceAmount}>$15</span>
          <span className={styles.priceUnit}>/user/month</span>
        </p>
        <FeatureList features={PRO_FEATURES} />
        <Link
          className={clsx('button', styles.planButton, styles.planButtonPrimary)}
          to="/dashboard?tab=subscription"
        >
          Start 14-day free trial
        </Link>
      </article>

      <article className={styles.plan} aria-labelledby="plan-team">
        <Heading as="h2" id="plan-team" className={styles.planName}>
          Team
        </Heading>
        <p className={styles.planSeats}>5 members included, US$8 per extra</p>
        <p className={styles.planPrice}>
          <span className={styles.priceAmount}>$199</span>
          <span className={styles.priceUnit}>/month</span>
        </p>
        <FeatureList features={TEAM_FEATURES} />
        <Link
          className={clsx('button', styles.planButton, styles.planButtonSecondary)}
          to="/dashboard?tab=team"
        >
          Set up Team
        </Link>
      </article>
    </section>
  );
}

function ComparisonValue({cell}: {cell: ComparisonCell}): ReactNode {
  const statusLabel = cell.included ? 'Included' : 'Not included';
  const ariaLabel = cell.detail
    ? `${statusLabel}, ${cell.detail}`
    : statusLabel;

  return (
    <span className={styles.comparisonValue} aria-label={ariaLabel}>
      <span
        className={clsx(
          styles.comparisonIcon,
          cell.included
            ? styles.comparisonIconYes
            : styles.comparisonIconNo,
        )}
        aria-hidden="true"
      >
        {cell.included ? '✓' : '✗'}
      </span>
      {cell.detail ? (
        <span className={styles.comparisonDetail}>{cell.detail}</span>
      ) : null}
    </span>
  );
}

function ComparisonSection(): ReactNode {
  return (
    <section className={styles.comparison} aria-label="Feature comparison">
      <Heading
        as="h2"
        className={clsx(styles.sectionHeading, styles.comparisonHeading)}
      >
        Feature comparison
      </Heading>
      <div className={styles.comparisonTableWrap}>
        <table className={styles.comparisonTable}>
          <colgroup>
            <col className={styles.comparisonFeatureCol} />
            <col className={styles.comparisonPlanCol} />
            <col className={styles.comparisonPlanCol} />
            <col className={styles.comparisonPlanCol} />
          </colgroup>
          <thead>
            <tr>
              <th scope="col" className={styles.comparisonFeatureHeader}>
                Feature
              </th>
              <th scope="col" className={styles.comparisonPlanHeader}>
                Free
              </th>
              <th
                scope="col"
                className={clsx(
                  styles.comparisonPlanHeader,
                  styles.comparisonPlanHeaderPro,
                )}
              >
                Pro
              </th>
              <th scope="col" className={styles.comparisonPlanHeader}>
                Team
              </th>
            </tr>
          </thead>
          <tbody>
            {COMPARISON_ROWS.map((row) => (
              <tr key={row.feature} className={styles.comparisonRow}>
                <th scope="row" className={styles.comparisonFeatureCell}>
                  <span className={styles.comparisonFeatureLabel}>
                    <span>{row.feature}</span>
                  </span>
                </th>
                <td className={styles.comparisonDataCell}>
                  <ComparisonValue cell={row.free} />
                </td>
                <td
                  className={clsx(
                    styles.comparisonDataCell,
                    styles.comparisonDataCellPro,
                  )}
                >
                  <ComparisonValue cell={row.pro} />
                </td>
                <td className={styles.comparisonDataCell}>
                  <ComparisonValue cell={row.team} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function FaqSection(): ReactNode {
  return (
    <section className={styles.faq} aria-label="Frequently asked questions">
      <Heading as="h2" className={styles.sectionHeading}>
        Frequently asked questions
      </Heading>

      <div className={styles.faqList}>
        {FAQ_ITEMS.map((item) => (
          <details key={item.question} className={styles.faqItem}>
            <summary className={styles.faqQuestion}>{item.question}</summary>
            <p className={styles.faqAnswer}>{item.answer}</p>
          </details>
        ))}
      </div>
    </section>
  );
}

export default function PricingPage(): ReactNode {
  return (
    <Layout
      title="Pricing"
      description="Treq pricing. The desktop app is free. Pro is $15 per user per month with a 14-day trial. Team is $199 per month for 5 members, then $8 per extra member."
    >
      <Head>
        <script type="application/ld+json">
          {JSON.stringify(PRICING_SCHEMA)}
        </script>
      </Head>
      <main className={styles.page}>
        <header className={styles.header}>
          <Heading as="h1" className={styles.title}>
            Pricing
          </Heading>
          <p className={styles.intro}>
            The Treq desktop app is free and open source. Pro and Team add the
            features that run on Treq&apos;s servers.
          </p>
        </header>

        <PlansSection />
        <ComparisonSection />
        <FaqSection />
      </main>
    </Layout>
  );
}
