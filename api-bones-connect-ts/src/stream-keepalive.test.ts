// SPDX-License-Identifier: MIT
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { StringValueSchema, type StringValue } from "@bufbuild/protobuf/wkt";
import { Code, ConnectError, type Transport } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";

import {
  STREAM_KEEPALIVE_FIELD_NUMBER,
  STREAM_KEEPALIVE_HEADER,
  STREAM_KEEPALIVE_HEADER_VALUE,
  STREAM_KEEPALIVE_INTERVAL,
  STREAM_KEEPALIVE_PAYLOAD,
  isKeepaliveMessage,
  isKeepalivePayload,
  makeStreamKeepaliveInterceptor,
} from "./stream-keepalive.js";
import { configureConnectTransport } from "./web.js";
import { rotatingStream } from "./stream-rotation.js";

const keepaliveMessage = (): StringValue => fromBinary(StringValueSchema, STREAM_KEEPALIVE_PAYLOAD);
const event = (value: string): StringValue => create(StringValueSchema, { value });

describe("keepalive payload", () => {
  it("is field 19999 as the varint 1", () => {
    expect([...STREAM_KEEPALIVE_PAYLOAD]).toEqual([0xf8, 0xe1, 0x09, 0x01]);
    expect(STREAM_KEEPALIVE_FIELD_NUMBER).toBe(19_999);
    expect(STREAM_KEEPALIVE_HEADER).toBe("bones-stream-keepalive");
    expect(STREAM_KEEPALIVE_HEADER_VALUE).toBe("1");
    expect(STREAM_KEEPALIVE_INTERVAL).toBe(15_000);
  });

  it.each([
    [[0xf8, 0xe1, 0x09, 0x01], true],
    [[], false],
    [[0xf8, 0xe1, 0x09], false],
    [[0xf8, 0xe1, 0x09, 0x02], false],
    [[0xf8, 0xe1, 0x09, 0x00], false],
    [[0xf8, 0xe1, 0x09, 0x01, 0x00], false],
    [[0x0a, 0x02, 0x65, 0x2d, 0x31], false],
  ])("%j is a keepalive: %s", (bytes, verdict) => {
    expect(isKeepalivePayload(new Uint8Array(bytes))).toBe(verdict);
  });
});

describe("isKeepaliveMessage", () => {
  it("recognises a keepalive decoded as a response type", () => {
    expect(isKeepaliveMessage(StringValueSchema, keepaliveMessage())).toBe(true);
  });

  it("keeps an empty message and an ordinary one", () => {
    expect(isKeepaliveMessage(StringValueSchema, create(StringValueSchema))).toBe(false);
    expect(isKeepaliveMessage(StringValueSchema, event("e-1"))).toBe(false);
  });

  it("keeps a message with field 19999 next to other content", () => {
    const bytes = new Uint8Array([...toBinary(StringValueSchema, event("e-1")), ...STREAM_KEEPALIVE_PAYLOAD]);
    expect(isKeepaliveMessage(StringValueSchema, fromBinary(StringValueSchema, bytes))).toBe(false);
  });

  it("keeps field 19999 carrying another value", () => {
    const other = fromBinary(StringValueSchema, new Uint8Array([0xf8, 0xe1, 0x09, 0x02]));
    expect(isKeepaliveMessage(StringValueSchema, other)).toBe(false);
  });
});

async function* script(items: readonly (StringValue | Error)[]): AsyncGenerator<StringValue> {
  for (const item of items) {
    if (item instanceof Error) throw item;
    yield item;
  }
}

function run(stream: boolean, items: readonly (StringValue | Error)[]) {
  const seen = { header: null as string | null };
  const next = async (req: { header: Headers }) => {
    seen.header = req.header.get(STREAM_KEEPALIVE_HEADER);
    return { stream, message: script(items) } as never;
  };
  const call = makeStreamKeepaliveInterceptor()(next as never);
  return {
    seen,
    call: (): Promise<{ message: unknown }> =>
      call({ stream, header: new Headers(), method: { output: StringValueSchema } } as never) as never,
  };
}

async function read(res: { message: unknown }): Promise<(string | Error)[]> {
  const got: (string | Error)[] = [];
  try {
    for await (const message of res.message as AsyncIterable<StringValue>) got.push(message.value);
  } catch (error) {
    got.push(error as Error);
  }
  return got;
}

describe("makeStreamKeepaliveInterceptor", () => {
  it("sets the header on a server-stream request only", async () => {
    const streamed = run(true, []);
    await streamed.call();
    expect(streamed.seen.header).toBe("1");
    const unary = run(false, []);
    await unary.call();
    expect(unary.seen.header).toBeNull();
  });

  it("drops keepalives and keeps events and the failure in order", async () => {
    const failure = new ConnectError("closed", Code.Unavailable);
    const { call } = run(true, [event("e-1"), keepaliveMessage(), keepaliveMessage(), event("e-2"), keepaliveMessage(), failure]);
    expect(await read(await call())).toEqual(["e-1", "e-2", failure]);
  });

  it("leaves a unary response untouched", async () => {
    const { call } = run(false, [keepaliveMessage()]);
    const res = await call();
    expect(await read(res)).toEqual([""]);
  });
});

const envelope = (flags: number, payload: Uint8Array): Uint8Array => {
  const out = new Uint8Array(5 + payload.length);
  out[0] = flags;
  new DataView(out.buffer).setUint32(1, payload.length);
  out.set(payload, 5);
  return out;
};

/** A fetch that answers every call as a Connect server stream and records the request headers. */
function streamingFetch(headers: Headers[], payloads: readonly Uint8Array[]): typeof fetch {
  return async (_input, init) => {
    headers.push(new Headers(init?.headers));
    const body = new Blob([
      ...payloads.map((p) => envelope(0, p)),
      envelope(2, new TextEncoder().encode("{}")),
    ] as BlobPart[]).stream();
    return new Response(body, { status: 200, headers: { "content-type": "application/connect+proto" } });
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

const webTransport = (fetchImpl: typeof fetch): Transport =>
  configureConnectTransport({
    baseUrl: "https://svc",
    profile: "service",
    policy: { schemaVersion: 1, methods: [] },
    fetch: fetchImpl,
  });

const wire = (value: string) => toBinary(StringValueSchema, event(value));

describe("the web transport", () => {
  it("opts in and hands the reader events only", async () => {
    const headers: Headers[] = [];
    const transport = webTransport(
      streamingFetch(headers, [wire("e-1"), STREAM_KEEPALIVE_PAYLOAD, wire("e-2")]),
    );
    const res = await transport.stream(method, undefined, undefined, undefined, (async function* () {
      yield { value: "x" };
    })());
    const got: string[] = [];
    for await (const message of res.message as AsyncIterable<StringValue>) got.push(message.value);
    expect(got).toEqual(["e-1", "e-2"]);
    expect(headers[0]?.get(STREAM_KEEPALIVE_HEADER)).toBe("1");
  });

  it("never lets a rotating stream see a keepalive", async () => {
    const headers: Headers[] = [];
    const transport = webTransport(
      streamingFetch(headers, [STREAM_KEEPALIVE_PAYLOAD, wire("e-1"), STREAM_KEEPALIVE_PAYLOAD]),
    );
    const got: string[] = [];
    for await (const message of rotatingStream<StringValue>({
      schema: StringValueSchema,
      connect: () => transport,
      open: ({ transport: opened, headers: extra, signal }) =>
        (async function* () {
          const res = await opened.stream(method, signal, undefined, extra, (async function* () {
            yield { value: "x" };
          })());
          yield* res.message as AsyncIterable<StringValue>;
        })(),
      dedupeKey: (message) => message.value,
    })) {
      got.push(message.value);
    }
    expect(got).toEqual(["e-1"]);
    expect(headers[0]?.get(STREAM_KEEPALIVE_HEADER)).toBe("1");
  });
});
