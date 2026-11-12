import React, { useEffect, useMemo, useState } from 'react';
import Layout from '@theme/Layout';
import Head from '@docusaurus/Head';
import Link from '@docusaurus/Link';
import catalog from '@site/src/data/skills.json';
import { ProviderFilter } from './_ProviderFilter';
import styles from './index.module.css';

type Skill = {
  id: string;
  name: string;
  description: string | null;
  source: string;
  category: string | null;
  license: string | null;
  proprietary?: boolean;
  path: string;
  url: string;
  route: string;
};

type Source = {
  id: string;
  name: string;
  url: string;
  skillCount: number;
  stars: number | null;
};

const skills = catalog.skills as Skill[];
const sources = catalog.sources as Source[];
const sourceById = new Map(sources.map((s) => [s.id, s]));

// Providers ranked by GitHub stars, for the right-hand aside.
const rankedSources = [...sources].sort((a, b) => (b.stars ?? 0) - (a.stars ?? 0));

const SKILLS_SCHEMA = {
  '@context': 'https://schema.org',
  '@type': 'CollectionPage',
  name: 'Skills Library',
  description: 'A searchable catalog of Agent Skills curated from Anthropic, OpenAI, Microsoft, Vercel, and other trusted sources.',
  url: 'https://treq.dev/skills',
  publisher: {
    '@type': 'Organization',
    name: 'Treq',
    url: 'https://treq.dev',
  },
};

function matches(skill: Skill, query: string): boolean {
  if (!query) return true;
  const haystack = `${skill.name} ${skill.description ?? ''} ${skill.category ?? ''}`.toLowerCase();
  return haystack.includes(query.toLowerCase());
}

function formatStars(n: number | null): string {
  if (n === null) return '—';
  if (n >= 1000) return `${(n / 1000).toFixed(1).replace(/\.0$/, '')}k`;
  return String(n);
}

export default function SkillsPage() {
  const [query, setQuery] = useState('');
  const [selectedProviders, setSelectedProviders] = useState<Set<string>>(new Set());
  const [categoryFilter, setCategoryFilter] = useState('all');

  // Deep-link support: /skills?q=<name> (used by the sitewide search index)
  // and /skills?provider=<source> (the desktop app's author links).
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const q = params.get('q');
    if (q) setQuery(q);
    const provider = params.get('provider');
    if (provider) setSelectedProviders(new Set([provider]));
  }, []);

  const providerMatch = (skill: Skill) =>
    selectedProviders.size === 0 || selectedProviders.has(skill.source);

  const categories = useMemo(() => {
    const scoped = skills.filter(providerMatch);
    return Array.from(new Set(scoped.map((s) => s.category).filter((c): c is string => !!c))).sort();
  }, [selectedProviders]);

  // Keep the category filter valid when the provider selection changes.
  useEffect(() => {
    if (categoryFilter !== 'all' && !categories.includes(categoryFilter)) {
      setCategoryFilter('all');
    }
  }, [categories, categoryFilter]);

  const filtered = useMemo(() => {
    return skills.filter((s) => {
      if (!providerMatch(s)) return false;
      if (categoryFilter !== 'all' && s.category !== categoryFilter) return false;
      return matches(s, query);
    });
  }, [query, selectedProviders, categoryFilter]);

  return (
    <Layout
      title="Skills Library"
      description="A searchable catalog of Agent Skills curated from Anthropic, OpenAI, Microsoft, Vercel, and other trusted sources."
    >
      <Head>
        <script type="application/ld+json">{JSON.stringify(SKILLS_SCHEMA)}</script>
      </Head>
      <div className={styles.page}>
        <div className={styles.header}>
          <h1 className={styles.title}>Skills Library</h1>
          <p className={styles.subtitle}>
            A searchable catalog of{' '}
            <a href="https://github.com/anthropics/skills" target="_blank" rel="noopener noreferrer">
              Agent Skills
            </a>{' '}
            curated from trusted sources.
          </p>
        </div>

        <div className={styles.layout}>
          <div className={styles.main}>
            <div className={styles.controls}>
              <input
                type="search"
                className={styles.search}
                placeholder="Search skills by name or description…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                aria-label="Search skills"
              />
              <div className={styles.filterRow}>
                <ProviderFilter
                  providers={sources}
                  selected={selectedProviders}
                  onChange={setSelectedProviders}
                />
                {categories.length > 0 && (
                  <select
                    className={styles.categorySelect}
                    value={categoryFilter}
                    onChange={(e) => setCategoryFilter(e.target.value)}
                    aria-label="Filter by category"
                  >
                    <option value="all">All categories</option>
                    {categories.map((c) => (
                      <option key={c} value={c}>{c}</option>
                    ))}
                  </select>
                )}
              </div>
            </div>

            <p className={styles.resultCount}>
              {filtered.length} of {skills.length} skills
            </p>

            {filtered.length === 0 ? (
              <p className={styles.empty}>No skills match your search.</p>
            ) : (
              <div className={styles.grid}>
                {filtered.map((skill) => (
                  <Link key={skill.id} to={skill.route} className={styles.card}>
                    <div className={styles.cardHead}>
                      <h2 className={styles.cardTitle}>{skill.name}</h2>
                      <span className={styles.sourceBadge} data-source={skill.source}>
                        {sourceById.get(skill.source)?.name ?? skill.source}
                      </span>
                    </div>
                    {skill.proprietary ? (
                      <p className={styles.cardNotice}>
                        Proprietary license — description &amp; preview unavailable. View on GitHub.
                      </p>
                    ) : (
                      <p className={styles.cardDesc}>{skill.description}</p>
                    )}
                    <div className={styles.tags}>
                      {skill.category && <span className={styles.tag}>{skill.category}</span>}
                      {skill.license && <span className={styles.tag}>{skill.license}</span>}
                    </div>
                  </Link>
                ))}
              </div>
            )}
          </div>

          <aside className={styles.aside} aria-label="Source repositories by stars">
            <h2 className={styles.asideTitle}>Source repos</h2>
            <p className={styles.asideSub}>Ranked by GitHub stars</p>
            <ol className={styles.repoList}>
              {rankedSources.map((s) => {
                const active = selectedProviders.has(s.id);
                return (
                  <li key={s.id} className={styles.repoItem}>
                    <button
                      type="button"
                      className={active ? styles.repoToggleActive : styles.repoToggle}
                      aria-pressed={active}
                      onClick={() => {
                        const next = new Set(selectedProviders);
                        if (next.has(s.id)) next.delete(s.id);
                        else next.add(s.id);
                        setSelectedProviders(next);
                      }}
                    >
                      <span className={styles.repoName}>{s.name}</span>
                      <span className={styles.repoStars}>★ {formatStars(s.stars)}</span>
                    </button>
                    <a
                      className={styles.repoLink}
                      href={s.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      aria-label={`${s.name} on GitHub`}
                    >
                      ↗
                    </a>
                  </li>
                );
              })}
            </ol>
          </aside>
        </div>
      </div>
    </Layout>
  );
}
