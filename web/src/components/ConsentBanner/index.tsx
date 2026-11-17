import {useEffect, useState, type ReactNode} from 'react';
import Link from '@docusaurus/Link';
import {
  OPEN_CONSENT_EVENT,
  readConsent,
  saveConsent,
  shouldAskForConsent,
  type ConsentChoice,
} from '@site/src/lib/consent';
import styles from './styles.module.css';

export default function ConsentBanner(): ReactNode {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (typeof (window as {gtag?: unknown}).gtag !== 'function') return undefined;
    const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone ?? '';
    setOpen(shouldAskForConsent(timeZone, readConsent()));
    const reopen = () => setOpen(true);
    window.addEventListener(OPEN_CONSENT_EVENT, reopen);
    return () => window.removeEventListener(OPEN_CONSENT_EVENT, reopen);
  }, []);

  if (!open) return null;
  const choose = (choice: ConsentChoice) => {
    saveConsent(choice);
    setOpen(false);
  };
  return (
    <div className={styles.banner} role="dialog" aria-label="Analytics cookies">
      <p className={styles.text}>
        We use analytics cookies to improve your experience on this site.{' '}
        <Link to="/privacy">Privacy policy</Link>
      </p>
      <div className={styles.actions}>
        <button type="button" className="button button--secondary button--sm" onClick={() => choose('denied')}>
          Reject
        </button>
        <button type="button" className="button button--primary button--sm" onClick={() => choose('granted')}>
          Accept
        </button>
      </div>
    </div>
  );
}

export function ConsentSettingsButton(): ReactNode {
  return (
    <button
      type="button"
      className="button button--secondary button--sm"
      onClick={() => window.dispatchEvent(new Event(OPEN_CONSENT_EVENT))}
    >
      Change analytics cookie choice
    </button>
  );
}
