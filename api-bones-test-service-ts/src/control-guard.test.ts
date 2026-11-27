// SPDX-License-Identifier: MIT

import assert from "node:assert/strict";
import { test } from "node:test";

import { refuse } from "./control-guard.js";

const host = "127.0.0.1:4000";

test("a loopback json request is served", () => {
  assert.equal(refuse({ method: "POST", headers: { host, "content-type": "application/json" } }, 4000), undefined);
  assert.equal(refuse({ method: "POST", headers: { host, "content-type": "application/json; charset=utf-8" } }, 4000), undefined);
  assert.equal(refuse({ method: "GET", headers: { host } }, 4000), undefined);
});

test("a wrong Host is refused with 403", () => {
  for (const wrong of [undefined, "localhost:4000", "evil.example:4000", "127.0.0.1:4001", "127.0.0.1"]) {
    const refusal = refuse({ method: "GET", headers: { host: wrong } }, 4000);
    assert.equal(refusal?.status, 403, String(wrong));
  }
});

test("a POST that is not json is refused with 415", () => {
  for (const type of [undefined, "text/plain", "text/plain;charset=UTF-8", "application/x-www-form-urlencoded", "multipart/form-data"]) {
    const refusal = refuse({ method: "POST", headers: { host, "content-type": type } }, 4000);
    assert.equal(refusal?.status, 415, String(type));
  }
});
