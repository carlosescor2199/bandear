// Serves the static website in site/ on 127.0.0.1 for local previews. Node built-ins only;
// development use only — GitHub Pages serves the real site.
//
//   node site/dev/serve.mjs [port]      (port 5300 by default)

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

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

const port = process.argv[2] === undefined ? DEFAULT_PORT : Number(process.argv[2]);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  console.error(`invalid port: ${process.argv[2]}`);
  process.exit(2);
}

// The file a request names, or null when it would leave the served folder.
function fileFor(url) {
  let pathname = decodeURIComponent(new URL(url, "http://localhost").pathname);
  if (pathname.endsWith("/")) {
    pathname += "index.html";
  }
  const file = resolve(ROOT, `.${pathname}`);
  return file === ROOT || file.startsWith(ROOT + sep) ? file : null;
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

const server = createServer(async (request, response) => {
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.writeHead(405, { Allow: "GET, HEAD", "Content-Type": "text/plain" });
    response.end("Method not allowed\n");
    return;
  }
  let file = null;
  try {
    file = fileFor(request.url);
  } catch {
    // A malformed URL falls through to the 404 below.
  }
  try {
    if (file === null || !(await stat(file)).isFile()) {
      throw new Error("not a file");
    }
    await send(response, request.method, 200, file);
  } catch {
    // Mirror GitHub Pages: serve 404.html when it exists, plain text otherwise.
    try {
      await send(response, request.method, 404, resolve(ROOT, "404.html"));
    } catch {
      response.writeHead(404, { "Content-Type": "text/plain" });
      response.end("Not found\n");
    }
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`Serving ${ROOT} on http://127.0.0.1:${port}/`);
});
