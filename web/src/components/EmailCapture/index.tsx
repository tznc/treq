import {useEffect, useState, type FormEvent, type ReactNode} from 'react';
import Link from '@docusaurus/Link';
import {supabase} from '@site/src/lib/supabase';
import styles from './styles.module.css';

type Status = 'idle' | 'sending' | 'done' | 'error';

export default function EmailCapture(): ReactNode {
  const [email, setEmail] = useState('');
  const [status, setStatus] = useState<Status>('idle');
  // Before hydration, typed text is lost and a click does a native submit.
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setStatus('sending');
    const {error} = await supabase.rpc('subscribe_newsletter', {
      p_email: email,
      p_source_path: window.location.pathname,
    });
    setStatus(error ? 'error' : 'done');
  };

  return (
    <form className={styles.form} aria-label="Get new guides by email" onSubmit={submit}>
      <p className={styles.title}>Get new guides by email</p>
      {status === 'done' ? (
        <p className={styles.done}>Thanks. New guides will reach your inbox.</p>
      ) : (
        <>
          <div className={styles.row}>
            <label className={styles.label} htmlFor="email-capture">Email address</label>
            <input
              id="email-capture"
              className={styles.input}
              type="email"
              required
              disabled={!ready}
              maxLength={254}
              autoComplete="email"
              placeholder="you@example.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
            <button className="button button--secondary" type="submit" disabled={!ready || status === 'sending'}>
              Subscribe
            </button>
          </div>
          {status === 'error' && (
            <p className={styles.error} role="alert">That did not work. Check the address and try again.</p>
          )}
          <p className={styles.note}>
            New guides on running coding agents in parallel, about once a month. Unsubscribe anytime.{' '}
            <Link to="/privacy">Privacy policy</Link>
          </p>
        </>
      )}
    </form>
  );
}
