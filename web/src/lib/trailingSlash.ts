/**
 * The site builds with `trailingSlash: true`, so /docs/cli 308-redirects to
 * /docs/cli/. Docusaurus permalinks come without the slash, and a URL in
 * structured data that redirects shows up in Search Console as "Page with
 * redirect". File URLs (with an extension) are served as-is.
 */
export function withTrailingSlash(url: string): string {
  const match = url.match(/^((?:https?:\/\/[^/?#]+)?)([^?#]*)(.*)$/) as RegExpMatchArray;
  const [, origin, path, rest] = match;
  if (path.endsWith('/') || /\.[a-z0-9]+$/i.test(path.split('/').pop() ?? '')) {
    return url;
  }
  return `${origin}${path}/${rest}`;
}
