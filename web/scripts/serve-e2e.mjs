// Serves the built site for the e2e tests the way Cloudflare Pages does:
// a real file (such as /version) is served as is, and a folder without its
// trailing slash 308s to it with the query string kept. `docusaurus serve`
// instead redirects every slashless path and drops the query.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import handler from 'serve-handler';

const root = path.resolve(import.meta.dirname, '..', 'build');
const port = Number(process.argv[2] ?? 3000);

http
  .createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    const target = path.join(root, decodeURIComponent(url.pathname));
    if (!url.pathname.endsWith('/') && target.startsWith(root) && isDir(target)) {
      res.writeHead(308, {Location: `${url.pathname}/${url.search}`});
      res.end();
      return;
    }
    handler(req, res, {public: root, cleanUrls: true, directoryListing: false});
  })
  .listen(port, () => console.log(`Serving ${root} at http://localhost:${port}`));

function isDir(p) {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}
