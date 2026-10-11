import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import {DownloadButton, OtherDownloads} from '@site/src/components/DownloadLinks';
import styles from './styles.module.css';

export default function DownloadCTA({
  title = 'Run parallel agents with Treq',
}: {
  title?: string;
}): ReactNode {
  return (
    <aside className={styles.cta}>
      <p className={styles.title}>{title}</p>
      <p className={styles.body}>
        Free, open source desktop app for Claude, Codex, and Cursor workspaces.
        macOS, Windows, and Linux.
      </p>
      <div className={styles.actions}>
        <DownloadButton location="cta" className="button button--primary" />
        <Link className="button button--secondary" to="/pricing">
          See pricing
        </Link>
      </div>
      <OtherDownloads location="cta" className={styles.other} />
    </aside>
  );
}
