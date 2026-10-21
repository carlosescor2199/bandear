// Serves the static website in site/ on 127.0.0.1 for local previews. Node built-ins only;
// development use only — GitHub Pages serves the real site.
//
//   node site/dev/serve.mjs [port]      (port 5300 by default)

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { resolveRequest } from "./resolve.mjs";

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const DEFAULT_PORT = 5300;
const TYPES = Object.freeze({
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ttf": "font/ttf",
});
const PLAIN = { "Content-Type": "text/plain; charset=utf-8" };

const port = process.argv[2] === undefined ? DEFAULT_PORT : Number(process.argv[2]);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  console.error(`invalid port: ${process.argv[2]}`);
  process.exit(2);
}

async function send(response, method, status, file) {
  const body = await readFile(file);
  response.writeHead(status, {
    "Content-Type": TYPES[extname(file).toLowerCase()] ?? "application/octet-stream",
    "Content-Length": body.length,
    "Cache-Control": "no-store",
  });
  response.end(method === "HEAD" ? undefined : body);
}

async function notFound(response, method) {
  // Mirror GitHub Pages: serve 404.html when it exists, plain text otherwise.
  try {
    await send(response, method, 404, resolve(ROOT, "404.html"));
  } catch {
    response.writeHead(404, PLAIN);
    response.end("Not found\n");
  }
}

const isDirectory = (file) => stat(file).then((info) => info.isDirectory(), () => false);

const server = createServer(async (request, response) => {
  const target = resolveRequest(request.method, request.url, ROOT);
  if (target.status === 405) {
    response.writeHead(405, { ...PLAIN, Allow: "GET, HEAD" });
    response.end("Method not allowed\n");
    return;
  }
  if (target.status === 400) {
    response.writeHead(400, PLAIN);
    response.end("Bad request\n");
    return;
  }
  if (target.status === 404) {
    await notFound(response, request.method);
    return;
  }
  // Like GitHub Pages, a folder named without its trailing slash redirects to the slash form.
  if (!target.directory && (await isDirectory(target.file))) {
    const query = new URL(request.url, "http://localhost").search;
    // One leading slash only, so the Location can never read as another host ("//host").
    const path = `/${target.pathname.replace(/^\/+/, "")}`;
    response.writeHead(301, { ...PLAIN, Location: `${path}/${query}` });
    response.end();
    return;
  }
  try {
    if (!(await stat(target.file)).isFile()) throw new Error("not a file");
    await send(response, request.method, 200, target.file);
  } catch {
    await notFound(response, request.method);
  }
});

server.on("error", (error) => {
  const reasons = {
    EADDRINUSE: `port ${port} is already in use; stop the other server or pass another port`,
    EACCES: `no permission to listen on port ${port}; pick a port above 1024`,
  };
  console.error(`Cannot serve the site: ${reasons[error.code] ?? error.message}`);
  process.exit(1);
});

server.listen(port, "127.0.0.1", () => {
  console.log(`Serving ${ROOT} on http://127.0.0.1:${port}/`);
});
