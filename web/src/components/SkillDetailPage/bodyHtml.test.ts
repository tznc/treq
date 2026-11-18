import { prepareBodyHtml } from './bodyHtml';

const bases = {
  blobBase: 'https://github.com/o/r/blob/main/skills/x',
  rawBase: 'https://raw.githubusercontent.com/o/r/main/skills/x',
};

test('demotes headings one level so the page keeps a single h1', () => {
  expect(prepareBodyHtml('<h1 id="a">A</h1><h2>B</h2><h6>F</h6>', bases)).toBe(
    '<h2 id="a">A</h2><h3>B</h3><h6>F</h6>',
  );
});

test('points relative links at GitHub and relative images at the raw CDN', () => {
  expect(
    prepareBodyHtml('<a href="references/api.md">api</a> <a href="../y/SKILL.md">y</a> <img src="./img/a.png" alt="">', bases),
  ).toBe(
    '<a href="https://github.com/o/r/blob/main/skills/x/references/api.md">api</a> ' +
      '<a href="https://github.com/o/r/blob/main/skills/y/SKILL.md">y</a> ' +
      '<img src="https://raw.githubusercontent.com/o/r/main/skills/x/img/a.png" alt="">',
  );
});

test('leaves absolute, anchor and mail links alone', () => {
  const html = '<a href="https://example.com">e</a><a href="#user-content-x">x</a><a href="mailto:a@b.c">m</a>';
  expect(prepareBodyHtml(html, bases)).toBe(html);
});
