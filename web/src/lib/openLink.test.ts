import {describe, expect, it} from '@jest/globals';
import {parsePrPath, prDeepLink, prGithubUrl} from './openLink';

describe('parsePrPath', () => {
  it('reads /open/pr/{owner}/{repo}/{n}', () => {
    expect(parsePrPath('/open/pr/tznc/treq/42')).toEqual({owner: 'tznc', repo: 'treq', number: 42});
    expect(parsePrPath('/open/pr/my-org/my.repo_x/7/')).toEqual({owner: 'my-org', repo: 'my.repo_x', number: 7});
  });

  it('rejects anything else', () => {
    for (const path of [
      '/open/pr/',
      '/open/pr/tznc/treq',
      '/open/pr/tznc/treq/abc',
      '/open/pr/tznc/treq/0',
      '/open/pr/tznc/treq/42/extra',
      '/open/pr/../treq/42',
      '/open/pr/a%2Fb/treq/42',
      '/other/tznc/treq/42',
    ]) {
      expect(parsePrPath(path)).toBeNull();
    }
  });
});

describe('PR links', () => {
  const pr = {owner: 'tznc', repo: 'treq', number: 42};
  it('builds the treq:// deep link', () => {
    expect(prDeepLink(pr)).toBe('treq://pr/open?repo=tznc%2Ftreq&number=42');
  });
  it('builds the GitHub URL', () => {
    expect(prGithubUrl(pr)).toBe('https://github.com/tznc/treq/pull/42');
  });
});
