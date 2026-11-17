import type {ReactNode} from 'react';
import Layout from '@theme/Layout';
import Link from '@docusaurus/Link';
import DownloadCTA from '@site/src/components/DownloadCTA';
import styles from './styles.module.css';

type Source = {id: string; name: string; url: string};
type SkillSummary = {id: string; name: string; description: string | null; category: string | null; route: string};

export default function SkillAuthorPage({source, skills}: {source: Source; skills: SkillSummary[]}): ReactNode {
  const description = `All ${skills.length} agent skills by ${source.name}, with their SKILL.md files. Browse each one or open it in Treq.`;
  const categories = [...new Set(skills.map((s) => s.category ?? 'Other'))].sort();
  return (
    <Layout title={`${source.name} agent skills`} description={description}>
      <main className={styles.page}>
        <div className={styles.breadcrumbs}>
          <Link to="/skills">Skills Library</Link>
          <span className={styles.sep}>/</span>
          <span>{source.name}</span>
        </div>
        <h1>{source.name} agent skills</h1>
        <p className={styles.lead}>
          {description} Source: <Link href={source.url}>{source.url.replace('https://', '')}</Link>
        </p>
        {categories.map((category) => (
          <section key={category}>
            <h2>{category}</h2>
            <ul className={styles.list}>
              {skills
                .filter((s) => (s.category ?? 'Other') === category)
                .map((skill) => (
                  <li key={skill.id}>
                    <Link to={skill.route} className={styles.name}>{skill.name}</Link>
                    {skill.description && <p className={styles.description}>{skill.description}</p>}
                  </li>
                ))}
            </ul>
          </section>
        ))}
        <DownloadCTA title={`Run ${source.name}'s skills in parallel agent workspaces`} />
      </main>
    </Layout>
  );
}
