// SPDX-License-Identifier: MIT
import { create, type DescMessage, type Message } from "@bufbuild/protobuf";
import { FieldDescriptorProto_Label, FileDescriptorProtoSchema } from "@bufbuild/protobuf/wkt";
import type { Transport } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";

import { T, control, field, registryWith } from "./stream-rotation.fixtures.js";
import {
  RotationMachine,
  declaresStreamControl,
  rotatingStream,
  type ControlSignal,
  type RotationAction,
  type RotationFrame,
} from "./stream-rotation.js";

const appFile = create(FileDescriptorProtoSchema, {
  name: "app/v1/events.proto",
  package: "app.v1",
  syntax: "proto3",
  dependency: ["bones/v1/stream.proto"],
  messageType: [
    { name: "Event", field: [field("id", "id", 1, T.STRING), control(2047)] },
    { name: "Plain", field: [field("id", "id", 1, T.STRING)] },
    { name: "WrongNumber", field: [control(2046)] },
    {
      name: "WrongType",
      field: [
        field("bones_control", "bonesControl", 2047, T.MESSAGE, { typeName: ".bones.v1.Drain" }),
      ],
    },
    { name: "InOneof", oneofDecl: [{ name: "pick" }], field: [control(2047, { oneofIndex: 0 })] },
    {
      name: "Repeated",
      field: [control(2047, { label: FieldDescriptorProto_Label.REPEATED })],
    },
  ],
});

const registry = registryWith(appFile);

function message(name: string): DescMessage {
  const found = registry.getMessage(name);
  if (found === undefined) throw new Error(`no test message ${name}`);
  return found;
}

const eventSchema = message("app.v1.Event");

describe("declaresStreamControl", () => {
  it("is true for field 2047 of StreamControl outside any oneof", () => {
    expect(declaresStreamControl(eventSchema)).toBe(true);
  });

  it.each([
    ["no such field", "app.v1.Plain"],
    ["another field number", "app.v1.WrongNumber"],
    ["another message type", "app.v1.WrongType"],
    ["a field inside a oneof", "app.v1.InOneof"],
    ["a repeated field", "app.v1.Repeated"],
  ])("is false for %s", (_label, name) => {
    expect(declaresStreamControl(message(name))).toBe(false);
  });
});

// Frames and machine plumbing.

const drain = (cutInMs: number): ControlSignal => ({ kind: "drain", cutInMs });
const cp = (domain: string, liveFrom: number, through: number, epoch = "1"): ControlSignal => ({
  kind: "checkpoint",
  domain,
  liveFrom: BigInt(liveFrom),
  deliveredThrough: BigInt(through),
  epoch,
});
const ev = (id: string): RotationFrame<string> => ({ data: { value: id, key: id } });
const ctl = (control: ControlSignal): RotationFrame<string> => ({ control });
const UNREADABLE: ControlSignal = { kind: "unreadable" };

const deliver = (value: string): RotationAction<string> => ({ kind: "deliver", value });
const hold = (value: string): RotationAction<string> => ({ kind: "hold", value });
const switched = (actions: RotationAction<string>[]) => actions.some((a) => a.kind === "switch");
const fellBack = (actions: RotationAction<string>[]) =>
  actions.find((a) => a.kind === "fallback");

/** A machine already drained at 1000 ms with a 30000 ms cut, so its deadline is 29000. */
function rotating(opts: { buffered?: RotationFrame<string>[] } = {}) {
  const buffered = opts.buffered ?? [];
  const machine = new RotationMachine<string>(() => buffered.splice(0));
  machine.onFrame("old", ctl(cp("a", 90, 99)), 500);
  expect(machine.onFrame("old", ctl(drain(30000)), 1000)).toEqual([{ kind: "open" }]);
  return { machine, buffered };
}

describe("RotationMachine proof met", () => {
  it("switches, delivers the old stream's buffered frames before closing it, and never repeats a key", () => {
    const { machine, buffered } = rotating();

    expect(machine.onFrame("replacement", ev("e-100"), 1100)).toEqual([hold("e-100")]);
    machine.onFrame("replacement", ev("e-101"), 1110);
    machine.onFrame("replacement", ev("e-102"), 1120);
    expect(machine.onFrame("replacement", ctl(cp("a", 100, 102)), 1130)).toEqual([]);
    expect(machine.onFrame("old", ev("e-100"), 1200)).toEqual([deliver("e-100")]);

    buffered.push(ev("e-101"));
    const actions = machine.onFrame("old", ctl(cp("a", 90, 100)), 1300);

    expect(actions).toEqual([
      deliver("e-101"),
      { kind: "close", stream: "old" },
      deliver("e-102"),
      { kind: "switch" },
    ]);
  });

  it("uses the replacement's first checkpoint, not its moving head", () => {
    const { machine } = rotating();
    machine.onFrame("replacement", ctl(cp("a", 100, 100)), 1100);
    // The replacement's head moves on; the proof is still judged against 100.
    expect(machine.onFrame("replacement", ctl(cp("a", 500, 500)), 1150)).toEqual([]);
    expect(switched(machine.onFrame("old", ctl(cp("a", 90, 100)), 1200))).toBe(true);
  });

  it("drops a key delivered by the previous stream until the next drain, and keeps keyless events", () => {
    const { machine } = rotating();
    machine.onFrame("old", ev("e-99"), 1050);
    machine.onFrame("replacement", ctl(cp("a", 100, 100)), 1100);
    machine.onFrame("old", ctl(cp("a", 90, 100)), 1200);

    expect(machine.onFrame("old", ev("e-99"), 1300)).toEqual([]);
    const keyless: RotationFrame<string> = { data: { value: "k" } };
    expect(machine.onFrame("old", keyless, 1310)).toEqual([deliver("k")]);
    expect(machine.onFrame("old", keyless, 1320)).toEqual([deliver("k")]);
  });

  it("rotates again after a switch", () => {
    const { machine } = rotating();
    machine.onFrame("replacement", ctl(cp("a", 100, 100)), 1100);
    machine.onFrame("old", ctl(cp("a", 90, 100)), 1200);
    expect(machine.onFrame("old", ctl(drain(30000)), 5000)).toEqual([{ kind: "open" }]);
  });
});

describe("RotationMachine proof missed", () => {
  it("keeps the old stream until the deadline less the margin, then falls back", () => {
    const { machine } = rotating();
    machine.onFrame("replacement", ctl(cp("a", 120, 130)), 1100);
    machine.onFrame("old", ctl(cp("a", 90, 100)), 1200);

    expect(machine.deadline).toBe(29000);
    expect(machine.onTick(28999)).toEqual([]);
    expect(machine.onTick(29000)).toEqual([
      { kind: "close", stream: "replacement" },
      { kind: "fallback", reason: "deadline" },
    ]);
    expect(machine.deadline).toBeUndefined();
    // The old stream carries on until the server cuts it, then the consumer's stream ends.
    expect(machine.onFrame("old", ev("e-1"), 29500)).toEqual([deliver("e-1")]);
    expect(machine.onEnd("old", 32000, "ended")).toEqual([{ kind: "end" }]);
  });

  it("discards what the replacement held when it falls back", () => {
    const { machine } = rotating();
    machine.onFrame("replacement", ev("e-100"), 1100);
    machine.onTick(29000);
    expect(machine.onFrame("replacement", ev("e-101"), 29100)).toEqual([]);
    expect(machine.onFrame("old", ctl(cp("a", 90, 500)), 29200)).toEqual([]);
  });

  it("does not switch for a proof completed after the deadline less the margin", () => {
    const { machine } = rotating();
    machine.onFrame("replacement", ctl(cp("a", 100, 100)), 1100);
    const actions = machine.onFrame("old", ctl(cp("a", 90, 100)), 29000);
    expect(switched(actions)).toBe(false);
    expect(fellBack(actions)).toEqual({ kind: "fallback", reason: "deadline" });
  });

  it("does not switch with no checkpoint on either stream", () => {
    const machine = new RotationMachine<string>();
    machine.onFrame("old", ctl(drain(30000)), 1000);
    expect(machine.onFrame("replacement", ev("e-1"), 1100)).toEqual([hold("e-1")]);
    expect(machine.onTick(29000)).toContainEqual({ kind: "fallback", reason: "deadline" });
  });

  it.each<[string, Array<["old" | "replacement", ControlSignal]>, string]>([
    ["the replacement changes epoch", [["replacement", cp("a", 100, 100, "1")], ["replacement", cp("a", 101, 101, "2")]], "epoch-change"],
    ["the old stream changes epoch", [["old", cp("a", 90, 100, "2")]], "epoch-change"],
    ["the epochs differ", [["replacement", cp("a", 100, 100, "2")]], "epoch-mismatch"],
    ["the old stream moves back in delivered_through", [["old", cp("a", 90, 100)], ["old", cp("a", 90, 98)]], "backward"],
    ["the old stream moves back in live_from", [["old", cp("a", 90, 100)], ["old", cp("a", 80, 100)]], "backward"],
    ["the replacement moves back in live_from", [["replacement", cp("a", 100, 100)], ["replacement", cp("a", 99, 100)]], "backward"],
    ["the replacement moves back in delivered_through", [["replacement", cp("a", 100, 100)], ["replacement", cp("a", 100, 99)]], "backward"],
  ])("falls back at once when %s", (_label, frames, reason) => {
    const { machine } = rotating();
    let last: RotationAction<string>[] = [];
    for (const [stream, control] of frames) last = machine.onFrame(stream, ctl(control), 1100);
    expect(last).toEqual([
      { kind: "close", stream: "replacement" },
      { kind: "fallback", reason },
    ]);
    // A checkpoint that would now cover the replacement does not revive the rotation.
    expect(switched(machine.onFrame("old", ctl(cp("a", 90, 900, "1")), 1200))).toBe(false);
  });

  it("falls back when either stream sends a control that cannot be read", () => {
    for (const stream of ["old", "replacement"] as const) {
      const { machine } = rotating();
      expect(machine.onFrame(stream, ctl(UNREADABLE), 1100)).toEqual([
        { kind: "close", stream: "replacement" },
        { kind: "fallback", reason: "unreadable-control" },
      ]);
    }
  });
});

describe("RotationMachine two domains", () => {
  /** Two domains checkpointed on the old stream, a drain, and the replacement covering `a` only. */
  function twoDomainsAnnouncingA() {
    const machine = new RotationMachine<string>();
    machine.onFrame("old", ctl(cp("a", 90, 99)), 500);
    machine.onFrame("old", ctl(cp("b", 40, 49)), 510);
    machine.onFrame("old", ctl(drain(30000)), 1000);
    machine.onFrame("replacement", ctl(cp("a", 100, 100)), 1100);
    return machine;
  }

  function twoDomains() {
    const machine = twoDomainsAnnouncingA();
    machine.onFrame("replacement", ctl(cp("b", 50, 50)), 1110);
    return machine;
  }

  it("switches only once both domains are covered", () => {
    const machine = twoDomains();
    expect(switched(machine.onFrame("old", ctl(cp("a", 90, 100)), 1200))).toBe(false);
    expect(switched(machine.onFrame("old", ctl(cp("b", 40, 50)), 1210))).toBe(true);
  });

  it("falls back when one domain stays uncovered", () => {
    const machine = twoDomains();
    machine.onFrame("old", ctl(cp("a", 90, 100)), 1200);
    machine.onFrame("old", ctl(cp("b", 40, 49)), 1210);
    expect(machine.onTick(29000)).toContainEqual({ kind: "fallback", reason: "deadline" });
  });

  it("falls back when the replacement never announces one domain", () => {
    const machine = twoDomainsAnnouncingA();
    expect(switched(machine.onFrame("old", ctl(cp("a", 90, 100)), 1200))).toBe(false);
    expect(machine.onTick(29000)).toContainEqual({ kind: "fallback", reason: "deadline" });
  });
});

describe("RotationMachine failing replacement", () => {
  it.each<[string, (m: RotationMachine<string>) => RotationAction<string>[], string]>([
    ["fails to open", (m) => m.onEnd("replacement", 1100, "failed"), "replacement-failed"],
    ["ends", (m) => m.onEnd("replacement", 1100, "ended"), "replacement-ended"],
    ["sends a drain", (m) => m.onFrame("replacement", ctl(drain(30000)), 1100), "replacement-drained"],
  ])("keeps the old stream when the replacement %s", (_label, act, reason) => {
    const { machine } = rotating();
    expect(act(machine)).toEqual([
      { kind: "close", stream: "replacement" },
      { kind: "fallback", reason },
    ]);
    expect(machine.onFrame("old", ev("e-1"), 1200)).toEqual([deliver("e-1")]);
  });
});

describe("RotationMachine old stream ending", () => {
  it("ends the consumer's stream and closes the replacement", () => {
    const { machine } = rotating();
    machine.onFrame("replacement", ev("e-100"), 1100);
    expect(machine.onEnd("old", 1200, "ended")).toEqual([
      { kind: "close", stream: "replacement" },
      { kind: "end" },
    ]);
    expect(machine.onFrame("replacement", ev("e-101"), 1300)).toEqual([]);
  });
});

describe("RotationMachine drain handling", () => {
  it("ignores a second drain on the old stream", () => {
    const { machine } = rotating();
    expect(machine.onFrame("old", ctl(drain(500)), 1100)).toEqual([]);
    expect(machine.deadline).toBe(29000);
  });

  it("ignores a drain after falling back", () => {
    const { machine } = rotating();
    machine.onTick(29000);
    expect(machine.onFrame("old", ctl(drain(30000)), 29100)).toEqual([]);
  });

  it.each([2000, 1500, 0])("opens nothing for a drain cutting in %i ms", (cutInMs) => {
    const machine = new RotationMachine<string>();
    expect(machine.onFrame("old", ctl(drain(cutInMs)), 1000)).toEqual([
      { kind: "fallback", reason: "too-close" },
    ]);
    expect(machine.deadline).toBeUndefined();
  });

  it("opens a replacement for a drain just outside the margin", () => {
    const machine = new RotationMachine<string>();
    expect(machine.onFrame("old", ctl(drain(2001)), 1000)).toEqual([{ kind: "open" }]);
  });
});

describe("RotationMachine bounds", () => {
  it("reads a cut longer than a minute as a minute", () => {
    const machine = new RotationMachine<string>();
    expect(machine.onFrame("old", ctl(drain(4_000_000_000)), 1000)).toEqual([{ kind: "open" }]);
    expect(machine.deadline).toBe(1000 + 60_000 - 2000);
  });

  it("abandons the replacement when it holds ten thousand frames, keeping the old stream", () => {
    const { machine } = rotating();
    for (let i = 0; i < 9_999; i += 1) {
      expect(machine.onFrame("replacement", ev(`r-${i}`), 1100)).toEqual([hold(`r-${i}`)]);
    }
    expect(machine.onFrame("replacement", ev("r-last"), 1100)).toEqual([
      { kind: "close", stream: "replacement" },
      { kind: "fallback", reason: "held-overflow" },
    ]);
    expect(machine.deadline).toBeUndefined();
    expect(machine.onFrame("replacement", ev("r-after"), 1200)).toEqual([]);
    expect(machine.onFrame("old", ev("e-1"), 1300)).toEqual([deliver("e-1")]);
    expect(machine.onEnd("old", 31000, "ended")).toEqual([{ kind: "end" }]);
  });
});

// The driver, over real async streams.

const eventOf = (id: string) => create(eventSchema, { id });
const controlOnly = (signal: ReturnType<typeof signalOf>) =>
  create(eventSchema, { bonesControl: create(message("bones.v1.StreamControl"), { signal }) });

function signalOf(kind: "drain" | "checkpoint", init: Record<string, unknown>) {
  return { case: kind, value: create(message(`bones.v1.${kind === "drain" ? "Drain" : "Checkpoint"}`), init) };
}
const drainMsg = (cutInMs: number) => controlOnly(signalOf("drain", { cutInMs }));
const cpMsg = (domain: string, liveFrom: number, through: number) =>
  controlOnly(
    signalOf("checkpoint", {
      domain,
      liveFrom: BigInt(liveFrom),
      deliveredThrough: BigInt(through),
      epoch: 1n,
    }),
  );

const idOf = (m: Message) => (m as unknown as { id: string }).id;
const never = new Promise<never>(() => undefined);

function fakeTransports() {
  const made: Array<{ close: ReturnType<typeof vi.fn> }> = [];
  const connect = vi.fn(() => {
    const transport = { close: vi.fn() };
    made.push(transport);
    return transport as unknown as Transport;
  });
  return { made, connect };
}

describe("rotatingStream", () => {
  it("passes a stream through unchanged, with no control header, when the type declares none", async () => {
    const { connect, made } = fakeTransports();
    const sent: Headers[] = [];
    const plain = message("app.v1.Plain");
    const items = [create(plain, { id: "p-1" }), create(plain, { id: "p-2" })];

    const got: Message[] = [];
    for await (const m of rotatingStream({
      schema: plain,
      connect,
      open: async function* ({ headers }) {
        sent.push(headers);
        yield* items;
      },
      dedupeKey: idOf,
    })) {
      got.push(m);
    }

    expect(got).toEqual(items);
    expect(got[0]).toBe(items[0]);
    expect(connect).toHaveBeenCalledTimes(1);
    expect(sent).toHaveLength(1);
    expect(sent[0]?.has("bones-stream-control")).toBe(false);
    expect(made[0]?.close).toHaveBeenCalled();
  });

  it("drops a control-only message and strips the control from one that carries data", async () => {
    const { connect } = fakeTransports();
    const sent: Headers[] = [];
    const withData = create(eventSchema, {
      id: "e-1",
      bonesControl: create(message("bones.v1.StreamControl"), {
        signal: signalOf("checkpoint", { domain: "a", liveFrom: 1n, deliveredThrough: 1n, epoch: 1n }),
      }),
    });

    const got: Message[] = [];
    for await (const m of rotatingStream({
      schema: eventSchema,
      connect,
      open: async function* ({ headers }) {
        sent.push(headers);
        yield cpMsg("a", 1, 1);
        yield withData;
        yield eventOf("e-2");
      },
      dedupeKey: idOf,
    })) {
      got.push(m);
    }

    expect(sent[0]?.get("bones-stream-control")).toBe("1");
    expect(got.map(idOf)).toEqual(["e-1", "e-2"]);
    expect((got[0] as unknown as { bonesControl?: unknown }).bonesControl).toBeUndefined();
  });

  it("rotates onto a fresh transport on a drain and closes the retired one", async () => {
    const { connect, made } = fakeTransports();
    const sent: Headers[] = [];
    let replacementOpened!: () => void;
    const opened = new Promise<void>((resolve) => {
      replacementOpened = resolve;
    });

    const oldStream = async function* () {
      yield eventOf("e-1");
      yield cpMsg("a", 90, 99);
      yield drainMsg(30000);
      await opened;
      yield cpMsg("a", 90, 100);
      await never;
    };
    const replacementStream = async function* () {
      yield eventOf("e-100");
      yield cpMsg("a", 100, 100);
      yield eventOf("e-101");
      yield eventOf("e-102");
      await never;
    };
    const streams = [oldStream, replacementStream];

    const got: string[] = [];
    for await (const m of rotatingStream({
      schema: eventSchema,
      connect,
      open: ({ headers }) => {
        sent.push(headers);
        if (sent.length === 2) replacementOpened();
        return streams[sent.length - 1]!();
      },
      dedupeKey: idOf,
    })) {
      got.push(idOf(m));
      if (got.length === 4) break;
    }

    expect(got).toEqual(["e-1", "e-100", "e-101", "e-102"]);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(sent.map((h) => h.get("bones-stream-control"))).toEqual(["1", "1"]);
    expect(made[0]?.close).toHaveBeenCalledTimes(1);
    expect(made[1]?.close).toHaveBeenCalledTimes(1);
  });

  it("ends as the old stream does when the replacement fails to open", async () => {
    const { connect, made } = fakeTransports();
    let opens = 0;
    const got: string[] = [];
    for await (const m of rotatingStream({
      schema: eventSchema,
      connect,
      open: () => {
        opens += 1;
        if (opens === 2) throw new Error("replacement refused");
        return (async function* () {
          yield cpMsg("a", 90, 99);
          yield drainMsg(30000);
          yield eventOf("e-1");
        })();
      },
      dedupeKey: idOf,
    })) {
      got.push(idOf(m));
    }
    expect(got).toEqual(["e-1"]);
    expect(made[1]?.close).toHaveBeenCalled();
  });

  /** Run a drain whose replacement says nothing, under a virtual clock. */
  async function silentReplacement(cutInMs: number) {
    const { connect, made } = fakeTransports();
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => {
      finish = resolve;
    });
    let opens = 0;
    const got: string[] = [];
    const consumer = (async () => {
      for await (const m of rotatingStream({
        schema: eventSchema,
        connect,
        open: () => {
          opens += 1;
          if (opens > 1) return (async function* () { await never; })();
          return (async function* () {
            yield cpMsg("a", 90, 99);
            yield drainMsg(cutInMs);
            await finished;
            yield eventOf("e-1");
          })();
        },
        dedupeKey: idOf,
      })) {
        got.push(idOf(m));
      }
    })();
    return { made, got, consumer, finish };
  }

  it("gives up on the replacement at the deadline less the margin and keeps the old stream", async () => {
    vi.useFakeTimers();
    try {
      const { made, got, consumer, finish } = await silentReplacement(5000);
      await vi.advanceTimersByTimeAsync(2999);
      expect(made).toHaveLength(2);
      expect(made[1]?.close).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(2);
      expect(made[1]?.close).toHaveBeenCalledTimes(1);
      finish();
      await consumer;
      expect(got).toEqual(["e-1"]);
      expect(made[0]?.close).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("reads a cut longer than a minute as a minute for the deadline", async () => {
    vi.useFakeTimers();
    try {
      const { made, consumer, finish } = await silentReplacement(4_000_000_000);
      await vi.advanceTimersByTimeAsync(57_999);
      expect(made).toHaveLength(2);
      expect(made[1]?.close).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(2);
      expect(made[1]?.close).toHaveBeenCalledTimes(1);
      finish();
      await consumer;
    } finally {
      vi.useRealTimers();
    }
  });

  it("fails as the old stream fails, after delivering what it sent, during a rotation", async () => {
    const { connect } = fakeTransports();
    let opens = 0;
    const got: string[] = [];
    const run = async () => {
      for await (const m of rotatingStream({
        schema: eventSchema,
        connect,
        open: () => {
          opens += 1;
          if (opens > 1) return (async function* () { await never; })();
          return (async function* () {
            yield eventOf("e-1");
            yield drainMsg(30000);
            throw new Error("old stream broke");
          })();
        },
        dedupeKey: idOf,
      })) {
        got.push(idOf(m));
      }
    };
    await expect(run()).rejects.toThrow("old stream broke");
    expect(got).toEqual(["e-1"]);
  });

  it("fails as the old stream fails when nothing is rotating", async () => {
    const { connect } = fakeTransports();
    const got: string[] = [];
    const run = async () => {
      for await (const m of rotatingStream({
        schema: eventSchema,
        connect,
        open: async function* () {
          yield eventOf("e-1");
          throw new Error("old stream broke");
        },
        dedupeKey: idOf,
      })) {
        got.push(idOf(m));
      }
    };
    await expect(run()).rejects.toThrow("old stream broke");
    expect(got).toEqual(["e-1"]);
  });

  it("passes an error through unchanged when the type declares no control", async () => {
    const { connect, made } = fakeTransports();
    const plain = message("app.v1.Plain");
    const failure = new Error("plain stream broke");
    const got: string[] = [];
    const run = async () => {
      for await (const m of rotatingStream({
        schema: plain,
        connect,
        open: async function* () {
          yield create(plain, { id: "p-1" });
          throw failure;
        },
        dedupeKey: idOf,
      })) {
        got.push(idOf(m));
      }
    };
    await expect(run()).rejects.toBe(failure);
    expect(got).toEqual(["p-1"]);
    expect(made[0]?.close).toHaveBeenCalledTimes(1);
  });

  it("reads the stream only as fast as the consumer pulls, with or without a control", async () => {
    for (const schema of [message("app.v1.Plain"), eventSchema]) {
      const { connect } = fakeTransports();
      let produced = 0;
      let pulled = 0;
      for await (const m of rotatingStream({
        schema,
        connect,
        open: async function* () {
          for (let i = 0; i < 1000; i += 1) {
            produced += 1;
            yield create(schema, { id: `m-${i}` });
          }
        },
        dedupeKey: idOf,
      })) {
        pulled += 1;
        expect(idOf(m)).toBe(`m-${pulled - 1}`);
        // Let anything that reads ahead of the consumer run.
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(produced).toBeLessThanOrEqual(pulled + 1);
        if (pulled === 5) break;
      }
      expect(produced).toBeLessThanOrEqual(6);
    }
  });
});
