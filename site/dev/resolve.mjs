// How the preview server answers a request, decided without touching the disk so node tests can
// check it (tests/serve.test.mjs). serve.mjs does the file system part.

import { resolve, sep } from "node:path";

// Returns one of:
//   { status: 405 }                       a method other than GET or HEAD
//   { status: 400 }                       a malformed URL or percent-encoding
//   { status: 404 }                       a path that would leave `root`
//   { file, pathname, directory }         the file to look for; `directory` is true when the
//                                         path ends in "/" and `file` is its index.html
// `pathname` is the URL's still-encoded path, safe to send back in a Location header.
export function resolveRequest(method, url, root) {
  if (method !== "GET" && method !== "HEAD") return { status: 405 };
  let pathname;
  let decoded;
  try {
    pathname = new URL(url, "http://localhost").pathname;
    decoded = decodeURIComponent(pathname);
  } catch {
    return { status: 400 };
  }
  if (decoded.includes("\0")) return { status: 400 };
  const directory = decoded.endsWith("/");
  const file = resolve(root, `.${directory ? `${decoded}index.html` : decoded}`);
  if (file !== root && !file.startsWith(root + sep)) return { status: 404 };
  return { file, pathname, directory };
}
