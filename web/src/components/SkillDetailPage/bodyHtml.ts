// Adjusts sanitized SKILL.md HTML for display on a skill page. The page title
// is the only h1, so body headings move down one level. Relative links and
// images resolve against the skill folder on GitHub, because the files they
// point to are not hosted on treq.dev.
const ABSOLUTE = /^(?:[a-z][a-z0-9+.-]*:|#|\/)/i;

function resolve(url: string, base: string): string {
  if (ABSOLUTE.test(url)) return url;
  return new URL(url, `${base}/`).href;
}

export function prepareBodyHtml(html: string, { blobBase, rawBase }: { blobBase: string; rawBase: string }): string {
  return html
    .replace(/<(\/?)h([1-5])\b/g, (_, slash: string, level: string) => `<${slash}h${Number(level) + 1}`)
    .replace(/(<a\b[^>]*?\shref=")([^"]*)"/g, (_, start: string, url: string) => `${start}${resolve(url, blobBase)}"`)
    .replace(/(<img\b[^>]*?\ssrc=")([^"]*)"/g, (_, start: string, url: string) => `${start}${resolve(url, rawBase)}"`);
}
