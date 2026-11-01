// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_stream_rotation.feature`.
 *
 * The steps drive the shipped `RotationMachine` directly with a virtual clock
 * in milliseconds: each event names the stream it happens on and the time it
 * happens at, and the machine's actions accumulate in a log the outcome steps
 * read. Events are plain strings whose id doubles as their dedupe key.
 */

import assert from "node:assert/strict";

import { create, type DescMessage, type Message } from "@bufbuild/protobuf";
import { FileDescriptorProtoSchema } from "@bufbuild/protobuf/wkt";
import type { Transport } from "@connectrpc/connect";
import { After, Given, Then, When, World } from "@cucumber/cucumber";

import {
  RotationMachine,
  rotatingStream,
  type RotationAction,
  type RotationFrame,
  type StreamRole,
} from "@brefwiz/api-bones-connect";

// The descriptor is shared with the package's unit tests, which keeps one copy.
import {
  T,
  control,
  field,
  registryWith,
} from "../../../api-bones-connect-ts/src/stream-rotation.fixtures.ts";

interface RotationWorld extends World {
  machine?: RotationMachine<string>;
  /** Frames the old stream has already received and the machine has not been shown. */
  buffered?: RotationFrame<string>[];
  /** Not `log`: cucumber's World already owns a `log` method. */
  actions?: RotationAction<string>[];
  schema?: DescMessage;
  /** Each call to the consumer's open function, in call order. */
  opens?: Opened[];
  consumer?: Promise<Observed>;
}

interface Opened {
  headers: Headers;
  stream: ScriptedStream;
}

interface Observed {
  events: string[];
  opens: Opened[];
}

type Log = readonly RotationAction<string>[];

const has = (log: Log, kind: RotationAction<string>["kind"]) =>
  log.some((action) => action.kind === kind);

const delivered = (log: Log): string[] =>
  log.flatMap((action) => (action.kind === "deliver" ? [action.value] : []));

const STREAMS: Readonly<Record<string, StreamRole>> = {
  "old stream": "old",
  replacement: "replacement",
};

/** What one phrase of the contract's event grammar makes a stream do. */
type Happening =
  | { frame: RotationFrame<string> }
  | { ended: "ended" | "failed" };

function happeningOf(what: string): Happening {
  let match: RegExpMatchArray | null;
  if ((match = what.match(/^delivers event "([^"]+)"$/))) {
    const id = match[1] as string;
    return { frame: { data: { value: id, key: id } } };
  }
  if (
    (match = what.match(
      /^checkpoints domain "([^"]+)" in epoch (\d+) live from (\d+) delivered through (\d+)$/,
    ))
  ) {
    return {
      frame: {
        control: {
          kind: "checkpoint",
          domain: match[1] as string,
          epoch: match[2] as string,
          liveFrom: BigInt(match[3] as string),
          deliveredThrough: BigInt(match[4] as string),
        },
      },
    };
  }
  if ((match = what.match(/^drains cutting in (\d+) ms$/))) {
    return { frame: { control: { kind: "drain", cutInMs: Number(match[1]) } } };
  }
  if (what === "sends a control that cannot be read") {
    return { frame: { control: { kind: "unreadable" } } };
  }
  if (what === "ends") return { ended: "ended" };
  if (what === "fails") return { ended: "failed" };
  throw new Error(`the contract names an event this step cannot build: ${what}`);
}

When(
  /^the (old stream|replacement) (.+) at (\d+) ms$/,
  function (this: RotationWorld, who: string, what: string, at: string) {
    if (this.machine === undefined) {
      this.machine = new RotationMachine<string>(() => (this.buffered ?? []).splice(0));
    }
    const machine = this.machine as RotationMachine<string>;
    const stream = STREAMS[who] as StreamRole;
    const happening = happeningOf(what);
    this.actions = [
      ...(this.actions ?? []),
      ...("frame" in happening
        ? machine.onFrame(stream, happening.frame, Number(at))
        : machine.onEnd(stream, Number(at), happening.ended)),
    ];
  },
);

Given("the old stream has already received event {string}", function (this: RotationWorld, id: string) {
  (this.buffered ??= []).push({ data: { value: id, key: id } });
});

When("the clock reaches {int} ms", function (this: RotationWorld, at: number) {
  const machine = this.machine as RotationMachine<string>;
  this.actions = [...(this.actions ?? []), ...machine.onTick(at)];
});

Then("the client opens a replacement", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.ok(has(log, "open"), "no replacement was opened");
});

Then("the client opens no replacement", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.ok(!has(log, "open"), "a replacement was opened");
});

Then("the client switches to the replacement", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.ok(has(log, "switch"), "the client never switched");
});

Then("the client does not switch", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.ok(!has(log, "switch"), "the client switched");
});

Then("the client keeps the old stream", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.ok(has(log, "fallback"), "the client never fell back to the old stream");
  assert.ok(!has(log, "switch"), "the client switched");
  assert.ok(!has(log, "end"), "the consumer's stream ended");
});

Then("the client has not given up on the replacement", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.ok(!has(log, "fallback"), "the client already fell back");
  assert.ok(!has(log, "end"), "the consumer's stream ended");
});

Then("the replacement is closed", function (this: RotationWorld) {
  const log = this.actions ?? [];
  const closed = log.some((action) => action.kind === "close" && action.stream === "replacement");
  assert.ok(closed, "the replacement was not closed");
});

Then("the consumer's stream ends", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.ok(has(log, "end"), "the consumer's stream did not end");
});

Then("the consumer has received no events", function (this: RotationWorld) {
  const log = this.actions ?? [];
  assert.deepEqual(delivered(log), []);
});

Then("the consumer receives events {string} in order", function (this: RotationWorld, ids: string) {
  const log = this.actions ?? [];
  const got = delivered(log);
  assert.deepEqual(got, ids.split(", "));
  assert.equal(new Set(got).size, got.length, "an event reached the consumer twice");
});

function positionOfClose(log: Log): number {
  const at = log.findIndex((action) => action.kind === "close" && action.stream === "old");
  assert.notEqual(at, -1, "the old stream was never closed");
  return at;
}

function positionOfDelivery(log: Log, id: string): number {
  const at = log.findIndex((action) => action.kind === "deliver" && action.value === id);
  assert.notEqual(at, -1, `the consumer never received ${id}`);
  return at;
}

Then(
  "the old stream is closed after the consumer received {string}",
  function (this: RotationWorld, id: string) {
    const log = this.actions ?? [];
    assert.ok(positionOfClose(log) > positionOfDelivery(log, id));
  },
);

Then(
  "the old stream is closed before the consumer received {string}",
  function (this: RotationWorld, id: string) {
    const log = this.actions ?? [];
    assert.ok(positionOfClose(log) < positionOfDelivery(log, id));
  },
);

// The shipped `rotatingStream`, driven through a scripted open function.

const registry = registryWith(
  create(FileDescriptorProtoSchema, {
    name: "app/v1/events.proto",
    package: "app.v1",
    syntax: "proto3",
    dependency: ["bones/v1/stream.proto"],
    messageType: [
      { name: "Declaring", field: [field("id", "id", 1, T.STRING), control(2047)] },
      { name: "NotDeclaring", field: [field("id", "id", 1, T.STRING)] },
    ],
  }),
);

function descriptor(name: string): DescMessage {
  const found = registry.getMessage(name);
  assert.ok(found !== undefined, `no message ${name}`);
  return found;
}

/** A server stream the scenario feeds by hand. */
class ScriptedStream implements AsyncIterable<Message> {
  private readonly items: Message[] = [];
  private waiting: ((result: IteratorResult<Message>) => void) | undefined;
  private ended = false;

  push(message: Message): void {
    if (this.waiting !== undefined) {
      const wake = this.waiting;
      this.waiting = undefined;
      wake({ value: message, done: false });
    } else {
      this.items.push(message);
    }
  }

  end(): void {
    this.ended = true;
    const wake = this.waiting;
    this.waiting = undefined;
    wake?.({ value: undefined, done: true });
  }

  [Symbol.asyncIterator](): AsyncIterator<Message> {
    return {
      next: () => {
        const item = this.items.shift();
        if (item !== undefined) return Promise.resolve({ value: item, done: false });
        if (this.ended) return Promise.resolve({ value: undefined, done: true });
        return new Promise<IteratorResult<Message>>((resolve) => {
          this.waiting = resolve;
        });
      },
    };
  }
}

const idOf = (message: Message) => (message as unknown as { id: string }).id;

const controlMessage = (schema: DescMessage, signal: { case: string; value: Message }) =>
  create(schema, { bonesControl: create(descriptor("bones.v1.StreamControl"), { signal }) });

const drainSignal = (cutInMs: number) => ({
  case: "drain",
  value: create(descriptor("bones.v1.Drain"), { cutInMs }),
});

const checkpointSignal = () => ({
  case: "checkpoint",
  value: create(descriptor("bones.v1.Checkpoint"), {
    domain: "orders",
    liveFrom: 90n,
    deliveredThrough: 99n,
    epoch: 7n,
  }),
});

/** Let every promise the driver is waiting on run to its next wait. */
async function settle(): Promise<void> {
  for (let turn = 0; turn < 5; turn += 1) {
    await new Promise<void>((resolve) => setImmediate(resolve));
  }
}

Given(
  /^a consumer whose response type (declares|does not declare) the stream control field$/,
  function (this: RotationWorld, declaration: string) {
    this.schema = descriptor(
      declaration === "declares" ? "app.v1.Declaring" : "app.v1.NotDeclaring",
    );
    this.opens = [];
  },
);

/** Read the consumer's stream to its end, with the open calls it made. */
async function observe(stream: AsyncIterable<Message>, opens: Opened[]): Promise<Observed> {
  const events: string[] = [];
  for await (const message of stream) events.push(idOf(message));
  return { events, opens: [...opens] };
}

/** Finish the server side, so the consumer's stream ends and can be read in full. */
function finishServer(opens: Opened[] | undefined): void {
  for (const opened of opens ?? []) opened.stream.end();
}

When("the consumer opens its stream through the client", async function (this: RotationWorld) {
  const opens: Opened[] = [];
  this.opens = opens;
  const stream = rotatingStream({
    schema: this.schema as DescMessage,
    connect: () => ({}) as Transport,
    open: (call) => {
      const scripted = new ScriptedStream();
      opens.push({ headers: call.headers, stream: scripted });
      return scripted;
    },
    dedupeKey: idOf,
  });
  this.consumer = observe(stream, opens);
  await settle();
});

When(/^the first stream drains cutting in (\d+) ms$/, async function (this: RotationWorld, cut: string) {
  (this.opens ?? [])[0]?.stream.push(
    controlMessage(this.schema as DescMessage, drainSignal(Number(cut))),
  );
  await settle();
});

When(
  'the first stream delivers event "e-1", a checkpoint and a drain notice',
  async function (this: RotationWorld) {
    const schema = this.schema as DescMessage;
    const first = (this.opens ?? [])[0]?.stream;
    first?.push(create(schema, { id: "e-1" }));
    first?.push(controlMessage(schema, checkpointSignal()));
    first?.push(controlMessage(schema, drainSignal(30000)));
    await settle();
  },
);

Then(
  /^the open request (carries|does not carry) the "([^"]+)" header$/,
  async function (this: RotationWorld, presence: string, name: string) {
    finishServer(this.opens);
    const observed = await this.consumer;
    assert.equal(observed?.opens.length, 1,"the client did not open exactly one stream");
    assert.equal(observed?.opens[0]?.headers.has(name), presence === "carries");
  },
);

Then(
  "the consumer's open function has been called once for each stream",
  async function (this: RotationWorld) {
    finishServer(this.opens);
    const observed = await this.consumer;
    assert.equal(observed?.opens.length, 2, "the client did not open the first stream and one replacement");
    assert.equal(
      new Set(observed?.opens.map((opened) => opened.stream)).size,
      2,
      "one open call did not serve exactly one stream",
    );
  },
);

Then("the consumer receives only event {string}", async function (this: RotationWorld, id: string) {
  finishServer(this.opens);
  const observed = await this.consumer;
  assert.deepEqual(observed?.events, [id]);
});

After(async function (this: RotationWorld) {
  finishServer(this.opens);
  await this.consumer;
});
