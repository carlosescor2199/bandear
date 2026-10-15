// Serves apps/desktop/ui on 127.0.0.1 so the console and the component gallery can be previewed in
// a browser: ES modules do not load from file://. Node built-ins only; development use only.
//
//   node apps/desktop/ui/dev/serve.mjs [port]      (port 5199 by default)
//
// Then open http://127.0.0.1:5199/?fake (the console on a simulated engine) or
// http://127.0.0.1:5199/dev/gallery.html (the components).

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const DEFAULT_PORT = 5199;
const TYPES = Object.freeze({
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".ttf": "font/ttf",
  ".woff2": "font/woff2",
});

const port = process.argv[2] === undefined ? DEFAULT_PORT : Number(process.argv[2]);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  console.error(`invalid port: ${process.argv[2]}`);
  process.exit(2);
}

// The file a request names, or null when it would leave the served folder.
function fileFor(url) {
  let pathname = decodeURIComponent(new URL(url, "http://localhost").pathname);
  if (pathname === "/") {
    pathname = "/index.html";
  }
  const file = resolve(ROOT, `.${pathname}`);
  return file === ROOT || file.startsWith(ROOT + sep) ? file : null;
}

function reply(response, status, body) {
  response.writeHead(status, { "Content-Type": "text/plain; charset=utf-8" });
  response.end(`${body}\n`);
}

const server = createServer(async (request, response) => {
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.setHeader("Allow", "GET, HEAD");
    reply(response, 405, "Method not allowed");
    return;
  }
  let file;
  try {
    file = fileFor(request.url);
  } catch {
    reply(response, 400, "Bad request");
    return;
  }
  if (file === null) {
    reply(response, 403, "Forbidden");
    return;
  }
  try {
    if (!(await stat(file)).isFile()) {
      throw new Error("not a file");
    }
    const body = await readFile(file);
    response.writeHead(200, {
      "Content-Type": TYPES[extname(file).toLowerCase()] ?? "application/octet-stream",
      "Content-Length": body.length,
      "Cache-Control": "no-store",
    });
    response.end(request.method === "HEAD" ? undefined : body);
  } catch {
    reply(response, 404, "Not found");
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`Serving ${ROOT}`);
  console.log(`  http://127.0.0.1:${port}/?fake`);
  console.log(`  http://127.0.0.1:${port}/dev/gallery.html`);
});
