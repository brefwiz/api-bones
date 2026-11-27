// SPDX-License-Identifier: MIT
import { create, toBinary } from "@bufbuild/protobuf";
import { CredentialAnswerSchema, CredentialSchema, type Credential as CredentialMessage } from "./generated/bones/sdk/v1/boundary_pb.js";
import { BonesError } from "./errors.js";
import type { JsSpanSink } from "./spans.js";

/** The boundary version this runtime speaks; a core built for another version refuses to construct a client. */
export const RUNTIME_BOUNDARY = 1;

/** What the host reads about the caller before every send. */
export interface Credential {
  readonly subject: string;
  readonly org: string;
  readonly headers: Readonly<Record<string, string>>;
}

/** What the core asks of the host's credential. */
export interface CredentialAsk {
  /** True when the server refused the credential last answered: answer a renewed one if there is one. */
  readonly refresh: boolean;
  /** Aborted when the call that waits for the answer ended before it arrived. */
  readonly signal: AbortSignal;
}

/** How a client authenticates: explicitly anonymous, or a credential answered whenever the core asks for it. */
export type CredentialSource = "anonymous" | ((ask: CredentialAsk) => Promise<Credential>);

/** The caller's session: what every client of an SDK opened from it authenticates with. */
export interface SdkSession {
  /**
   * Reject, or answer no credential, once the session has ended (sign-out); the core then drops every page it held for this session.
   */
  readonly credential: CredentialSource;
  /** Called when a server refused the credential as unauthenticated and a renewed one did not cure it. */
  readonly onUnauthenticated?: () => void;
}

/** The object the Rust core calls back into: the credential, the unauthenticated notice and the span sink. */
export interface SdkHost {
  readonly anonymous: boolean;
  /** Absent for an anonymous host. Answers the bytes of a `bones.sdk.v1.CredentialAnswer`. */
  readonly credential?: (refresh: boolean, signal: AbortSignal) => Promise<Uint8Array>;
  readonly unauthenticated: () => void;
  readonly start: (bytes: Uint8Array) => Uint8Array;
  readonly end: (bytes: Uint8Array) => void;
}

function message(credential: Credential): CredentialMessage {
  return create(CredentialSchema, { subject: credential.subject, org: credential.org, headers: { ...credential.headers } });
}

/**
 * The host object of one client built from `session`. It answers the core's
 * credential asks as `bones.sdk.v1.CredentialAnswer` bytes: the credential the
 * session answers, in full, every time. The host keeps no state between asks,
 * so one session may back any number of clients and an ask the core abandons
 * leaves nothing behind. Whether a refresh renewed anything is the core's to
 * decide, by comparing what it sent with what the refresh answered. Spans go
 * to `sink`.
 */
export function hostOf(session: SdkSession, sink: JsSpanSink): SdkHost {
  const source = session.credential;
  const common = {
    unauthenticated: (): void => session.onUnauthenticated?.(),
    start: (bytes: Uint8Array): Uint8Array => sink.start(bytes),
    end: (bytes: Uint8Array): void => sink.end(bytes),
  };
  if (typeof source !== "function") return { anonymous: true, ...common };
  return {
    anonymous: false,
    credential: async (refresh, signal) =>
      toBinary(
        CredentialAnswerSchema,
        create(CredentialAnswerSchema, { answer: { case: "credential", value: message(await source({ refresh, signal })) } }),
      ),
    ...common,
  };
}

/** The origin of the page the code runs in, where a client's base address defaults to. */
export function pageOrigin(): string {
  const origin = (globalThis as { location?: { origin?: string } }).location?.origin;
  if (origin === undefined || origin === "" || origin === "null") {
    throw new BonesError({
      code: "VALIDATION_FAILED",
      emitter: "bones",
      platform: "VALIDATION_FAILED",
      connectCode: "invalid_argument",
      message: "no page origin; pass baseUrl",
    });
  }
  return origin;
}
