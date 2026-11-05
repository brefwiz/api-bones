// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_stream_keepalive.feature`.
 *
 * The reader is a server-stream call through the shipped
 * `configureConnectTransport`. The scripted server answers it as a Connect
 * server stream on a protobuf codec: after each full interval of silence it
 * sends one keepalive, an ordinary data envelope carrying the keepalive
 * payload, and it records the headers the request carried.
 */

import assert from "node:assert/strict";

import { create, toBinary } from "@bufbuild/protobuf";
import { StringValueSchema, type StringValue } from "@bufbuild/protobuf/wkt";
import { Given, Then, When, World } from "@cucumber/cucumber";

import {
  STREAM_KEEPALIVE_FIELD_NUMBER,
  STREAM_KEEPALIVE_HEADER,
  STREAM_KEEPALIVE_INTERVAL,
  STREAM_KEEPALIVE_PAYLOAD,
  isKeepalivePayload,
} from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

/** What the scripted server sends, in order: a data payload or a stream failure. */
type Scripted = { data: Uint8Array } | { failure: string };

interface KeepaliveWorld extends World {
  payload: Uint8Array;
  fields?: { no: number; wireType: number; value: number }[];
  verdict?: boolean;
  script: Scripted[];
  keepalives: number;
  received: string[];
  failure?: string;
  requestHeaders?: Headers;
  exhausted?: boolean;
}

const hex = (text: string): Uint8Array => {
  assert.equal(text.length % 2, 0, `odd hex length in ${text}`);
  return Uint8Array.from(text.match(/../g) ?? [], (pair) => parseInt(pair, 16));
};

const toHex = (bytes: Uint8Array): string =>
  [...bytes].map((byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join("");

const dataEvent = (id: string): Scripted => ({
  data: toBinary(StringValueSchema, create(StringValueSchema, { value: id })),
});

const world = (self: World): KeepaliveWorld => {
  const w = self as KeepaliveWorld;
  w.script ??= [];
  w.keepalives ??= 0;
  w.received ??= [];
  return w;
};

function silence(w: KeepaliveWorld, millis: number): void {
  const sent = Math.floor(millis / STREAM_KEEPALIVE_INTERVAL);
  for (let at = 0; at < sent; at += 1) {
    w.script.push({ data: STREAM_KEEPALIVE_PAYLOAD });
    w.keepalives += 1;
  }
}

/** Read a protobuf varint-only message as its fields. */
function readVarintFields(bytes: Uint8Array): { no: number; wireType: number; value: number }[] {
  const fields: { no: number; wireType: number; value: number }[] = [];
  let at = 0;
  const varint = (): number => {
    let out = 0;
    for (let shift = 0; ; shift += 7) {
      const byte = bytes[at++] as number;
      out += (byte & 0x7f) * 2 ** shift;
      if ((byte & 0x80) === 0) return out;
    }
  };
  while (at < bytes.length) {
    const tag = varint();
    const wireType = tag % 8;
    assert.equal(wireType, 0, "the payload holds a field that is not a varint");
    fields.push({ no: Math.floor(tag / 8), wireType, value: varint() });
  }
  return fields;
}

const envelope = (flags: number, payload: Uint8Array): Uint8Array => {
  const out = new Uint8Array(5 + payload.length);
  out[0] = flags;
  new DataView(out.buffer).setUint32(1, payload.length);
  out.set(payload, 5);
  return out;
};

/** The server: a Connect server stream on a protobuf codec, answered from the script. */
function scriptedServer(w: KeepaliveWorld): typeof fetch {
  return async (_input, init) => {
    w.requestHeaders = new Headers(init?.headers);
    const text = new TextEncoder();
    const parts: Uint8Array[] = [];
    let failed = false;
    for (const item of w.script) {
      if ("data" in item) parts.push(envelope(0, item.data));
      else {
        failed = true;
        parts.push(
          envelope(2, text.encode(JSON.stringify({ error: { code: "unavailable", message: item.failure } }))),
        );
      }
    }
    if (!failed) parts.push(envelope(2, text.encode("{}")));
    return new Response(new Blob(parts as BlobPart[]).stream(), {
      status: 200,
      headers: { "content-type": "application/connect+proto" },
    });
  };
}

const method = {
  kind: "rpc",
  name: "Watch",
  localName: "watch",
  parent: { typeName: "pkg.v1.Events" },
  methodKind: "server_streaming",
  input: StringValueSchema,
  output: StringValueSchema,
  idempotency: 0,
  deprecated: false,
} as never;

function readerTransport(w: KeepaliveWorld) {
  return configureConnectTransport({
    baseUrl: "https://svc",
    profile: "service",
    policy: { schemaVersion: 1, methods: [] },
    fetch: scriptedServer(w),
  });
}

async function* request(): AsyncGenerator<{ value: string }> {
  yield { value: "watch" };
}

Given("the keepalive payload", function (this: World) {
  world(this).payload = STREAM_KEEPALIVE_PAYLOAD;
});

Given("the payload bytes {string}", function (this: World, bytes: string) {
  world(this).payload = hex(bytes);
});

When("the payload is read as protobuf wire data", function (this: KeepaliveWorld) {
  this.fields = readVarintFields(this.payload);
  const verdict = isKeepalivePayload(this.payload);
  this.verdict = verdict;
});

When("the payload is checked for a keepalive", function (this: KeepaliveWorld) {
  const verdict = isKeepalivePayload(this.payload);
  this.verdict = verdict;
});

Then("the payload is the bytes {string}", function (this: KeepaliveWorld, expected: string) {
  assert.equal(toHex(this.payload), expected);
});

Then(
  "the payload holds one field, number {int}, as the varint {int}",
  function (this: KeepaliveWorld, number: number, value: number) {
    assert.equal(this.verdict, true);
    const fields = this.fields ?? assert.fail("payload not read");
    assert.equal(fields.length, 1);
    assert.equal(fields[0]?.no, number);
    assert.equal(number, STREAM_KEEPALIVE_FIELD_NUMBER);
    assert.equal(fields[0]?.value, value);
  },
);

Then("the payload is a keepalive: {word}", function (this: KeepaliveWorld, expected: string) {
  assert.equal(String(this.verdict), expected);
});

Given(
  "a server stream that delivers the event {string}, stays silent for {int} ms and delivers the event {string}",
  function (this: World, first: string, millis: number, last: string) {
    const w = world(this);
    w.script.push(dataEvent(first));
    silence(w, millis);
    w.script.push(dataEvent(last));
  },
);

Given(
  "a server stream that delivers the event {string}, stays silent for {int} ms and fails with {string}",
  function (this: World, first: string, millis: number, failure: string) {
    const w = world(this);
    w.script.push(dataEvent(first));
    silence(w, millis);
    w.script.push({ failure });
  },
);

Given(
  "a server stream that delivers the event {string} also carrying field 19999 as the varint 1",
  function (this: World, id: string) {
    const w = world(this);
    const event = dataEvent(id) as { data: Uint8Array };
    w.script.push({ data: new Uint8Array([...event.data, ...STREAM_KEEPALIVE_PAYLOAD]) });
  },
);

async function readMessages(stream: AsyncIterable<StringValue>): Promise<{ received: string[]; failure?: string }> {
  const received: string[] = [];
  try {
    for await (const message of stream) {
      assert.ok(
        message.value !== "" || message.$unknown === undefined,
        "the reader was handed a message with no event",
      );
      received.push(message.value);
    }
  } catch (error) {
    return { received, failure: (error as { rawMessage?: string }).rawMessage ?? String(error) };
  }
  return { received };
}

When("the reader reads the stream", async function (this: KeepaliveWorld) {
  const w = world(this);
  const res = await readerTransport(w).stream(method, undefined, undefined, undefined, request());
  const outcome = await readMessages(res.message as AsyncIterable<StringValue>);
  this.received = outcome.received;
  this.failure = outcome.failure;
});

When("the reader prepares a server-stream request", async function (this: KeepaliveWorld) {
  const w = world(this);
  w.script = [];
  const res = await readerTransport(w).stream(method, undefined, undefined, undefined, request());
  for await (const message of res.message as AsyncIterable<StringValue>) void message;
  this.exhausted = (await (res.message as AsyncIterable<StringValue>)[Symbol.asyncIterator]().next()).done;
});

Then(
  /^the server sent (\d+) keepalives? while the stream was silent$/,
  function (this: World, expected: string) {
    assert.equal(world(this).keepalives, Number(expected));
  },
);

Then(
  "the reader received the events {string} and nothing else",
  function (this: KeepaliveWorld, expected: string) {
    assert.equal(this.received.join(","), expected);
  },
);

Then("the reader read the failure {string}", function (this: World, expected: string) {
  assert.equal(world(this).failure, expected);
});

Then(
  "the request carries the header {string} set to {string}",
  function (this: KeepaliveWorld, name: string, value: string) {
    assert.equal(this.exhausted, true, "the empty response stream finished");
    assert.equal(name, STREAM_KEEPALIVE_HEADER);
    const headers = world(this).requestHeaders ?? assert.fail("no request prepared");
    assert.equal(headers.get(name), value);
  },
);

