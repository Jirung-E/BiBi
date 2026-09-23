import {createServer} from 'node:http';
import {readFile, stat} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {snapshot, detail} from './fixture.mjs';

// Serve the production UI against a closed fixture API, never the user's BiBi service.
const build = fileURLToPath(new URL('../../build/', import.meta.url));
const port = Number(process.env.BIBI_UI_TEST_PORT || 44901);
const mime = {'.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.ico': 'image/x-icon', '.woff2': 'font/woff2'};
await stat(path.join(build, 'index.html'));
const server = createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  res.setHeader('Cache-Control', 'no-store');
  const json = (value, status = 200) => {res.writeHead(status, {'Content-Type': 'application/json'});res.end(JSON.stringify(value));};
  if (req.method !== 'GET') return json({error: 'Read-only UI fixture: commands are disabled.'}, 405);
  if (url.pathname === '/__ready') return json({fixture: true});
  if (url.pathname === '/api/snapshot') return json(snapshot);
  if (url.pathname.startsWith('/api/runs/')) {
    const value = detail(decodeURIComponent(url.pathname.slice('/api/runs/'.length)));
    return json(value || {error: 'Unknown fixture run'}, value ? 200 : 404);
  }
  if (url.pathname === '/api/stream') {
    res.writeHead(200, {'Content-Type': 'text/event-stream', Connection: 'keep-alive'});
    res.write(': fixture\n\n');
    const heartbeat = setInterval(() => res.write(': fixture\n\n'), 15_000);
    heartbeat.unref();
    req.on('close', () => clearInterval(heartbeat));
    return;
  }
  if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/auth/')) return json({error: 'Unknown fixture route'}, 404);
  try {
    const relative = decodeURIComponent(url.pathname).replace(/^\/+/, '');
    let file = path.resolve(build, relative || 'index.html');
    if (!file.startsWith(build)) return json({error: 'Invalid path'}, 400);
    if (!path.extname(file)) file = path.join(build, 'index.html');
    const body = await readFile(file);
    res.writeHead(200, {'Content-Type': mime[path.extname(file)] || 'application/octet-stream'});
    res.end(body);
  } catch {json({error: 'Not found'}, 404);}
});
server.listen(port, '127.0.0.1', () => console.log('BiBi UI fixture http://127.0.0.1:' + port));
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => {
  server.closeAllConnections();
  server.close(() => process.exit(0));
});
