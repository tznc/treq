import type { ReactNode } from 'react';
import Link from '@docusaurus/Link';
import CodeBlock from '@theme/CodeBlock';
import { MarkdownPreview } from './MarkdownPreview';
import type { SkillPageProps } from './types';
import { formatBytes } from './utils';
import styles from './styles.module.css';

// The server-rendered part of a skill page: the SKILL.md body when its license
// allows, when the skill runs, how to install it, its files and related skills.
// `children` is the live file browser, placed in the Files section.
export function SkillPageSections({
  skill,
  author,
  authorRoute,
  body,
  licenseUrl,
  manualOnly,
  trigger,
  install,
  related,
  children,
}: SkillPageProps & { children?: ReactNode }) {
  const repoUrl = skill.url.split('/tree/')[0];
  const repoName = repoUrl.replace('https://github.com/', '');
  const links = {
    blobBase: skill.url.replace('/tree/', '/blob/'),
    rawBase: skill.url.replace('https://github.com/', 'https://raw.githubusercontent.com/').replace('/tree/', '/'),
  };
  const totalBytes = skill.files.reduce((sum, file) => sum + file.size, 0);
  const name = skill.name;

  return (
    <>
      {body !== null && (
        <section className={styles.section} aria-labelledby="skill-md">
          <h2 id="skill-md">SKILL.md</h2>
          <p className={styles.credit} data-testid="skill-credit">
            Written by {author} and published in{' '}
            <a href={repoUrl} target="_blank" rel="noopener noreferrer">{repoName}</a> under the{' '}
            {licenseUrl ? (
              <a href={licenseUrl} target="_blank" rel="noopener noreferrer">{skill.license} license</a>
            ) : (
              `${skill.license} license`
            )}
            . Treq shows it unchanged except for heading levels and links to files on GitHub.
          </p>
          <div className={styles.bodyPanel} data-testid="skill-body">
            <MarkdownPreview content={body} links={links} />
          </div>
        </section>
      )}

      {!skill.proprietary && (
        <section className={styles.section} aria-labelledby="when-it-runs">
          <h2 id="when-it-runs">When {name} runs</h2>
          {manualOnly ? (
            <p>
              In Claude Code, {name} runs only when you type <code>/{name}</code>. Its frontmatter sets{' '}
              <code>disable-model-invocation: true</code>, which keeps the description out of Claude's context. In
              Codex, type <code>${name}</code> to run it.
            </p>
          ) : (
            <p>
              Claude Code and Codex keep each installed skill's name and description in context. When a request
              matches the description, the agent loads the full SKILL.md. You can also run it yourself: type{' '}
              <code>/{name}</code> in Claude Code or <code>${name}</code> in Codex.
            </p>
          )}
          {trigger && (
            <>
              <p>The description says when to use it:</p>
              <blockquote>{trigger}</blockquote>
            </>
          )}
        </section>
      )}

      {!skill.proprietary && install && (
        <section className={styles.section} aria-labelledby="install">
          <h2 id="install">Install {name}</h2>
          <h3>Claude Code</h3>
          <p>
            Copy the <code>{install.folder}</code> folder into <code>.claude/skills/</code> in your project. To use it
            in every project, copy it into <code>~/.claude/skills/</code> instead.
          </p>
          <CodeBlock language="bash">{install.claudeCode}</CodeBlock>
          <h3>Codex</h3>
          <p>
            Codex reads skills from <code>.agents/skills/</code> in your repository, and from{' '}
            <code>~/.agents/skills/</code> for every repository. It detects new skills on its own. Restart Codex if{' '}
            {name} does not appear.
          </p>
          <CodeBlock language="bash">{install.codex}</CodeBlock>
          <h3>Treq</h3>
          <p>
            Open <strong>Settings</strong> and select the <strong>Skills</strong> tab. The tab appears when{' '}
            <strong>Skills installation</strong> is on under Settings → Feature Preview. Search for {name} and choose{' '}
            <strong>Install…</strong>.
          </p>
          <p>
            <strong>Install for repository</strong> writes the skill under <code>.treq/skills/</code> in the current
            repository. <strong>Install for application</strong> makes it available to every repository on this
            machine. Each new workspace gets a copy in <code>.claude/skills/</code> and <code>.agents/skills/</code>, so
            Claude Code and Codex both find it. See{' '}
            <Link to="/docs/how-to/installing-skills">Installing Skills from the Library</Link>.
          </p>
        </section>
      )}

      {!skill.proprietary && (
        <section className={styles.section} aria-labelledby="files">
          <h2 id="files">Files</h2>
          <p>
            {body !== null && skill.files.length === 1
              ? `${name} ships one file, SKILL.md, shown above.`
              : `${name} ships ${skill.files.length} ${skill.files.length === 1 ? 'file' : 'files'}, ${formatBytes(totalBytes)} in total. Select a file to load it from GitHub.`}
          </p>
          {children}
        </section>
      )}

      {related.length > 0 && (
        <section className={styles.section} aria-labelledby="related">
          <h2 id="related">Related skills</h2>
          <ul className={styles.related}>
            {related.map((item) => (
              <li key={item.route}>
                <Link to={item.route} className={styles.relatedName}>{item.name}</Link>
                <span className={styles.relatedAuthor}> by {item.author}</span>
                {item.description && <p className={styles.relatedDesc}>{item.description}</p>}
              </li>
            ))}
          </ul>
          {authorRoute && (
            <p>
              <Link to={authorRoute}>All skills by {author}</Link>
            </p>
          )}
        </section>
      )}
    </>
  );
}
