// SPDX-License-Identifier: MIT
//
// Stream rotation: a long-lived server stream is replaced before the server
// cuts it, so the consumer sees one logical stream with no gap.
//
// A server about to cut a stream sends a drain notice in-band, a message that
// sets only field 2047 (`bones.v1.StreamControl`) of the stream's response
// message. It also sends checkpoints, one per domain, each reporting how far
// the stream has delivered. On a drain the client opens a replacement on a
// fresh connection and switches only on proof that nothing falls between the
// two streams: in every domain either stream has checkpointed, the old
// stream's latest `delivered_through` is at or above the `live_from` of the
// replacement's FIRST checkpoint, both in the same epoch. Any other outcome
// keeps the old stream until the server cuts it, and the consumer's own loop
// takes its usual path (reopen and report a gap).
//
// `RotationMachine` is the pure decision logic: frames, lifecycle events and
// the current time in, actions out. `rotatingStream` runs it over real streams
// and timers.

import {
  clone,
  create,
  equals,
  type DescMessage,
  type Message,
} from "@bufbuild/protobuf";
import type { Transport } from "@connectrpc/connect";

/** The field number every control-carrying response message reserves. */
const STREAM_CONTROL_FIELD = 2047;
const STREAM_CONTROL_TYPE = "bones.v1.StreamControl";
const STREAM_CONTROL_HEADER = "bones-stream-control";

/**
 * The time the client keeps in hand before the cut: the replacement must prove
 * continuity this long before the server drops the old stream, leaving time to
 * finish the switch. Fixed; a consumer cannot move it.
 */
const SWITCH_MARGIN_MS = 2000;

/**
 * The longest cut a server announces. A drain naming a longer cut is read as
 * this one, so the rotation deadline never lies beyond it. Fixed.
 */
const MAX_CUT_MS = 60_000;

/**
 * The most frames held back from the replacement. Reaching it abandons the
 * replacement and keeps the old stream. Fixed.
 */
const MAX_HELD_FRAMES = 10_000;

/**
 * True only when `schema` has a field numbered 2047, of message type
 * `bones.v1.StreamControl`, outside any oneof. A server sends controls only to
 * a client that asked for them, and the client asks only when the response
 * type can carry one.
 */
export function declaresStreamControl(schema: DescMessage): boolean {
  return schema.fields.some(
    (field) =>
      field.number === STREAM_CONTROL_FIELD &&
      field.fieldKind === "message" &&
      field.message.typeName === STREAM_CONTROL_TYPE &&
      field.oneof === undefined,
  );
}

/** One control signal, as the machine reads it. */
export type ControlSignal =
  | { kind: "drain"; cutInMs: number }
  | {
      kind: "checkpoint";
      domain: string;
      liveFrom: bigint;
      deliveredThrough: bigint;
      epoch: string;
    }
  | { kind: "unreadable" };

/** One frame off a stream: its control, if any, and its application data, if any. */
export interface RotationFrame<T> {
  control?: ControlSignal;
  data?: { value: T; key?: string };
}

/** Which of the two streams an event belongs to. */
export type StreamRole = "old" | "replacement";

export type FallbackReason =
  | "deadline"
  | "too-close"
  | "epoch-change"
  | "epoch-mismatch"
  | "backward"
  | "replacement-drained"
  | "unreadable-control"
  | "replacement-ended"
  | "replacement-failed"
  | "held-overflow";

/** What the caller must do next, in order. */
export type RotationAction<T> =
  | { kind: "deliver"; value: T }
  | { kind: "hold"; value: T }
  | { kind: "open" }
  | { kind: "close"; stream: StreamRole }
  | { kind: "switch" }
  | { kind: "fallback"; reason: FallbackReason }
  | { kind: "end" };

interface Mark {
  liveFrom: bigint;
  deliveredThrough: bigint;
  epoch: string;
}

interface Track {
  first: Map<string, Mark>;
  latest: Map<string, Mark>;
}

const newTrack = (): Track => ({ first: new Map(), latest: new Map() });

type Phase = "steady" | "rotating" | "kept" | "ended";

/**
 * The rotation decision logic. It owns no stream and no timer: every call
 * carries the time in milliseconds, so a test drives it with a virtual clock.
 *
 * Old-stream data is delivered as it arrives; replacement data is held until
 * the switch. `takeBufferedOld` returns the frames the old stream has already
 * received but the machine has not yet been shown; the machine delivers them
 * before it closes the old stream.
 */
export class RotationMachine<T> {
  private phase: Phase = "steady";
  private drained = false;
  private cutAt = 0;
  private oldTrack = newTrack();
  private replacementTrack = newTrack();
  private held: Array<{ value: T; key?: string }> = [];
  private current = new Set<string>();
  private previous = new Set<string>();

  constructor(private readonly takeBufferedOld: () => RotationFrame<T>[] = () => []) {}

  /** When the current rotation gives up and keeps the old stream, if one is under way. */
  get deadline(): number | undefined {
    return this.phase === "rotating" ? this.cutAt : undefined;
  }

  /** A frame arrived on `stream` at `now`. */
  onFrame(stream: StreamRole, frame: RotationFrame<T>, now: number): RotationAction<T>[] {
    if (this.phase === "ended") return [];
    const out = this.expire(now);
    if (stream === "replacement") {
      if (this.phase !== "rotating") return out;
      if (frame.data) {
        this.held.push(frame.data);
        if (this.held.length >= MAX_HELD_FRAMES) {
          out.push(...this.keepOld("held-overflow"));
          return out;
        }
        out.push({ kind: "hold", value: frame.data.value });
      }
      if (frame.control) out.push(...this.replacementControl(frame.control));
      return out;
    }
    if (frame.data) out.push(...this.deliver(frame.data));
    if (frame.control) out.push(...this.oldControl(frame.control, now));
    return out;
  }

  /** A stream ended (`ended`) or failed, including failing to open (`failed`). */
  onEnd(stream: StreamRole, now: number, cause: "ended" | "failed"): RotationAction<T>[] {
    if (this.phase === "ended") return [];
    const out = this.expire(now);
    if (stream === "replacement") {
      if (this.phase !== "rotating") return out;
      out.push(
        ...this.keepOld(cause === "failed" ? "replacement-failed" : "replacement-ended"),
      );
      return out;
    }
    if (this.phase === "rotating") out.push({ kind: "close", stream: "replacement" });
    this.phase = "ended";
    this.held = [];
    out.push({ kind: "end" });
    return out;
  }

  /** Time passed with no event. */
  onTick(now: number): RotationAction<T>[] {
    return this.phase === "ended" ? [] : this.expire(now);
  }

  private expire(now: number): RotationAction<T>[] {
    if (this.phase === "rotating" && now >= this.cutAt) return this.keepOld("deadline");
    return [];
  }

  private keepOld(reason: FallbackReason): RotationAction<T>[] {
    const wasRotating = this.phase === "rotating";
    this.phase = "kept";
    this.held = [];
    this.replacementTrack = newTrack();
    const out: RotationAction<T>[] = [];
    if (wasRotating) out.push({ kind: "close", stream: "replacement" });
    out.push({ kind: "fallback", reason });
    return out;
  }

  private oldControl(control: ControlSignal, now: number): RotationAction<T>[] {
    switch (control.kind) {
      case "drain": {
        if (this.drained) return [];
        this.drained = true;
        const cutAt = now + Math.min(control.cutInMs, MAX_CUT_MS) - SWITCH_MARGIN_MS;
        if (cutAt <= now) {
          this.phase = "kept";
          return [{ kind: "fallback", reason: "too-close" }];
        }
        this.phase = "rotating";
        this.cutAt = cutAt;
        this.previous = new Set();
        return [{ kind: "open" }];
      }
      case "checkpoint":
        return this.checkpoint("old", control);
      case "unreadable":
        return this.phase === "rotating" ? this.keepOld("unreadable-control") : [];
    }
  }

  private replacementControl(control: ControlSignal): RotationAction<T>[] {
    switch (control.kind) {
      case "drain":
        return this.keepOld("replacement-drained");
      case "checkpoint":
        return this.checkpoint("replacement", control);
      case "unreadable":
        return this.keepOld("unreadable-control");
    }
  }

  private checkpoint(
    stream: StreamRole,
    cp: Extract<ControlSignal, { kind: "checkpoint" }>,
  ): RotationAction<T>[] {
    const own = stream === "old" ? this.oldTrack : this.replacementTrack;
    const other = stream === "old" ? this.replacementTrack : this.oldTrack;
    const mark: Mark = {
      liveFrom: cp.liveFrom,
      deliveredThrough: cp.deliveredThrough,
      epoch: cp.epoch,
    };
    if (this.phase === "rotating") {
      const before = own.latest.get(cp.domain);
      if (before !== undefined) {
        if (before.epoch !== mark.epoch) return this.keepOld("epoch-change");
        if (mark.liveFrom < before.liveFrom || mark.deliveredThrough < before.deliveredThrough) {
          return this.keepOld("backward");
        }
      }
      // Compared against the other stream's mark the proof reads: the old
      // stream's latest, the replacement's first.
      const peer = stream === "old" ? other.first.get(cp.domain) : other.latest.get(cp.domain);
      if (peer !== undefined && peer.epoch !== mark.epoch) return this.keepOld("epoch-mismatch");
    }
    own.latest.set(cp.domain, mark);
    if (!own.first.has(cp.domain)) own.first.set(cp.domain, mark);
    if (this.phase === "rotating" && this.proven()) return this.switchOver();
    return [];
  }

  private proven(): boolean {
    const domains = new Set([...this.oldTrack.latest.keys(), ...this.replacementTrack.latest.keys()]);
    if (domains.size === 0) return false;
    for (const domain of domains) {
      const old = this.oldTrack.latest.get(domain);
      const first = this.replacementTrack.first.get(domain);
      if (old === undefined || first === undefined) return false;
      if (old.epoch !== first.epoch) return false;
      if (old.deliveredThrough < first.liveFrom) return false;
    }
    return true;
  }

  private switchOver(): RotationAction<T>[] {
    const out: RotationAction<T>[] = [];
    for (const frame of this.takeBufferedOld()) {
      if (frame.data) out.push(...this.deliver(frame.data));
    }
    out.push({ kind: "close", stream: "old" });
    this.previous = this.current;
    this.current = new Set();
    for (const event of this.held) out.push(...this.deliver(event));
    this.held = [];
    this.oldTrack = this.replacementTrack;
    this.replacementTrack = newTrack();
    this.drained = false;
    this.phase = "steady";
    out.push({ kind: "switch" });
    return out;
  }

  private deliver(event: { value: T; key?: string }): RotationAction<T>[] {
    const { key } = event;
    if (key !== undefined) {
      if (this.current.has(key) || this.previous.has(key)) return [];
      this.current.add(key);
    }
    return [{ kind: "deliver", value: event.value }];
  }
}

/** Read a decoded `StreamControl` into the machine's vocabulary. */
function readControl(raw: unknown): ControlSignal {
  const signal = (raw as { signal?: { case?: string; value?: Record<string, unknown> } }).signal;
  const value = signal?.value;
  if (signal?.case === "drain" && value !== undefined) {
    return typeof value.cutInMs === "number"
      ? { kind: "drain", cutInMs: value.cutInMs }
      : { kind: "unreadable" };
  }
  if (signal?.case === "checkpoint" && value !== undefined) {
    const { domain, liveFrom, deliveredThrough, epoch } = value;
    if (
      typeof domain === "string" &&
      isPosition(liveFrom) &&
      isPosition(deliveredThrough) &&
      (typeof epoch === "bigint" || typeof epoch === "number" || typeof epoch === "string")
    ) {
      return {
        kind: "checkpoint",
        domain,
        liveFrom: BigInt(liveFrom),
        deliveredThrough: BigInt(deliveredThrough),
        epoch: String(epoch),
      };
    }
  }
  return { kind: "unreadable" };
}

function isPosition(value: unknown): value is bigint | number {
  return typeof value === "bigint" || typeof value === "number";
}

/**
 * Split a response message into its control and the rest. `rest` is the
 * message itself when it carried no control, the message with the control
 * cleared when it carried data too, and `undefined` when it carried only the
 * control.
 */
function splitControl<M extends Message>(
  schema: DescMessage,
  message: M,
): { control?: ControlSignal; rest?: M } {
  const field = schema.fields.find((f) => f.number === STREAM_CONTROL_FIELD);
  if (field === undefined) return { rest: message };
  const raw = (message as unknown as Record<string, unknown>)[field.localName];
  if (raw === undefined || raw === null) return { rest: message };
  const control = readControl(raw);
  const rest = clone(schema, message);
  (rest as unknown as Record<string, unknown>)[field.localName] = undefined;
  if (equals(schema, rest, create(schema))) return { control };
  return { control, rest: rest as unknown as M };
}

/** What `open` receives for each stream the driver starts. */
export interface OpenCall {
  /** A transport used for this stream alone. */
  transport: Transport;
  /** Request headers to send; carries `bones-stream-control: 1` when the schema declares it. */
  headers: Headers;
  /** Aborted when the driver retires the stream. */
  signal: AbortSignal;
}

export interface RotatingStreamOptions<M extends Message> {
  /** The stream's response message. */
  schema: DescMessage;
  /**
   * Produces the transport for one stream. Called once per stream, so a Node
   * caller passes `() => configureNodeConnectTransport(opts)` and every stream
   * gets a fresh session; a web caller passes its one shared transport. A
   * retired transport is closed when it has a `close` method.
   */
  connect: () => Transport | Promise<Transport>;
  /** Starts the server stream on the given transport. */
  open: (call: OpenCall) => AsyncIterable<M>;
  /** The key an event is deduplicated on, or undefined for an event that is never dropped. */
  dedupeKey: (message: M) => string | undefined;
}

/** What one read of a stream produced. */
type Read<M> =
  | { kind: "message"; message: M }
  | { kind: "end" }
  | { kind: "error"; error: unknown };

/**
 * One stream, read on demand: at most one read is outstanding, and the next
 * one starts only when the driver asks for it, so a slow consumer holds back
 * the stream instead of filling a queue.
 */
interface Leg<M> {
  /** The settled result of the outstanding read, until `take` consumes it. */
  ready: Read<M> | undefined;
  /** The outstanding read, started if none is. */
  read(): Promise<Read<M>>;
  /** Mark the settled read as consumed, so the next `read` pulls again. */
  take(): void;
  close(): void;
}

const clockMs = (): number => performance.now();

/**
 * Open a server stream that rotates itself on a drain notice.
 *
 * The consumer iterates one logical stream. Control frames never reach it: a
 * message that carried only a control is dropped, one that carried data too is
 * yielded with the control cleared. When the response type does not declare
 * the control field, one stream is opened and passed through unchanged.
 *
 * When rotation cannot prove continuity the old stream stays until the server
 * cuts it, and the iteration ends or fails exactly as that stream does.
 */
export async function* rotatingStream<M extends Message>(
  options: RotatingStreamOptions<M>,
): AsyncGenerator<M, void, undefined> {
  const { schema, connect, open, dedupeKey } = options;
  const rotates = declaresStreamControl(schema);

  const openCall = (transport: Transport, signal: AbortSignal): OpenCall => {
    const headers = new Headers();
    if (rotates) headers.set(STREAM_CONTROL_HEADER, "1");
    return { transport, headers, signal };
  };

  // A type that declares no control is one stream, passed through untouched
  // with nothing reading it but the consumer.
  if (!rotates) {
    const controller = new AbortController();
    const transport = await connect();
    try {
      yield* open(openCall(transport, controller.signal));
    } finally {
      controller.abort();
      closeTransport(transport);
    }
    return;
  }

  const startLeg = (): Leg<M> => {
    const controller = new AbortController();
    let transport: Transport | undefined;
    let iterator: AsyncIterator<M> | undefined;
    let pending: Promise<Read<M>> | undefined;
    let closed = false;

    const pull = async (): Promise<Read<M>> => {
      try {
        if (iterator === undefined) {
          const connected = await connect();
          if (closed) {
            closeTransport(connected);
            return { kind: "end" };
          }
          transport = connected;
          iterator = open(openCall(connected, controller.signal))[Symbol.asyncIterator]();
        }
        const step = await iterator.next();
        return step.done === true ? { kind: "end" } : { kind: "message", message: step.value };
      } catch (error) {
        return { kind: "error", error };
      }
    };

    const leg: Leg<M> = {
      ready: undefined,
      read() {
        pending ??= pull().then((result) => {
          leg.ready = result;
          return result;
        });
        return pending;
      },
      take() {
        pending = undefined;
        leg.ready = undefined;
      },
      close() {
        if (closed) return;
        closed = true;
        controller.abort();
        closeTransport(transport);
      },
    };
    return leg;
  };

  const toFrame = (message: M): RotationFrame<M> => {
    const { control, rest } = splitControl(schema, message);
    return {
      control,
      data: rest === undefined ? undefined : { value: rest, key: dedupeKey(rest) },
    };
  };

  let oldLeg = startLeg();
  let replacement: Leg<M> | undefined;
  void oldLeg.read();

  // Old-stream frames read ahead of the machine, shown to it before anything
  // else and delivered by the machine before it closes the old stream.
  const unseen: RotationFrame<M>[] = [];
  // The old stream ended while frames read ahead were still unseen; the end
  // waits until they have been shown.
  type Ended = Extract<Read<M>, { kind: "end" | "error" }>;
  let oldEnd = undefined as Ended | undefined;
  let oldFailure = undefined as { error: unknown } | undefined;

  const machine = new RotationMachine<M>(() => unseen.splice(0));

  // Move what the old stream has already produced into `unseen`, without
  // waiting for more.
  const readAheadOld = (): void => {
    while (oldEnd === undefined && oldLeg.ready !== undefined) {
      const result = oldLeg.ready;
      oldLeg.take();
      if (result.kind === "message") {
        unseen.push(toFrame(result.message));
        void oldLeg.read();
      } else {
        oldEnd = result;
      }
    }
  };

  const endOld = (end: Ended):RotationAction<M>[] => {
    oldFailure = end.kind === "error" ? { error: end.error } : undefined;
    return machine.onEnd("old", clockMs(), end.kind === "error" ? "failed" : "ended");
  };

  type Woke =
    | { who: "old" | "replacement"; result: Read<M> }
    | { who: "deadline" };

  const wake = async (): Promise<Woke> => {
    const cutAt = machine.deadline;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const reads: Array<Promise<Woke>> = [];
    // The replacement is listed first: its frames are only held, and reading
    // them first is what makes the old stream's read-ahead matter to the switch.
    if (replacement !== undefined) {
      reads.push(replacement.read().then((result): Woke => ({ who: "replacement", result })));
    }
    reads.push(oldLeg.read().then((result): Woke => ({ who: "old", result })));
    if (cutAt !== undefined) {
      reads.push(
        new Promise<Woke>((resolve) => {
          timer = setTimeout(() => resolve({ who: "deadline" }), Math.max(0, cutAt - clockMs()));
        }),
      );
    }
    try {
      return await Promise.race(reads);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
    }
  };

  try {
    for (;;) {
      let actions: RotationAction<M>[];
      const queued = unseen.shift();
      if (queued !== undefined) {
        actions = machine.onFrame("old", queued, clockMs());
      } else if (oldEnd !== undefined) {
        const end = oldEnd;
        oldEnd = undefined;
        actions = endOld(end);
      } else {
        const woke = await wake();
        if (woke.who === "deadline") {
          actions = machine.onTick(clockMs());
        } else if (woke.who === "old") {
          oldLeg.take();
          actions =
            woke.result.kind === "message"
              ? machine.onFrame("old", toFrame(woke.result.message), clockMs())
              : endOld(woke.result);
        } else {
          replacement?.take();
          if (woke.result.kind === "message") {
            readAheadOld();
            actions = machine.onFrame("replacement", toFrame(woke.result.message), clockMs());
          } else {
            replacement?.close();
            replacement = undefined;
            actions = machine.onEnd(
              "replacement",
              clockMs(),
              woke.result.kind === "error" ? "failed" : "ended",
            );
          }
        }
      }
      for (const action of actions) {
        switch (action.kind) {
          case "deliver":
            yield action.value;
            break;
          case "open":
            replacement = startLeg();
            void replacement.read();
            break;
          case "close":
            if (action.stream === "old") {
              oldLeg.close();
              oldEnd = undefined;
              oldFailure = undefined;
            } else {
              replacement?.close();
              replacement = undefined;
            }
            break;
          case "switch":
            if (replacement !== undefined) oldLeg = replacement;
            replacement = undefined;
            break;
          case "end":
            if (oldFailure !== undefined) throw oldFailure.error;
            return;
          case "hold":
          case "fallback":
            break;
        }
      }
    }
  } finally {
    oldLeg.close();
    replacement?.close();
  }
}

function closeTransport(transport: Transport | undefined): void {
  const closable = transport as { close?: () => void } | undefined;
  if (typeof closable?.close === "function") closable.close();
}
