import {describe, expect, it} from '@jest/globals';
import {withTrailingSlash} from './trailingSlash';

describe('withTrailingSlash', () => {
  it('adds the slash Docusaurus serves pages at (trailingSlash: true)', () => {
    expect(withTrailingSlash('https://treq.dev/learn/concepts/git/merge-vs-rebase')).toBe(
      'https://treq.dev/learn/concepts/git/merge-vs-rebase/',
    );
    expect(withTrailingSlash('/docs/security-and-privacy')).toBe('/docs/security-and-privacy/');
  });

  it('keeps URLs that already end in a slash, and the site root', () => {
    expect(withTrailingSlash('https://treq.dev/learn/')).toBe('https://treq.dev/learn/');
    expect(withTrailingSlash('https://treq.dev')).toBe('https://treq.dev/');
  });

  it('puts the slash before a query or hash', () => {
    expect(withTrailingSlash('https://treq.dev/docs/cli?x=1#flags')).toBe(
      'https://treq.dev/docs/cli/?x=1#flags',
    );
  });

  it('leaves file URLs alone', () => {
    expect(withTrailingSlash('https://treq.dev/img/og.png')).toBe('https://treq.dev/img/og.png');
    expect(withTrailingSlash('https://treq.dev/llms.txt')).toBe('https://treq.dev/llms.txt');
  });
});
