// SPDX-License-Identifier: MIT

import assert from "node:assert/strict";
import { test } from "node:test";

import { isLoopbackOrigin, preflight } from "./cors.js";

test("a page served from loopback is a loopback origin", () => {
  for (const origin of ["http://127.0.0.1:5173", "http://localhost:3000", "https://localhost", "http://[::1]:8080"]) {
    assert.equal(isLoopbackOrigin(origin), true, origin);
  }
});

test("anything else is not a loopback origin", () => {
  for (const origin of [
    "https://evil.example",
    "http://localhost.evil.example",
    "http://evil.example:80@localhost",
    "http://localhost@evil.example",
    "http://localhost:3000/path",
    "http://localhost:3000/",
    "ftp://localhost",
    "http://10.0.0.1:3000",
    "http://127.0.0.2:3000",
    "http://0.0.0.0:3000",
    "http://user@localhost:3000",
    "HTTP://LOCALHOST:3000",
    "http://localhost:80",
    "http://localhost:3000?x=1",
    "null",
    "",
    "not a url",
  ]) {
    assert.equal(isLoopbackOrigin(origin), false, origin);
  }
});

test("a preflight from a loopback origin is allowed with the headers it asks for", () => {
  const answer = preflight("OPTIONS", "http://localhost:5173", "content-type, connect-protocol-version");
  assert.equal(answer?.status, 204);
  assert.equal(answer?.headers["access-control-allow-origin"], "http://localhost:5173");
  assert.equal(answer?.headers["access-control-allow-headers"], "content-type, connect-protocol-version");
  assert.equal(answer?.headers["access-control-allow-methods"], "GET, POST, OPTIONS");
  assert.equal(answer?.headers["access-control-max-age"], "600");
  assert.equal(answer?.headers.vary, "origin, access-control-request-headers");
});

test("a preflight that asks for no headers is allowed with none", () => {
  assert.equal(preflight("OPTIONS", "http://127.0.0.1:1", undefined)?.headers["access-control-allow-headers"], "");
});

test("a preflight from another origin is refused with no CORS header", () => {
  const answer = preflight("OPTIONS", "https://evil.example", "content-type");
  assert.equal(answer?.status, 403);
  assert.deepEqual(Object.keys(answer?.headers ?? {}), ["vary"]);
});

test("a request that is not a preflight is left to the service", () => {
  assert.equal(preflight("POST", "http://localhost:5173", undefined), undefined);
  assert.equal(preflight("OPTIONS", undefined, undefined), undefined);
  assert.equal(preflight(undefined, undefined, undefined), undefined);
});
