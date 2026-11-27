// SPDX-License-Identifier: MIT
// ===== ./call-error.d.ts =====
import { type CallError } from "./generated/bones/sdk/v1/boundary_pb.js";
import { BonesError, PageError } from "./errors.js";
/**
 * The error a decoded `bones.sdk.v1.CallError` stands for: a {@link PageError}
 * when it names a page failure, else a {@link BonesError}. An argument that
 * names no member of its oneof is dropped and the error kept; a page failure
 * of an unknown kind is read as a failed call (`rpc`).
 */
export declare function errorOf(message: CallError): BonesError | PageError;
/**
 * The error a caller sees for what a Rust export rejected with. The core
 * rejects with `bones.sdk.v1.CallError` bytes only: those are revived into
 * their error class, and anything else (a host failure, an abort) is given
 * back unchanged.
 */
export declare function mapCallError(rejection: unknown): unknown;
// ===== ./codec.d.ts =====
import { type DescMessage, type MessageShape } from "@bufbuild/protobuf";
import { BonesError } from "./errors.js";
import type { Page } from "./list-call.js";
/** One result of a batch chunk, in request order. */
export type BatchItem<T> = {
    readonly ok: T;
} | {
    readonly error: BonesError;
};
/**
 * The page a handle delivered: the bytes of a `bones.sdk.v1.Page`, each of its
 * items decoded by the item message's schema.
 */
export declare function decodePage<Desc extends DescMessage>(delivered: Uint8Array, schema: Desc): Page<MessageShape<Desc>>;
/**
 * The chunk a handle delivered: the bytes of a `bones.sdk.v1.BatchChunk`, one
 * outcome per requested id in request order, each item decoded by the item
 * message's schema. An outcome that names neither an item nor an error is
 * reported as a failed precondition of that id.
 */
export declare function decodeBatchChunk<Desc extends DescMessage>(bytes: Uint8Array, schema: Desc): BatchItem<MessageShape<Desc>>[];
// ===== ./errors.d.ts =====
/**
 * The error model a generated wrapper raises. The Rust core sorts every
 * failure into these fields; the runtime only revives them into classes.
 */
/** One typed argument of an error message, as the wrapper revives it. */
export type ErrorArg = string | bigint | number | Date;
/** The fields the Rust core reports for a failed call. */
export interface BonesErrorInit {
    /** The emitted error code, such as `GRANT_MISSING`. */
    readonly code: string;
    /** The emitting deployable. */
    readonly emitter: string;
    /** The platform `ErrorCode` the Rust core maps the refusal to, such as `FORBIDDEN`. */
    readonly platform: string;
    /** The Connect code of the refusal, in its lower-case wire form. */
    readonly connectCode: string;
    /** The message arguments, keyed by placeholder name. */
    readonly args?: Readonly<Record<string, ErrorArg>>;
    /** The diagnostic text, for logs. Clients render {@link formatError} instead. */
    readonly message: string;
    /**
     * The whole milliseconds a `resource_exhausted` refusal named in its
     * `retry-after`; absent when it named none.
     */
    readonly retryAfterMs?: number;
}
/** A failed call. A generated wrapper extends it once per service. */
export declare class BonesError extends Error {
    name: string;
    readonly code: string;
    readonly emitter: string;
    readonly platform: string;
    readonly connectCode: string;
    readonly args: Readonly<Record<string, ErrorArg>>;
    /** The wait the refusal named, in milliseconds; `null` when it named none. */
    readonly retryAfterMs: number | null;
    constructor(init: BonesErrorInit);
}
/** Why a page of a list could not be delivered. */
export type PageErrorKind = "cursor_invalid" | "cursor_expired" | "cursor_query_mismatch" | "rpc";
/** The page-level detail the Rust core attaches to a list failure. */
export interface PageDetail {
    readonly kind: PageErrorKind;
    readonly deliveredPages: number;
    readonly deliveredItems: number;
}
/** A list failed part-way: what was delivered before it, and why it stopped. */
export declare class PageError extends BonesError {
    name: string;
    readonly kind: PageErrorKind;
    readonly deliveredPages: number;
    readonly deliveredItems: number;
    constructor(init: BonesErrorInit, page: PageDetail);
}
/** Whether the server refused the list's position rather than the call itself. */
export declare function isCursorRejected(error: unknown): boolean;
// ===== ./format-error.d.ts =====
import type { BonesError } from "./errors.js";
/** The ICU messages of one emitter in one locale, keyed `<emitter>.errors.<CODE>.title` and `.description`. */
export type ErrorCatalog = Readonly<Record<string, string>>;
/**
 * Adds an emitter's messages for one locale. Only keys prefixed `<emitter>.errors.` are kept; the same entries again change nothing, different ones make that emitter's locale unusable.
 */
export declare function registerErrorCatalog(emitter: string, locale: string, messages: ErrorCatalog): void;
/** What {@link formatError} reads of an error. */
export type FormattableError = Pick<BonesError, "emitter" | "code" | "platform" | "args">;
/** What a caller may pin when formatting. */
export interface FormatErrorOptions {
    /** The IANA time zone dates and times render in. Defaults to `UTC`. */
    readonly timeZone?: string;
}
/** The option is not a time zone the platform knows. */
export declare class FormatErrorOptionError extends Error {
    name: string;
    readonly rule: "unknown_time_zone";
    constructor();
}
/** A formatted error: the text and, when its catalog has one, the longer hint. */
export interface FormattedError {
    readonly text: string;
    readonly hint?: string;
}
/**
 * The error's message in `locale`, falling back to `en`, then to the platform text of its code; `text` is empty only while no platform catalog is registered.
 */
export declare function formatError(error: FormattableError, locale: string, options?: FormatErrorOptions): FormattedError;
// ===== ./generated/bones/sdk/v1/boundary_pb.d.ts =====
import type { GenEnum, GenFile, GenMessage } from "@bufbuild/protobuf/codegenv2";
import type { ErrorArg } from "../../v1/errors_pb.js";
import type { Total } from "../../v1/pagination_pb.js";
import type { Duration, Timestamp } from "@bufbuild/protobuf/wkt";
import type { Message } from "@bufbuild/protobuf";
/**
 * Describes the file bones/sdk/v1/boundary.proto.
 */
export declare const file_bones_sdk_v1_boundary: GenFile;
/**
 * A failed call. It is the only value an export rejects with, and the only
 * failure a handle delivers.
 *
 * @generated from message bones.sdk.v1.CallError
 */
export type CallError = Message<"bones.sdk.v1.CallError"> & {
    /**
     * The emitted error code, such as GRANT_MISSING; the platform code when the
     * server named none.
     *
     * @generated from field: string code = 1;
     */
    code: string;
    /**
     * The emitting deployable; empty when the server stamped none.
     *
     * @generated from field: string emitter = 2;
     */
    emitter: string;
    /**
     * The platform ErrorCode, such as FORBIDDEN.
     *
     * @generated from field: string platform = 3;
     */
    platform: string;
    /**
     * The Connect code of the refusal, as its wire name (`invalid_argument`).
     *
     * @generated from field: string connect_code = 4;
     */
    connectCode: string;
    /**
     * The diagnostic text, for logs.
     *
     * @generated from field: string message = 5;
     */
    message: string;
    /**
     * The message arguments, keyed by placeholder name.
     *
     * @generated from field: map<string, bones.v1.ErrorArg> args = 6;
     */
    args: {
        [key: string]: ErrorArg;
    };
    /**
     * Present only when a list failed.
     *
     * @generated from field: bones.sdk.v1.PageFailure page = 7;
     */
    page?: PageFailure | undefined;
    /**
     * The wait a resource_exhausted refusal named in its retry-after.
     *
     * @generated from field: google.protobuf.Duration retry_after = 8;
     */
    retryAfter?: Duration | undefined;
};
/**
 * Describes the message bones.sdk.v1.CallError.
 * Use `create(CallErrorSchema)` to create a new message.
 */
export declare const CallErrorSchema: GenMessage<CallError>;
/**
 * What a list had delivered when it failed, and why it stopped.
 *
 * @generated from message bones.sdk.v1.PageFailure
 */
export type PageFailure = Message<"bones.sdk.v1.PageFailure"> & {
    /**
     * @generated from field: bones.sdk.v1.PageFailure.Kind kind = 1;
     */
    kind: PageFailure_Kind;
    /**
     * Pages handed over before the failure.
     *
     * @generated from field: uint32 delivered_pages = 2;
     */
    deliveredPages: number;
    /**
     * Items handed over before the failure.
     *
     * @generated from field: uint64 delivered_items = 3;
     */
    deliveredItems: bigint;
};
/**
 * Describes the message bones.sdk.v1.PageFailure.
 * Use `create(PageFailureSchema)` to create a new message.
 */
export declare const PageFailureSchema: GenMessage<PageFailure>;
/**
 * @generated from enum bones.sdk.v1.PageFailure.Kind
 */
export declare enum PageFailure_Kind {
    /**
     * @generated from enum value: KIND_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * @generated from enum value: KIND_CURSOR_INVALID = 1;
     */
    CURSOR_INVALID = 1,
    /**
     * @generated from enum value: KIND_CURSOR_EXPIRED = 2;
     */
    CURSOR_EXPIRED = 2,
    /**
     * @generated from enum value: KIND_CURSOR_QUERY_MISMATCH = 3;
     */
    CURSOR_QUERY_MISMATCH = 3,
    /**
     * @generated from enum value: KIND_RPC = 4;
     */
    RPC = 4
}
/**
 * Describes the enum bones.sdk.v1.PageFailure.Kind.
 */
export declare const PageFailure_KindSchema: GenEnum<PageFailure_Kind>;
/**
 * One page of a list. Each item is the encoded item message; a repeated bytes
 * field of encoded messages is byte-identical on the wire to a repeated field
 * of that message, so a host decodes each item by its schema.
 *
 * @generated from message bones.sdk.v1.Page
 */
export type Page = Message<"bones.sdk.v1.Page"> & {
    /**
     * @generated from field: repeated bytes items = 1;
     */
    items: Uint8Array[];
    /**
     * @generated from field: string revision = 2;
     */
    revision: string;
    /**
     * True when the page was served from the revision cache.
     *
     * @generated from field: bool from_cache = 3;
     */
    fromCache: boolean;
    /**
     * @generated from field: bool has_next = 4;
     */
    hasNext: boolean;
    /**
     * @generated from field: bones.v1.Total total = 5;
     */
    total?: Total | undefined;
};
/**
 * Describes the message bones.sdk.v1.Page.
 * Use `create(PageSchema)` to create a new message.
 */
export declare const PageSchema: GenMessage<Page>;
/**
 * One chunk of a batch: one outcome per requested id, in request order.
 *
 * @generated from message bones.sdk.v1.BatchChunk
 */
export type BatchChunk = Message<"bones.sdk.v1.BatchChunk"> & {
    /**
     * @generated from field: repeated bones.sdk.v1.BatchResult results = 1;
     */
    results: BatchResult[];
};
/**
 * Describes the message bones.sdk.v1.BatchChunk.
 * Use `create(BatchChunkSchema)` to create a new message.
 */
export declare const BatchChunkSchema: GenMessage<BatchChunk>;
/**
 * @generated from message bones.sdk.v1.BatchResult
 */
export type BatchResult = Message<"bones.sdk.v1.BatchResult"> & {
    /**
     * @generated from oneof bones.sdk.v1.BatchResult.outcome
     */
    outcome: {
        /**
         * The encoded item message.
         *
         * @generated from field: bytes item = 1;
         */
        value: Uint8Array;
        case: "item";
    } | {
        /**
         * @generated from field: bones.sdk.v1.CallError error = 2;
         */
        value: CallError;
        case: "error";
    } | {
        case: undefined;
        value?: undefined;
    };
};
/**
 * Describes the message bones.sdk.v1.BatchResult.
 * Use `create(BatchResultSchema)` to create a new message.
 */
export declare const BatchResultSchema: GenMessage<BatchResult>;
/**
 * The caller's credential, read by the core before every send.
 *
 * @generated from message bones.sdk.v1.Credential
 */
export type Credential = Message<"bones.sdk.v1.Credential"> & {
    /**
     * @generated from field: string subject = 1;
     */
    subject: string;
    /**
     * @generated from field: string org = 2;
     */
    org: string;
    /**
     * The headers that carry it: authorization, brefwiz-relayed-credential.
     *
     * @generated from field: map<string, string> headers = 3;
     */
    headers: {
        [key: string]: string;
    };
};
/**
 * Describes the message bones.sdk.v1.Credential.
 * Use `create(CredentialSchema)` to create a new message.
 */
export declare const CredentialSchema: GenMessage<Credential>;
/**
 * What a host answers when the core asks for its credential.
 *
 * @generated from message bones.sdk.v1.CredentialAnswer
 */
export type CredentialAnswer = Message<"bones.sdk.v1.CredentialAnswer"> & {
    /**
     * @generated from oneof bones.sdk.v1.CredentialAnswer.answer
     */
    answer: {
        /**
         * @generated from field: bones.sdk.v1.Credential credential = 1;
         */
        value: Credential;
        case: "credential";
    } | {
        case: undefined;
        value?: undefined;
    };
};
/**
 * Describes the message bones.sdk.v1.CredentialAnswer.
 * Use `create(CredentialAnswerSchema)` to create a new message.
 */
export declare const CredentialAnswerSchema: GenMessage<CredentialAnswer>;
/**
 * What the core reports when it opens a span.
 *
 * @generated from message bones.sdk.v1.SpanStart
 */
export type SpanStart = Message<"bones.sdk.v1.SpanStart"> & {
    /**
     * `<package.Service>/<Method>` for an RPC, a descriptive name otherwise.
     *
     * @generated from field: string name = 1;
     */
    name: string;
    /**
     * @generated from field: google.protobuf.Timestamp start_time = 2;
     */
    startTime?: Timestamp | undefined;
    /**
     * @generated from field: bones.sdk.v1.SpanStart.Kind kind = 3;
     */
    kind: SpanStart_Kind;
    /**
     * The parent's traceparent; absent for a new root.
     *
     * @generated from field: optional string parent = 4;
     */
    parent?: string | undefined;
    /**
     * The parent's tracestate.
     *
     * @generated from field: optional string tracestate = 5;
     */
    tracestate?: string | undefined;
    /**
     * @generated from field: map<string, bones.sdk.v1.AttributeValue> attributes = 6;
     */
    attributes: {
        [key: string]: AttributeValue;
    };
};
/**
 * Describes the message bones.sdk.v1.SpanStart.
 * Use `create(SpanStartSchema)` to create a new message.
 */
export declare const SpanStartSchema: GenMessage<SpanStart>;
/**
 * @generated from enum bones.sdk.v1.SpanStart.Kind
 */
export declare enum SpanStart_Kind {
    /**
     * @generated from enum value: KIND_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * @generated from enum value: KIND_CLIENT = 1;
     */
    CLIENT = 1,
    /**
     * @generated from enum value: KIND_INTERNAL = 2;
     */
    INTERNAL = 2
}
/**
 * Describes the enum bones.sdk.v1.SpanStart.Kind.
 */
export declare const SpanStart_KindSchema: GenEnum<SpanStart_Kind>;
/**
 * What the host answers when a span starts.
 *
 * @generated from message bones.sdk.v1.SpanStarted
 */
export type SpanStarted = Message<"bones.sdk.v1.SpanStarted"> & {
    /**
     * @generated from field: string traceparent = 1;
     */
    traceparent: string;
    /**
     * False when the span is not recording: the core still sends the root
     * traceparent and ends the root, but makes no child start or end crossings
     * for the call.
     *
     * @generated from field: bool recording = 2;
     */
    recording: boolean;
};
/**
 * Describes the message bones.sdk.v1.SpanStarted.
 * Use `create(SpanStartedSchema)` to create a new message.
 */
export declare const SpanStartedSchema: GenMessage<SpanStarted>;
/**
 * What the core reports when it ends a span the sink started.
 *
 * @generated from message bones.sdk.v1.SpanEnd
 */
export type SpanEnd = Message<"bones.sdk.v1.SpanEnd"> & {
    /**
     * The value start returned.
     *
     * @generated from field: string traceparent = 1;
     */
    traceparent: string;
    /**
     * @generated from field: google.protobuf.Timestamp end_time = 2;
     */
    endTime?: Timestamp | undefined;
    /**
     * @generated from field: bones.sdk.v1.SpanEnd.Status status = 3;
     */
    status: SpanEnd_Status;
    /**
     * `<connectCode> <code>`, set when status is STATUS_ERROR.
     *
     * @generated from field: optional string error = 4;
     */
    error?: string | undefined;
    /**
     * Attributes known only at the end.
     *
     * @generated from field: map<string, bones.sdk.v1.AttributeValue> attributes = 5;
     */
    attributes: {
        [key: string]: AttributeValue;
    };
};
/**
 * Describes the message bones.sdk.v1.SpanEnd.
 * Use `create(SpanEndSchema)` to create a new message.
 */
export declare const SpanEndSchema: GenMessage<SpanEnd>;
/**
 * @generated from enum bones.sdk.v1.SpanEnd.Status
 */
export declare enum SpanEnd_Status {
    /**
     * @generated from enum value: STATUS_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * @generated from enum value: STATUS_OK = 1;
     */
    OK = 1,
    /**
     * @generated from enum value: STATUS_ERROR = 2;
     */
    ERROR = 2,
    /**
     * The work was abandoned or its outcome is not known.
     *
     * @generated from enum value: STATUS_UNSET = 3;
     */
    UNSET = 3
}
/**
 * Describes the enum bones.sdk.v1.SpanEnd.Status.
 */
export declare const SpanEnd_StatusSchema: GenEnum<SpanEnd_Status>;
/**
 * @generated from message bones.sdk.v1.AttributeValue
 */
export type AttributeValue = Message<"bones.sdk.v1.AttributeValue"> & {
    /**
     * @generated from oneof bones.sdk.v1.AttributeValue.value
     */
    value: {
        /**
         * @generated from field: string string = 1;
         */
        value: string;
        case: "string";
    } | {
        /**
         * @generated from field: int64 int64 = 2;
         */
        value: bigint;
        case: "int64";
    } | {
        /**
         * @generated from field: double double = 3;
         */
        value: number;
        case: "double";
    } | {
        /**
         * @generated from field: bool bool = 4;
         */
        value: boolean;
        case: "bool";
    } | {
        case: undefined;
        value?: undefined;
    };
};
/**
 * Describes the message bones.sdk.v1.AttributeValue.
 * Use `create(AttributeValueSchema)` to create a new message.
 */
export declare const AttributeValueSchema: GenMessage<AttributeValue>;
/**
 * The identity of a built core, and the boundary version it speaks.
 *
 * @generated from message bones.sdk.v1.BuildInfo
 */
export type BuildInfo = Message<"bones.sdk.v1.BuildInfo"> & {
    /**
     * @generated from field: string name = 1;
     */
    name: string;
    /**
     * @generated from field: string version = 2;
     */
    version: string;
    /**
     * The boundary version the core speaks; a wrapper stamped for another
     * version is refused with SDK_BOUNDARY_MISMATCH.
     *
     * @generated from field: uint32 boundary = 3;
     */
    boundary: number;
};
/**
 * Describes the message bones.sdk.v1.BuildInfo.
 * Use `create(BuildInfoSchema)` to create a new message.
 */
export declare const BuildInfoSchema: GenMessage<BuildInfo>;
// ===== ./generated/bones/v1/annotations_pb.d.ts =====
import type { GenEnum, GenExtension, GenFile, GenMessage } from "@bufbuild/protobuf/codegenv2";
import type { EnumValueOptions, FieldOptions, MessageOptions, MethodOptions, ServiceOptions } from "@bufbuild/protobuf/wkt";
import type { Message } from "@bufbuild/protobuf";
/**
 * Describes the file bones/v1/annotations.proto.
 */
export declare const file_bones_v1_annotations: GenFile;
/**
 * One declared argument of an error code: the name a catalog placeholder
 * refers to and the kind of value the emitter always supplies for it.
 *
 * @generated from message bones.v1.ErrorArgDeclaration
 */
export type ErrorArgDeclaration = Message<"bones.v1.ErrorArgDeclaration"> & {
    /**
     * The placeholder name, e.g. `seat` for `{seat}` in the catalog string.
     *
     * @generated from field: string name = 1;
     */
    name: string;
    /**
     * The kind of value carried in `bones.v1.ErrorArg`.
     *
     * @generated from field: bones.v1.ErrorArgKind kind = 2;
     */
    kind: ErrorArgKind;
};
/**
 * Describes the message bones.v1.ErrorArgDeclaration.
 * Use `create(ErrorArgDeclarationSchema)` to create a new message.
 */
export declare const ErrorArgDeclarationSchema: GenMessage<ErrorArgDeclaration>;
/**
 * One capability a service depends on, and the authority it intends to
 * exercise against it.
 *
 * This is a request, not a grant. See the file header.
 *
 * @generated from message bones.v1.RequiredCapability
 */
export type RequiredCapability = Message<"bones.v1.RequiredCapability"> & {
    /**
     * Allocated capability name, in the platform's capability vocabulary.
     * An entry naming nothing declares no dependency and is refused rather
     * than skipped: a dependency the service cannot name is one nothing can
     * bind, and dropping it would let the service serve without it.
     *
     * @generated from field: string name = 1;
     */
    name: string;
    /**
     * Scopes the service intends to exercise against `name`. Empty means it
     * reaches the provider without exercising any named authority — true of a
     * dependency consumed only as transport or attestation.
     *
     * @generated from field: repeated string scopes = 2;
     */
    scopes: string[];
};
/**
 * Describes the message bones.v1.RequiredCapability.
 * Use `create(RequiredCapabilitySchema)` to create a new message.
 */
export declare const RequiredCapabilitySchema: GenMessage<RequiredCapability>;
/**
 * Text a person reads in place of an identifier.
 *
 * @generated from message bones.v1.Label
 */
export type Label = Message<"bones.v1.Label"> & {
    /**
     * The label in the platform's source language, English. Mandatory and
     * non-empty; plain text, one short phrase, never the identifier it labels.
     *
     * @generated from field: string text = 1;
     */
    text: string;
    /**
     * Translations of `text`, keyed by BCP 47 language tag ("fr", "es", "he").
     * A viewer whose language has no entry reads `text`.
     *
     * @generated from field: map<string, string> translations = 2;
     */
    translations: {
        [key: string]: string;
    };
};
/**
 * Describes the message bones.v1.Label.
 * Use `create(LabelSchema)` to create a new message.
 */
export declare const LabelSchema: GenMessage<Label>;
/**
 * The label of one capability this contract provides.
 *
 * @generated from message bones.v1.CapabilityLabel
 */
export type CapabilityLabel = Message<"bones.v1.CapabilityLabel"> & {
    /**
     * A capability this contract provides, exactly as `capability` or
     * `provided_capabilities` names it.
     *
     * @generated from field: string name = 1;
     */
    name: string;
    /**
     * @generated from field: bones.v1.Label label = 2;
     */
    label?: Label | undefined;
};
/**
 * Describes the message bones.v1.CapabilityLabel.
 * Use `create(CapabilityLabelSchema)` to create a new message.
 */
export declare const CapabilityLabelSchema: GenMessage<CapabilityLabel>;
/**
 * The label of one scope this contract's RPCs require or it declares in
 * `provided_broker_scopes`.
 *
 * @generated from message bones.v1.PermissionLabel
 */
export type PermissionLabel = Message<"bones.v1.PermissionLabel"> & {
    /**
     * A scope named in some `authz.requires` or `provided_broker_scopes` entry
     * of this contract, byte-identical.
     *
     * @generated from field: string scope = 1;
     */
    scope: string;
    /**
     * @generated from field: bones.v1.Label label = 2;
     */
    label?: Label | undefined;
};
/**
 * Describes the message bones.v1.PermissionLabel.
 * Use `create(PermissionLabelSchema)` to create a new message.
 */
export declare const PermissionLabelSchema: GenMessage<PermissionLabel>;
/**
 * One scope of a provided capability that no RPC requires and a broker
 * enforces.
 *
 * @generated from message bones.v1.BrokerScope
 */
export type BrokerScope = Message<"bones.v1.BrokerScope"> & {
    /**
     * A capability this contract provides, exactly as `capability` or
     * `provided_capabilities` names it.
     *
     * @generated from field: string capability = 1;
     */
    capability: string;
    /**
     * The CanonicalClaim scope, carrying no subject: `<ns>:events:subscribe`
     * for the provider's own events, `<ns>:events:<topic>:subscribe` for one
     * live-only topic of them, `<ns>:events:<topic>:subscribe:own` for one
     * topic of the holder's own entity's events only, or `<ns>:<right>` for a
     * broker's own right.
     * The file header holds its forms and the complete refusal list.
     *
     * @generated from field: string scope = 2;
     */
    scope: string;
    /**
     * A capability that implies this scope. Applies only to the own-events
     * form `<ns>:events:<topic>:subscribe:own`, and names a capability the same
     * provider provides. When set, every workload that holds the named
     * capability, through its own standing grant or its consumption, is granted
     * this broker scope without a grant record.
     * Only while the holder holds that capability; the scope drops at the next
     * mint once it is withdrawn.
     * Empty: nothing is implied; the scope is granted only by consuming
     * `capability` or by an explicit grant. Names a capability by the same
     * grammar as `capability`.
     *
     * @generated from field: string implied_by = 3;
     */
    impliedBy: string;
};
/**
 * Describes the message bones.v1.BrokerScope.
 * Use `create(BrokerScopeSchema)` to create a new message.
 */
export declare const BrokerScopeSchema: GenMessage<BrokerScope>;
/**
 * One permission group: a provided capability and the scopes it confers.
 *
 * @generated from message bones.v1.CapabilityScopes
 */
export type CapabilityScopes = Message<"bones.v1.CapabilityScopes"> & {
    /**
     * A capability this contract provides, exactly as `capability` or
     * `provided_capabilities` names it.
     *
     * @generated from field: string capability = 1;
     */
    capability: string;
    /**
     * The scopes holding the capability confers, byte-identical to how
     * `authz.requires` or `provided_broker_scopes` names them.
     *
     * @generated from field: repeated string scopes = 2;
     */
    scopes: string[];
};
/**
 * Describes the message bones.v1.CapabilityScopes.
 * Use `create(CapabilityScopesSchema)` to create a new message.
 */
export declare const CapabilityScopesSchema: GenMessage<CapabilityScopes>;
/**
 * One provided capability flagged as conferring approval of a class of
 * action.
 *
 * @generated from message bones.v1.ApprovalCapability
 */
export type ApprovalCapability = Message<"bones.v1.ApprovalCapability"> & {
    /**
     * A capability this contract provides, exactly as `capability` or
     * `provided_capabilities` names it. It is also the name of the action class
     * its holders approve.
     *
     * @generated from field: string capability = 1;
     */
    capability: string;
    /**
     * What the class is called to a person who picks approvers: "Approve
     * refunds", never the identifier. `label.text` is mandatory and non-empty.
     *
     * @generated from field: bones.v1.Label label = 2;
     */
    label?: Label | undefined;
};
/**
 * Describes the message bones.v1.ApprovalCapability.
 * Use `create(ApprovalCapabilitySchema)` to create a new message.
 */
export declare const ApprovalCapabilitySchema: GenMessage<ApprovalCapability>;
/**
 * Authorization declaration attached to an RPC.
 *
 * @generated from message bones.v1.AuthzRule
 */
export type AuthzRule = Message<"bones.v1.AuthzRule"> & {
    /**
     * Whether a credential is required. See `AuthzKind`.
     *
     * @generated from field: bones.v1.AuthzKind kind = 1;
     */
    kind: AuthzKind;
    /**
     * Why the RPC is unauthenticated. Mandatory and non-empty for
     * AUTHZ_KIND_PUBLIC; ignored otherwise.
     *
     * @generated from field: string reason = 2;
     */
    reason: string;
    /**
     * CanonicalClaim scopes the caller must hold. Every listed scope
     * must be present — the list is a conjunction, so adding one can
     * only ever narrow who may call.
     *
     * Mandatory and non-empty for AUTHZ_KIND_AUTHENTICATED: an
     * authenticated RPC that names no scope would admit every
     * authenticated caller on the platform, which is the failure this
     * option exists to prevent. Must be empty for AUTHZ_KIND_PUBLIC,
     * which has no credential to read scopes from.
     *
     * @generated from field: repeated string requires = 3;
     */
    requires: string[];
};
/**
 * Describes the message bones.v1.AuthzRule.
 * Use `create(AuthzRuleSchema)` to create a new message.
 */
export declare const AuthzRuleSchema: GenMessage<AuthzRule>;
/**
 * One resource attribute a credential can be narrowed on at a service.
 *
 * @generated from message bones.v1.AttributeDimension
 */
export type AttributeDimension = Message<"bones.v1.AttributeDimension"> & {
    /**
     * lower_snake_case, unique within the service.
     *
     * @generated from field: string name = 1;
     */
    name: string;
    /**
     * @generated from field: bones.v1.AttributeType type = 2;
     */
    type: AttributeType;
    /**
     * UNSPECIFIED is invalid.
     *
     * @generated from field: bones.v1.AttributePolarity polarity = 3;
     */
    polarity: AttributePolarity;
    /**
     * UNSPECIFIED and EXACT both mean exact.
     *
     * @generated from field: bones.v1.AttributeMatching matching = 4;
     */
    matching: AttributeMatching;
    /**
     * Required if and only if `matching` is SEGMENT_PREFIX, forbidden
     * otherwise. Exactly one character, not whitespace, `*`, `?` or `>`.
     *
     * @generated from field: string separator = 5;
     */
    separator: string;
    /**
     * Human label of the dimension, per locale. Consumers render it and
     * never fall back to `name`.
     *
     * @generated from field: bones.v1.Label label = 6;
     */
    label?: Label | undefined;
};
/**
 * Describes the message bones.v1.AttributeDimension.
 * Use `create(AttributeDimensionSchema)` to create a new message.
 */
export declare const AttributeDimensionSchema: GenMessage<AttributeDimension>;
/**
 * The MCP projection of one RPC. The shape of the call is
 * `bones.v1.call_shape`, not a field here.
 *
 * @generated from message bones.v1.McpProjection
 */
export type McpProjection = Message<"bones.v1.McpProjection"> & {
    /**
     * Optional human title. Name and description come from the RPC and its
     * comments.
     *
     * @generated from field: string title = 2;
     */
    title: string;
};
/**
 * Describes the message bones.v1.McpProjection.
 * Use `create(McpProjectionSchema)` to create a new message.
 */
export declare const McpProjectionSchema: GenMessage<McpProjection>;
/**
 * A message declared as a managed resource.
 *
 * @generated from message bones.v1.ResourceOption
 */
export type ResourceOption = Message<"bones.v1.ResourceOption"> & {
    /**
     * The Terraform noun, lower_snake_case.
     *
     * @generated from field: string type = 1;
     */
    type: string;
    /**
     * The field of the message that identifies an instance.
     *
     * @generated from field: string identity_field = 2;
     */
    identityField: string;
    /**
     * Per-verb RPC overrides.
     *
     * @generated from field: bones.v1.ResourceMethods methods = 3;
     */
    methods?: ResourceMethods | undefined;
};
/**
 * Describes the message bones.v1.ResourceOption.
 * Use `create(ResourceOptionSchema)` to create a new message.
 */
export declare const ResourceOptionSchema: GenMessage<ResourceOption>;
/**
 * The RPCs implementing each verb of a resource. Each is the name of a method
 * in the same service; empty means the AIP convention `<Verb><Message>`, and
 * `List<Message>s` for list.
 *
 * @generated from message bones.v1.ResourceMethods
 */
export type ResourceMethods = Message<"bones.v1.ResourceMethods"> & {
    /**
     * @generated from field: string create = 1;
     */
    create: string;
    /**
     * @generated from field: string get = 2;
     */
    get: string;
    /**
     * @generated from field: string update = 3;
     */
    update: string;
    /**
     * @generated from field: string delete = 4;
     */
    delete: string;
    /**
     * @generated from field: string list = 5;
     */
    list: string;
};
/**
 * Describes the message bones.v1.ResourceMethods.
 * Use `create(ResourceMethodsSchema)` to create a new message.
 */
export declare const ResourceMethodsSchema: GenMessage<ResourceMethods>;
/**
 * A domain event declared as a trigger.
 *
 * @generated from message bones.v1.AutomationTrigger
 */
export type AutomationTrigger = Message<"bones.v1.AutomationTrigger"> & {
    /**
     * What a person picking a trigger reads, in domain terms. Mandatory.
     *
     * @generated from field: bones.v1.Label label = 1;
     */
    label?: Label | undefined;
};
/**
 * Describes the message bones.v1.AutomationTrigger.
 * Use `create(AutomationTriggerSchema)` to create a new message.
 */
export declare const AutomationTriggerSchema: GenMessage<AutomationTrigger>;
/**
 * A public RPC declared as an action.
 *
 * @generated from message bones.v1.AutomationAction
 */
export type AutomationAction = Message<"bones.v1.AutomationAction"> & {
    /**
     * What a person picking a step reads, in domain terms. Mandatory.
     *
     * @generated from field: bones.v1.Label label = 1;
     */
    label?: Label | undefined;
    /**
     * What running the action changes. UNSPECIFIED is refused.
     *
     * @generated from field: bones.v1.ActionEffect effect = 2;
     */
    effect: ActionEffect;
    /**
     * How a repeat of one business operation behaves. Mandatory unless the
     * effect is PURE, which is natural by definition.
     *
     * @generated from field: bones.v1.IdempotencyMode idempotency = 3;
     */
    idempotency: IdempotencyMode;
    /**
     * Top-level request fields that identify one business operation. Required
     * and non-empty if and only if `idempotency` is KEYED.
     *
     * @generated from field: repeated string idempotency_key_fields = 4;
     */
    idempotencyKeyFields: string[];
    /**
     * Only a person's own signed press runs this action; no automation step
     * may call it.
     *
     * @generated from field: bool person_reserved = 5;
     */
    personReserved: boolean;
    /**
     * The action changes state the product's planner owns, and the product
     * pins an automation's change so its planner respects it.
     *
     * @generated from field: bool planner_pinned = 6;
     */
    plannerPinned: boolean;
    /**
     * Set on a read action that finds instances of one kind of entity, so a
     * form renders a parameter of that kind as a picker over this action.
     *
     * @generated from field: bones.v1.EntityResolver resolves = 7;
     */
    resolves?: EntityResolver | undefined;
};
/**
 * Describes the message bones.v1.AutomationAction.
 * Use `create(AutomationActionSchema)` to create a new message.
 */
export declare const AutomationActionSchema: GenMessage<AutomationAction>;
/**
 * How a read action turns a person's search text into candidates. Every path
 * is a dot-separated list of proto field names.
 *
 * @generated from message bones.v1.EntityResolver
 */
export type EntityResolver = Message<"bones.v1.EntityResolver"> & {
    /**
     * The top-level string field of the request that takes the search text.
     *
     * @generated from field: string query_field = 1;
     */
    queryField: string;
    /**
     * The repeated message field of the response that holds the candidates.
     *
     * @generated from field: string results_field = 2;
     */
    resultsField: string;
    /**
     * The field of a candidate whose value a binding receives.
     *
     * @generated from field: string value_field = 3;
     */
    valueField: string;
    /**
     * The field of a candidate a person reads.
     *
     * @generated from field: string label_field = 4;
     */
    labelField: string;
};
/**
 * Describes the message bones.v1.EntityResolver.
 * Use `create(EntityResolverSchema)` to create a new message.
 */
export declare const EntityResolverSchema: GenMessage<EntityResolver>;
/**
 * What a string field's value refers to.
 *
 * @generated from message bones.v1.FieldReference
 */
export type FieldReference = Message<"bones.v1.FieldReference"> & {
    /**
     * @generated from oneof bones.v1.FieldReference.target
     */
    target: {
        /**
         * The referenced field, `<package>.<Message>.<field>`, on a
         * `bones.v1.resource` message; the same format as `defaulted_from`.
         *
         * @generated from field: string field = 1;
         */
        value: string;
        case: "field";
    } | {
        /**
         * Declares the value names a principal id that must belong to the
         * caller's organization; a hint only, grants nothing. The server
         * verifies membership and authorizes the write; no consumer auto-fills
         * or trusts it.
         *
         * @generated from field: bool principal = 2;
         */
        value: boolean;
        case: "principal";
    } | {
        case: undefined;
        value?: undefined;
    };
};
/**
 * Describes the message bones.v1.FieldReference.
 * Use `create(FieldReferenceSchema)` to create a new message.
 */
export declare const FieldReferenceSchema: GenMessage<FieldReference>;
/**
 * The kind of an error argument, matching the member of `bones.v1.ErrorArg`
 * that carries it. The members are prefixed because proto enum values share
 * their enum's scope.
 *
 * @generated from enum bones.v1.ErrorArgKind
 */
export declare enum ErrorArgKind {
    /**
     * No declared kind. A declaration with it is refused.
     *
     * @generated from enum value: ERROR_ARG_KIND_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * `ErrorArg.string`: text, for `{x}` and `{x, select, ...}`.
     *
     * @generated from enum value: ERROR_ARG_KIND_STRING = 1;
     */
    STRING = 1,
    /**
     * `ErrorArg.int64`: a whole number, for `{x, number}` and `{x, plural, ...}`.
     *
     * @generated from enum value: ERROR_ARG_KIND_INT64 = 2;
     */
    INT64 = 2,
    /**
     * `ErrorArg.double`: a real number, for `{x, number}` and `{x, plural, ...}`.
     *
     * @generated from enum value: ERROR_ARG_KIND_DOUBLE = 3;
     */
    DOUBLE = 3,
    /**
     * `ErrorArg.timestamp`: an instant, for `{x, date, ...}` and `{x, time, ...}`.
     *
     * @generated from enum value: ERROR_ARG_KIND_TIMESTAMP = 4;
     */
    TIMESTAMP = 4
}
/**
 * Describes the enum bones.v1.ErrorArgKind.
 */
export declare const ErrorArgKindSchema: GenEnum<ErrorArgKind>;
/**
 * Who a product's capability is granted to when an organization enables the
 * product. For values 1 to 3 each value includes every role of the values
 * before it. Value 4 is not ordinal: it sits outside that ordering, and readers
 * never compare audience values numerically.
 *
 * @generated from enum bones.v1.CapabilityAudience
 */
export declare enum CapabilityAudience {
    /**
     * No declaration. The capability is never part of a default grant.
     *
     * @generated from enum value: CAPABILITY_AUDIENCE_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * Granted to the `admin` role.
     *
     * @generated from enum value: CAPABILITY_AUDIENCE_ADMINISTRATORS = 1;
     */
    ADMINISTRATORS = 1,
    /**
     * Granted to the `admin` and `member` roles.
     *
     * @generated from enum value: CAPABILITY_AUDIENCE_MEMBERS = 2;
     */
    MEMBERS = 2,
    /**
     * Granted to the `admin`, `member` and `viewer` roles.
     *
     * @generated from enum value: CAPABILITY_AUDIENCE_EVERYONE = 3;
     */
    EVERYONE = 3,
    /**
     * Granted to admin and member, but only through a product that consumes it,
     * bound to that product's workload-facing server; never by the provider's own
     * enablement nor by hand.
     *
     * @generated from enum value: CAPABILITY_AUDIENCE_MEMBERS_RELAYED = 4;
     */
    MEMBERS_RELAYED = 4
}
/**
 * Describes the enum bones.v1.CapabilityAudience.
 */
export declare const CapabilityAudienceSchema: GenEnum<CapabilityAudience>;
/**
 * Whether an RPC requires a credential at all.
 *
 * This is the only authorization question answerable before a
 * credential is parsed, which is why it is a declaration rather than a
 * scope. Everything else an RPC requires is expressed in
 * `AuthzRule.requires`.
 *
 * @generated from enum bones.v1.AuthzKind
 */
export declare enum AuthzKind {
    /**
     * No declaration. Enforcement denies the RPC — the option must name
     * a kind explicitly for the RPC to be reachable.
     *
     * @generated from enum value: AUTHZ_KIND_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * Served without authentication. `reason` is mandatory and must be
     * non-empty: an unauthenticated route is a decision that has to be
     * justified in the contract where reviewers can see it.
     *
     * Kept at its original pre-migration number (3) deliberately: a
     * stale vendored copy that still declares the old principal-class
     * enum will resolve this value identically under both schemas,
     * keeping genuinely-public declarations working across a
     * stale/new vendor mismatch instead of breaking them.
     *
     * @generated from enum value: AUTHZ_KIND_PUBLIC = 3;
     */
    PUBLIC = 3,
    /**
     * Requires a verified credential, plus every scope named in
     * `AuthzRule.requires`. The principal's class is irrelevant: a
     * workload, a human, and a delegated on-behalf-of token are all
     * admitted on identical terms if they hold the required scopes.
     *
     * @generated from enum value: AUTHZ_KIND_AUTHENTICATED = 4;
     */
    AUTHENTICATED = 4
}
/**
 * Describes the enum bones.v1.AuthzKind.
 */
export declare const AuthzKindSchema: GenEnum<AuthzKind>;
/**
 * The type of values a dimension carries.
 *
 * @generated from enum bones.v1.AttributeType
 */
export declare enum AttributeType {
    /**
     * No declaration. Invalid on a declared dimension.
     *
     * @generated from enum value: ATTRIBUTE_TYPE_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * @generated from enum value: ATTRIBUTE_TYPE_STRING = 1;
     */
    STRING = 1,
    /**
     * @generated from enum value: ATTRIBUTE_TYPE_UUID = 2;
     */
    UUID = 2
}
/**
 * Describes the enum bones.v1.AttributeType.
 */
export declare const AttributeTypeSchema: GenEnum<AttributeType>;
/**
 * What an absent restriction on a dimension means.
 *
 * @generated from enum bones.v1.AttributePolarity
 */
export declare enum AttributePolarity {
    /**
     * No declaration. Invalid.
     *
     * @generated from enum value: ATTRIBUTE_POLARITY_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * Nothing is permitted until the authority names a value; absent denies.
     *
     * @generated from enum value: ATTRIBUTE_POLARITY_GRANTED = 1;
     */
    GRANTED = 1,
    /**
     * Everything the scopes reach is permitted until a restriction names
     * values; absent permits.
     *
     * @generated from enum value: ATTRIBUTE_POLARITY_NARROWABLE = 2;
     */
    NARROWABLE = 2
}
/**
 * Describes the enum bones.v1.AttributePolarity.
 */
export declare const AttributePolaritySchema: GenEnum<AttributePolarity>;
/**
 * How a restriction value is compared with a request value.
 *
 * @generated from enum bones.v1.AttributeMatching
 */
export declare enum AttributeMatching {
    /**
     * Same as EXACT.
     *
     * @generated from enum value: ATTRIBUTE_MATCHING_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * @generated from enum value: ATTRIBUTE_MATCHING_EXACT = 1;
     */
    EXACT = 1,
    /**
     * A value is a list of segments split on `separator`; a prefix covers every
     * value beneath it.
     *
     * @generated from enum value: ATTRIBUTE_MATCHING_SEGMENT_PREFIX = 2;
     */
    SEGMENT_PREFIX = 2
}
/**
 * Describes the enum bones.v1.AttributeMatching.
 */
export declare const AttributeMatchingSchema: GenEnum<AttributeMatching>;
/**
 * What kind of call an RPC is.
 *
 * @generated from enum bones.v1.CallShape
 */
export declare enum CallShape {
    /**
     * No declaration. Derived from the signature: unary and client stream. A
     * server stream is refused until it declares one of the values below.
     *
     * @generated from enum value: CALL_SHAPE_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * One request, one response.
     *
     * @generated from enum value: CALL_SHAPE_UNARY = 1;
     */
    UNARY = 1,
    /**
     * A stream of requests, one response.
     *
     * @generated from enum value: CALL_SHAPE_CLIENT_STREAM = 2;
     */
    CLIENT_STREAM = 2,
    /**
     * A server stream that ends.
     *
     * @generated from enum value: CALL_SHAPE_FINITE_STREAM = 3;
     */
    FINITE_STREAM = 3,
    /**
     * A server stream with no end: a subscription.
     *
     * @generated from enum value: CALL_SHAPE_UNBOUNDED_STREAM = 4;
     */
    UNBOUNDED_STREAM = 4,
    /**
     * A long-running operation: an operation handle that completes later.
     *
     * @generated from enum value: CALL_SHAPE_LONG_RUNNING = 5;
     */
    LONG_RUNNING = 5
}
/**
 * Describes the enum bones.v1.CallShape.
 */
export declare const CallShapeSchema: GenEnum<CallShape>;
/**
 * What running an action changes in the world.
 *
 * @generated from enum bones.v1.ActionEffect
 */
export declare enum ActionEffect {
    /**
     * No declaration. Refused.
     *
     * @generated from enum value: ACTION_EFFECT_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * No effect outside the action's own result: a read action.
     *
     * @generated from enum value: ACTION_EFFECT_PURE = 1;
     */
    PURE = 1,
    /**
     * An effect confined to a scope that is discarded, never compensated.
     *
     * @generated from enum value: ACTION_EFFECT_EPHEMERAL = 2;
     */
    EPHEMERAL = 2,
    /**
     * A lasting effect the product can undo.
     *
     * @generated from enum value: ACTION_EFFECT_COMPENSABLE = 3;
     */
    COMPENSABLE = 3,
    /**
     * A lasting effect nothing undoes: an irreversible action.
     *
     * @generated from enum value: ACTION_EFFECT_POINT_OF_NO_RETURN = 4;
     */
    POINT_OF_NO_RETURN = 4
}
/**
 * Describes the enum bones.v1.ActionEffect.
 */
export declare const ActionEffectSchema: GenEnum<ActionEffect>;
/**
 * How a repeated invocation of one business operation behaves.
 *
 * @generated from enum bones.v1.IdempotencyMode
 */
export declare enum IdempotencyMode {
    /**
     * No declaration. Refused unless the effect is PURE.
     *
     * @generated from enum value: IDEMPOTENCY_MODE_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * Repeating the action is harmless by nature.
     *
     * @generated from enum value: IDEMPOTENCY_MODE_NATURAL = 1;
     */
    NATURAL = 1,
    /**
     * Repeating with the same values of `idempotency_key_fields` is
     * deduplicated by the product.
     *
     * @generated from enum value: IDEMPOTENCY_MODE_KEYED = 2;
     */
    KEYED = 2
}
/**
 * Describes the enum bones.v1.IdempotencyMode.
 */
export declare const IdempotencyModeSchema: GenEnum<IdempotencyMode>;
/**
 * How sensitive a field's data is. Each value is more sensitive than the
 * values before it.
 *
 * @generated from enum bones.v1.DataClassification
 */
export declare enum DataClassification {
    /**
     * Unclassified. Never a classification: a check refuses a covered field
     * in this state, and it reads the same as the option being absent.
     *
     * @generated from enum value: DATA_CLASSIFICATION_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * Anyone may see it.
     *
     * @generated from enum value: DATA_CLASSIFICATION_PUBLIC = 1;
     */
    PUBLIC = 1,
    /**
     * Any member of the organization may see it.
     *
     * @generated from enum value: DATA_CLASSIFICATION_INTERNAL = 2;
     */
    INTERNAL = 2,
    /**
     * Only those the organization authorizes may see it; personal data is at
     * least this.
     *
     * @generated from enum value: DATA_CLASSIFICATION_CONFIDENTIAL = 3;
     */
    CONFIDENTIAL = 3,
    /**
     * Credentials, payment and special-category data; never leaves the
     * product except to destinations cleared for it.
     *
     * @generated from enum value: DATA_CLASSIFICATION_RESTRICTED = 4;
     */
    RESTRICTED = 4
}
/**
 * Describes the enum bones.v1.DataClassification.
 */
export declare const DataClassificationSchema: GenEnum<DataClassification>;
/**
 * The format a string field's value takes.
 *
 * @generated from enum bones.v1.ValueFormat
 */
export declare enum ValueFormat {
    /**
     * No declared format.
     *
     * @generated from enum value: VALUE_FORMAT_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * A UUID.
     *
     * @generated from enum value: VALUE_FORMAT_UUID = 1;
     */
    UUID = 1,
    /**
     * An absolute URI per RFC 3986: it has a scheme. A syntax declaration
     * only; implies no scheme allowlist and no fetch. A consumer that
     * dereferences it applies its own scheme/host allowlist (SSRF).
     *
     * @generated from enum value: VALUE_FORMAT_URI = 2;
     */
    URI = 2
}
/**
 * Describes the enum bones.v1.ValueFormat.
 */
export declare const ValueFormatSchema: GenEnum<ValueFormat>;
/**
 * Singular predecessor of `provided_capabilities`, still read. Empty or
 * absent means this service is not enrolled as a capability provider.
 *
 * @generated from extension: string capability = 5102349;
 */
export declare const capability: GenExtension<ServiceOptions, string>;
/**
 * Capabilities this service must have bound before it can serve, with
 * the authority it intends to exercise against each. Absent means it
 * requires none. See file header for semantics.
 *
 * @generated from extension: repeated bones.v1.RequiredCapability required_capabilities = 5102350;
 */
export declare const required_capabilities: GenExtension<ServiceOptions, RequiredCapability[]>;
/**
 * Allocated capabilities this service serves. Naming one does not allocate
 * it. Absent means this service is not enrolled as a capability provider.
 * See file header for semantics.
 *
 * @generated from extension: repeated string provided_capabilities = 5102351;
 */
export declare const provided_capabilities: GenExtension<ServiceOptions, string[]>;
/**
 * Human labels for the capabilities this contract provides. See file header.
 *
 * @generated from extension: repeated bones.v1.CapabilityLabel capability_labels = 5102352;
 */
export declare const capability_labels: GenExtension<ServiceOptions, CapabilityLabel[]>;
/**
 * Human labels for every scope this contract's RPCs require or it declares
 * in `provided_broker_scopes`. See file header.
 *
 * @generated from extension: repeated bones.v1.PermissionLabel permission_labels = 5102353;
 */
export declare const permission_labels: GenExtension<ServiceOptions, PermissionLabel[]>;
/**
 * Scopes of a provided capability that a broker, not an RPC, enforces. See
 * file header.
 *
 * @generated from extension: repeated bones.v1.BrokerScope provided_broker_scopes = 5102354;
 */
export declare const provided_broker_scopes: GenExtension<ServiceOptions, BrokerScope[]>;
/**
 * Organization roles this service's capabilities are granted to when an
 * organization enables the product. Absent or UNSPECIFIED means never part
 * of a default grant. See file header.
 *
 * @generated from extension: bones.v1.CapabilityAudience capability_audience = 5102355;
 */
export declare const capability_audience: GenExtension<ServiceOptions, CapabilityAudience>;
/**
 * Task-level permission groups: each names a provided capability and the
 * scopes it confers. See file header.
 *
 * @generated from extension: repeated bones.v1.CapabilityScopes capability_scopes = 5102356;
 */
export declare const capability_scopes: GenExtension<ServiceOptions, CapabilityScopes[]>;
/**
 * Resource attribute dimensions a credential can be narrowed on at this
 * service. Absent means none. See file header.
 *
 * @generated from extension: repeated bones.v1.AttributeDimension attributes = 5102358;
 */
export declare const attributes: GenExtension<ServiceOptions, AttributeDimension[]>;
/**
 * Provided capabilities whose holders approve a class of action, each with
 * a human label. See file header.
 *
 * @generated from extension: repeated bones.v1.ApprovalCapability approval_capabilities = 5102376;
 */
export declare const approval_capabilities: GenExtension<ServiceOptions, ApprovalCapability[]>;
/**
 * Declared error variants for this RPC. Variant names are
 * ALL_CAPS_SNAKE and must match a value in the service's
 * companion error enum. See file header for full semantics.
 *
 * @generated from extension: repeated string errors = 5102347;
 */
export declare const errors: GenExtension<MethodOptions, string[]>;
/**
 * Declared authorization requirement for this RPC. Absent option
 * means the RPC is denied to every caller. See file header.
 *
 * @generated from extension: bones.v1.AuthzRule authz = 5102348;
 */
export declare const authz: GenExtension<MethodOptions, AuthzRule>;
/**
 * Opts this RPC into the MCP projection. Absent means not projected. See
 * file header.
 *
 * @generated from extension: bones.v1.McpProjection mcp = 5102359;
 */
export declare const mcp: GenExtension<MethodOptions, McpProjection>;
/**
 * What kind of call this RPC is. Absent or UNSPECIFIED means derived from
 * the signature. Read by every projection. See file header.
 *
 * @generated from extension: bones.v1.CallShape call_shape = 5102362;
 */
export declare const call_shape: GenExtension<MethodOptions, CallShape>;
/**
 * The single-item method of the same service this RPC batches. Absent means
 * the RPC is not a batch form. See file header.
 *
 * @generated from extension: string batch_of = 5102367;
 */
export declare const batch_of: GenExtension<MethodOptions, string>;
/**
 * Declares this RPC an action of the product's automation surface. Absent
 * means organizations cannot automate it. See file header.
 *
 * @generated from extension: bones.v1.AutomationAction automation_action = 5102364;
 */
export declare const automation_action: GenExtension<MethodOptions, AutomationAction>;
/**
 * The unary List RPC of the same service whose request this server stream
 * accepts and whose items it streams. Absent means the RPC is not a list
 * stream. See file header.
 *
 * @generated from extension: string list_stream_of = 5102371;
 */
export declare const list_stream_of: GenExtension<MethodOptions, string>;
/**
 * The request carries the caller's top-level `string idempotency_key` and
 * the call is IDEMPOTENT under it, single or batch; the marking service
 * must record and replay the first answer per organization, workload, relayed
 * person and method.
 * Absent or false means the RPC takes no key. See file header.
 *
 * @generated from extension: bool idempotency_key = 5102375;
 */
export declare const idempotency_key: GenExtension<MethodOptions, boolean>;
/**
 * Declares this message a managed resource. See file header.
 *
 * @generated from extension: bones.v1.ResourceOption resource = 5102361;
 */
export declare const resource: GenExtension<MessageOptions, ResourceOption>;
/**
 * Declares this domain-event message a trigger of the product's automation
 * surface. Absent means no automation starts from it. See file header.
 *
 * @generated from extension: bones.v1.AutomationTrigger automation_trigger = 5102363;
 */
export declare const automation_trigger: GenExtension<MessageOptions, AutomationTrigger>;
/**
 * The fully-qualified name of the message this message is instantiated
 * from. Absent means the message is not an instance of another message. See
 * file header.
 *
 * @generated from extension: string instance_of = 5102369;
 */
export declare const instance_of: GenExtension<MessageOptions, string>;
/**
 * The attribute dimension this field carries, naming one declared in the
 * service's `attributes`. See file header.
 *
 * @generated from extension: string attr = 5102360;
 */
export declare const attr: GenExtension<FieldOptions, string>;
/**
 * How sensitive this field's data is. Absent reads as
 * DATA_CLASSIFICATION_UNSPECIFIED, the unclassified state. See file header.
 *
 * @generated from extension: bones.v1.DataClassification classification = 5102365;
 */
export declare const classification: GenExtension<FieldOptions, DataClassification>;
/**
 * What this field denotes, as one term of the curated vocabulary (schema.org
 * core plus the extensions the platform curates), written with its
 * `schema:` prefix: `schema:startDate`. See file header.
 *
 * @generated from extension: string meaning = 5102368;
 */
export declare const meaning: GenExtension<FieldOptions, string>;
/**
 * The fully-qualified field (`<package>.<Message>.<field>`) on the
 * `instance_of` message that proposes this field's default. The value stays
 * overridable; absent means the field has no proposed default. See file
 * header.
 *
 * @generated from extension: string defaulted_from = 5102370;
 */
export declare const defaulted_from: GenExtension<FieldOptions, string>;
/**
 * F1: The sibling string field holding the definition id that types this
 * bones.v1.TypedValue. Every TypedValue field names its definition this way.
 *
 * @generated from extension: string typed_by = 5102372;
 */
export declare const typed_by: GenExtension<FieldOptions, string>;
/**
 * The single child write of the same service that each child in this
 * field passes through when it is written with its parent. Absent means
 * the field is not a nested write. See file header.
 *
 * @generated from extension: string nested_write_of = 5102373;
 */
export declare const nested_write_of: GenExtension<FieldOptions, string>;
/**
 * The format this string field's value takes. Absent reads as
 * VALUE_FORMAT_UNSPECIFIED, no declared format. On a repeated field it
 * applies to each element.
 *
 * @generated from extension: bones.v1.ValueFormat format = 5102377;
 */
export declare const format: GenExtension<FieldOptions, ValueFormat>;
/**
 * What this string field's value refers to. Absent means the field refers
 * to nothing declared. On a repeated field it applies to each element.
 *
 * @generated from extension: bones.v1.FieldReference references = 5102378;
 */
export declare const references: GenExtension<FieldOptions, FieldReference>;
/**
 * This enum value is output-only: the service reports it, but no request
 * may set it. A consumer that builds or validates a request, such as a
 * Terraform provider's schema or a conformance sampler, leaves it out of
 * the values a caller may choose; one that decodes a response still accepts
 * it. Absent reads as false: the value is settable.
 *
 * @generated from extension: bool output_only = 5102379;
 */
export declare const output_only: GenExtension<EnumValueOptions, boolean>;
/**
 * The typed arguments of this error code's message, in the order the code's
 * owner declares them. Goes on the value of the companion error enum that
 * `bones.v1.errors` names, so the code and its arguments are one
 * declaration. The code's catalog strings may use exactly these
 * placeholders, each with a compatible kind, and the `error-catalog` check
 * refuses a string that does not. Absent reads as no arguments. A name is
 * unique within the value and follows the `ErrorArg` key grammar
 * `^[a-z][a-z0-9_]{0,63}$`.
 *
 * @generated from extension: repeated bones.v1.ErrorArgDeclaration error_args = 5102380;
 */
export declare const error_args: GenExtension<EnumValueOptions, ErrorArgDeclaration[]>;
// ===== ./generated/bones/v1/errors_pb.d.ts =====
import type { GenFile, GenMessage } from "@bufbuild/protobuf/codegenv2";
import type { Timestamp } from "@bufbuild/protobuf/wkt";
import type { Message } from "@bufbuild/protobuf";
/**
 * Describes the file bones/v1/errors.proto.
 */
export declare const file_bones_v1_errors: GenFile;
/**
 * One typed argument of an error message. The viewer's client formats it in
 * the viewer's locale (ICU plural, select, number and date), so a service
 * carries the value and never an interpolated string.
 *
 * An argument is declared with its code in the emitter's proto. A field marked
 * `debug_redact` holds a secret and is never an argument.
 *
 * The member names are `string`, `int64`, `double` and `timestamp` so no
 * generated variant repeats the enum name; the JSON names stay `stringValue`,
 * `int64Value`, `doubleValue` and `timestampValue`.
 *
 * @generated from message bones.v1.ErrorArg
 */
export type ErrorArg = Message<"bones.v1.ErrorArg"> & {
    /**
     * @generated from oneof bones.v1.ErrorArg.value
     */
    value: {
        /**
         * @generated from field: string string = 1 [json_name = "stringValue"];
         */
        value: string;
        case: "string";
    } | {
        /**
         * @generated from field: int64 int64 = 2 [json_name = "int64Value"];
         */
        value: bigint;
        case: "int64";
    } | {
        /**
         * @generated from field: double double = 3 [json_name = "doubleValue"];
         */
        value: number;
        case: "double";
    } | {
        /**
         * @generated from field: google.protobuf.Timestamp timestamp = 4 [json_name = "timestampValue"];
         */
        value: Timestamp;
        case: "timestamp";
    } | {
        case: undefined;
        value?: undefined;
    };
};
/**
 * Describes the message bones.v1.ErrorArg.
 * Use `create(ErrorArgSchema)` to create a new message.
 */
export declare const ErrorArgSchema: GenMessage<ErrorArg>;
/**
 * A single field-level validation failure. Composed by services into a
 * Connect error envelope under `details` (one `ValidationFailure` per
 * failing field). Server-side request-validators typically emit one of
 * these per `validator`-style rule that fires on a request body.
 *
 * @generated from message bones.v1.ValidationFailure
 */
export type ValidationFailure = Message<"bones.v1.ValidationFailure"> & {
    /**
     * JSON Pointer to the offending field (RFC 6901), e.g. `/email`
     * or `/items/0/quantity`. Empty string for top-level failures.
     *
     * @generated from field: string field = 1;
     */
    field: string;
    /**
     * Human-readable description of what went wrong.
     *
     * @generated from field: string message = 2;
     */
    message: string;
    /**
     * Optional machine-readable rule identifier (e.g. `min_length`,
     * `format`, `range`). Empty when the failure doesn't map to a
     * discrete rule.
     *
     * @generated from field: string rule = 3;
     */
    rule: string;
    /**
     * Typed arguments of the message for `rule`, keyed by the placeholder name
     * in the `bones.validation.<rule>` catalog string. Never a `debug_redact`
     * field.
     *
     * @generated from field: map<string, bones.v1.ErrorArg> args = 4;
     */
    args: {
        [key: string]: ErrorArg;
    };
};
/**
 * Describes the message bones.v1.ValidationFailure.
 * Use `create(ValidationFailureSchema)` to create a new message.
 */
export declare const ValidationFailureSchema: GenMessage<ValidationFailure>;
/**
 * Why a call was refused, in the emitter's own vocabulary. Attached to the
 * Connect error envelope under `details`, once per error, so the caller can
 * read the emitted error code rather than only the coarse Connect code.
 *
 * SDK error types render the three values as a text token,
 * `[bones-error code=<code> emitter=<emitter>]`, so the
 * code survives any reporter that keeps only text.
 *
 * @generated from message bones.v1.ErrorInfo
 */
export type ErrorInfo = Message<"bones.v1.ErrorInfo"> & {
    /**
     * The emitted error code: the name of the value in the emitter's error
     * enum, e.g. `GRANT_MISSING`. Upper-case letters, digits and underscores,
     * starting with a letter, at most 128 characters.
     *
     * @generated from field: string code = 1;
     */
    code: string;
    /**
     * The deployable that emitted the error, e.g. `payments`. Lower-case
     * letters, digits, `.`, `_` and `-`, at most 128 characters. Stamped by the
     * transport layer of the emitting service; empty until then.
     *
     * @generated from field: string emitter = 2;
     */
    emitter: string;
    /**
     * Typed arguments of the message for `code`, keyed by the placeholder name
     * in the `<emitter>.errors.<CODE>` catalog string. Each is declared with
     * the code in the emitter's proto. Never a `debug_redact` field.
     *
     * @generated from field: map<string, bones.v1.ErrorArg> args = 4;
     */
    args: {
        [key: string]: ErrorArg;
    };
};
/**
 * Describes the message bones.v1.ErrorInfo.
 * Use `create(ErrorInfoSchema)` to create a new message.
 */
export declare const ErrorInfoSchema: GenMessage<ErrorInfo>;
// ===== ./generated/bones/v1/pagination_pb.d.ts =====
import type { GenEnum, GenFile, GenMessage } from "@bufbuild/protobuf/codegenv2";
import type { Message } from "@bufbuild/protobuf";
/**
 * Describes the file bones/v1/pagination.proto.
 */
export declare const file_bones_v1_pagination: GenFile;
/**
 * Cursor-based pagination request. Empty cursor = start; page_size 0 = server default.
 *
 *   message ListFoosRequest {
 *     bones.v1.PageRequest page = 1;
 *   }
 *
 * @generated from message bones.v1.PageRequest
 */
export type PageRequest = Message<"bones.v1.PageRequest"> & {
    /**
     * Opaque cursor returned by a previous PageResponse.next_cursor. Clients
     * must treat it as opaque. Empty string means start of collection.
     * The server SEALS each cursor using authenticated encryption, with an
     * expiry and bindings to service, method, organization, principal and query
     * hash. Before any query runs, the server authenticates and checks these
     * bindings and expiry. Public ErrorInfo reason strings are:
     * - CURSOR_INVALID: tampered cursor or wrong service, method, organization
     *   or principal scope.
     * - CURSOR_EXPIRED: authentic cursor whose expiry has passed.
     * - CURSOR_QUERY_MISMATCH: authentic, correctly scoped cursor whose query
     *   hash does not match the request.
     * Every rejection above happens before any query runs.
     *
     * @generated from field: string cursor = 1;
     */
    cursor: string;
    /**
     * Max items the server should return on this page.
     * 0 or unset means use the server default. Values above the server maximum
     * are clamped by the server, following AIP-158.
     *
     * @generated from field: uint32 page_size = 2;
     */
    pageSize: number;
};
/**
 * Describes the message bones.v1.PageRequest.
 * Use `create(PageRequestSchema)` to create a new message.
 */
export declare const PageRequestSchema: GenMessage<PageRequest>;
/**
 * Cursor-based pagination response. Empty next_cursor = no more pages.
 * `revision` doubles as the list-level ETag for cache validation.
 *
 *   message ListFoosResponse {
 *     repeated Foo items = 1;
 *     bones.v1.PageResponse page = 2;
 *   }
 *
 * @generated from message bones.v1.PageResponse
 */
export type PageResponse = Message<"bones.v1.PageResponse"> & {
    /**
     * Opaque cursor for the next page. Empty when no more pages.
     *
     * @generated from field: string next_cursor = 1;
     */
    nextCursor: string;
    /**
     * Collection-level revision/ETag — changes whenever anything visible under
     * the request's filter and caller authorization may have changed. Used for
     * list-level cache validation via if-none-match metadata.
     *
     * @generated from field: string revision = 2;
     */
    revision: string;
    /**
     * Size of the collection visible under the request's filter and caller
     * authorization, with the same scope as revision. Unset unless opted in.
     *
     * @generated from field: bones.v1.Total total = 3;
     */
    total?: Total | undefined;
};
/**
 * Describes the message bones.v1.PageResponse.
 * Use `create(PageResponseSchema)` to create a new message.
 */
export declare const PageResponseSchema: GenMessage<PageResponse>;
/**
 * A collection size and how it was obtained. Both KIND_ESTIMATE and KIND_EXACT count
 * only items visible under the request's filter and caller authorization,
 * with the same scope as PageResponse.revision. Table-wide or planner-only
 * estimates across organizations are forbidden.
 *
 * @generated from message bones.v1.Total
 */
export type Total = Message<"bones.v1.Total"> & {
    /**
     * @generated from field: uint64 value = 1;
     */
    value: bigint;
    /**
     * @generated from field: bones.v1.Total.Kind kind = 2;
     */
    kind: Total_Kind;
};
/**
 * Describes the message bones.v1.Total.
 * Use `create(TotalSchema)` to create a new message.
 */
export declare const TotalSchema: GenMessage<Total>;
/**
 * @generated from enum bones.v1.Total.Kind
 */
export declare enum Total_Kind {
    /**
     * @generated from enum value: KIND_UNSPECIFIED = 0;
     */
    UNSPECIFIED = 0,
    /**
     * An estimate within the request's filter and authorization; may be off.
     *
     * @generated from enum value: KIND_ESTIMATE = 1;
     */
    ESTIMATE = 1,
    /**
     * An exact count within the request's filter and authorization, computed
     * once per revision.
     *
     * @generated from enum value: KIND_EXACT = 2;
     */
    EXACT = 2
}
/**
 * Describes the enum bones.v1.Total.Kind.
 */
export declare const Total_KindSchema: GenEnum<Total_Kind>;
/**
 * Offset/limit pagination request.
 *
 * Defaults track the Rust crate's `PaginationParams::default()`:
 * limit = 20 (when 0), offset = 0.
 *
 *   message ListUsersRequest {
 *     bones.v1.OffsetPageRequest page = 1;
 *   }
 *
 * @generated from message bones.v1.OffsetPageRequest
 * @deprecated
 */
export type OffsetPageRequest = Message<"bones.v1.OffsetPageRequest"> & {
    /**
     * Max items the server should return on this page.
     * 0 means use the server default (20).
     *
     * @generated from field: uint64 limit = 1;
     */
    limit: bigint;
    /**
     * Number of items to skip before this page.
     *
     * @generated from field: uint64 offset = 2;
     */
    offset: bigint;
};
/**
 * Describes the message bones.v1.OffsetPageRequest.
 * Use `create(OffsetPageRequestSchema)` to create a new message.
 * @deprecated
 */
export declare const OffsetPageRequestSchema: GenMessage<OffsetPageRequest>;
/**
 * Offset/limit pagination response.
 *
 *   message ListUsersResponse {
 *     repeated User items = 1;
 *     bones.v1.OffsetPageResponse page = 2;
 *   }
 *
 * @generated from message bones.v1.OffsetPageResponse
 * @deprecated
 */
export type OffsetPageResponse = Message<"bones.v1.OffsetPageResponse"> & {
    /**
     * Total number of items across all pages.
     *
     * @generated from field: uint64 total_count = 1;
     */
    totalCount: bigint;
    /**
     * Whether more items exist beyond this page.
     *
     * @generated from field: bool has_more = 2;
     */
    hasMore: boolean;
    /**
     * Max items per page (echoes the requested or default limit).
     *
     * @generated from field: uint64 limit = 3;
     */
    limit: bigint;
    /**
     * Number of items skipped before this page.
     *
     * @generated from field: uint64 offset = 4;
     */
    offset: bigint;
    /**
     * Opaque revision of the listed collection as of this page, usable as an
     * ETag for conditional reads.
     *
     * @generated from field: string revision = 5;
     */
    revision: string;
};
/**
 * Describes the message bones.v1.OffsetPageResponse.
 * Use `create(OffsetPageResponseSchema)` to create a new message.
 * @deprecated
 */
export declare const OffsetPageResponseSchema: GenMessage<OffsetPageResponse>;
// ===== ./index.d.ts =====
export { BonesError, PageError, isCursorRejected, type BonesErrorInit, type ErrorArg, type PageDetail, type PageErrorKind } from "./errors.js";
export { iterableOf, iterateHandle, listCallOf, type ErrorMapper, type Handle, type ListCall, type ListCallSource, type Page, type PageOptions, type Total } from "./list-call.js";
export { FormatErrorOptionError, formatError, registerErrorCatalog, type ErrorCatalog, type FormatErrorOptions, type FormattableError, type FormattedError } from "./format-error.js";
export { JsSpanSink, callCtx, type CallCtx } from "./spans.js";
export { mapCallError } from "./call-error.js";
export * from "./codec.js";
export * from "./session.js";
export { lazyOnce, wasmInput } from "./loader.js";
// ===== ./list-call.d.ts =====
import type { Span } from "@opentelemetry/api";
import type { JsSpanSink } from "./spans.js";
/** Options a caller passes to a list method; the Rust core acts on them. */
export interface PageOptions {
    pageSize?: number;
    prefetch?: 0 | 1;
    signal?: AbortSignal;
    cache?: "use" | "bypass";
}
export interface Total {
    readonly value: bigint;
    readonly kind: "estimate" | "exact";
}
export interface Page<T> {
    readonly items: readonly T[];
    readonly revision: string;
    readonly total?: Total;
    readonly fromCache: boolean;
    /** True exactly when the response carried a non-empty next cursor. */
    readonly hasNext: boolean;
}
/** A list a caller can walk page by page, item by item, or read the first page of. */
export interface ListCall<T> {
    pages(): AsyncIterable<Page<T>>;
    items(): AsyncIterable<T>;
    firstPage(): Promise<Page<T>>;
    /**
     * The same list reading `pages` ahead of the consumer: 0 reads only the
     * pages asked for, 1 reads one ahead; the Rust core reads one ahead for any
     * larger value. It wins over the `prefetch` the list was called with.
     */
    withPrefetch(pages: 0 | 1): ListCall<T>;
    /** Active iteration span, usable by the UI for first-row measurements. */
    readonly span?: Span;
}
/** A Rust-side cursor over a sequence of messages, each delivered as its protobuf bytes. */
export interface Handle<Raw = Uint8Array> {
    /** The next message, or `undefined` when the sequence has ended. */
    next(): Promise<Raw | undefined>;
    /** Releases the Rust side, dropping anything it read ahead. */
    cancel(): void;
    /** The `traceparent` of the handle's open iteration span, or `undefined` when it is not recording. */
    span(): string | undefined;
}
/** What turns a handle's rejection into the error the caller sees. */
export type ErrorMapper = (rejection: unknown) => unknown;
/**
 * Walks a handle as an async iterator. The handle is cancelled exactly once:
 * when the sequence ends, when `next()` fails, or when the consumer calls
 * `return()` (a `break`, an unmount or an abort), whichever comes first.
 */
export declare function iterateHandle<T, Raw = Uint8Array>(handle: Handle<Raw>, decode: (raw: Raw) => T, mapError?: ErrorMapper): AsyncIterableIterator<T>;
/** A sequence that opens a fresh handle for every walk, such as a server stream. */
export declare function iterableOf<T, Raw = Uint8Array>(open: () => Handle<Raw>, decode: (raw: Raw) => T, mapError?: ErrorMapper): AsyncIterable<T>;
/** What a generated wrapper supplies to build a {@link ListCall}. */
export interface ListCallSource<T, Raw = Uint8Array> {
    /**
     * Opens a handle over the list's pages. Called once per walk. A `prefetch`
     * names the pages the Rust core fetches ahead and wins over the caller's
     * option; `undefined` leaves the caller's option. `firstOnly` asks the core
     * for its first-page read: the host forwards it and keeps no prefetch rule.
     */
    openPages(prefetch?: 0 | 1, firstOnly?: boolean): Handle<Raw>;
    decodePage(raw: Raw): Page<T>;
    mapError?: ErrorMapper;
    /** Resolves a handle's iteration span to the live span. */
    sink: JsSpanSink;
}
/** A {@link ListCall} over Rust page handles. */
export declare function listCallOf<T, Raw = Uint8Array>(source: ListCallSource<T, Raw>): ListCall<T>;
// ===== ./loader.d.ts =====
/**
 * Starts something once, on first use. Concurrent callers share the one start.
 * A failed start is forgotten, so the next caller starts again.
 */
export declare function lazyOnce<T>(start: () => Promise<T>): () => Promise<T>;
/**
 * What the wasm module's initialiser takes for the module at `url`. A `file:`
 * module can only be reached from Node, so its bytes are read; any other
 * location is handed back for the initialiser to load.
 */
export declare function wasmInput(url: URL): Promise<URL | Uint8Array>;
// ===== ./session.d.ts =====
import type { JsSpanSink } from "./spans.js";
/** The boundary version this runtime speaks; a core built for another version refuses to construct a client. */
export declare const RUNTIME_BOUNDARY = 1;
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
/**
 * The host object of one client built from `session`. It answers the core's
 * credential asks as `bones.sdk.v1.CredentialAnswer` bytes: the credential the
 * session answers, in full, every time. The host keeps no state between asks,
 * so one session may back any number of clients and an ask the core abandons
 * leaves nothing behind. Whether a refresh renewed anything is the core's to
 * decide, by comparing what it sent with what the refresh answered. Spans go
 * to `sink`.
 */
export declare function hostOf(session: SdkSession, sink: JsSpanSink): SdkHost;
/** The origin of the page the code runs in, where a client's base address defaults to. */
export declare function pageOrigin(): string;
// ===== ./spans.d.ts =====
import type { Span, Tracer } from "@opentelemetry/api";
/** The per-call inputs the host hands the Rust core. */
export interface CallCtx {
    signal?: AbortSignal;
    traceparent?: string;
    tracestate?: string;
}
/**
 * The call context for the active JavaScript trace: the W3C trace fields the
 * registered propagator writes for `context.active()`, plus the caller's abort
 * signal. Only the host can see the active context.
 */
export declare function callCtx(signal?: AbortSignal): CallCtx;
/** The live JavaScript OpenTelemetry spans of the Rust core's spans. */
export declare class JsSpanSink {
    #private;
    /**
     * Without a `tracer` each span is started on the tracer the global provider
     * gives at that moment, so a provider registered after this sink was built
     * (the wrapper builds its sink at module load) still receives the spans.
     */
    constructor(tracer?: Tracer);
    /**
     * Opens a span from the bytes of a `bones.sdk.v1.SpanStart` and answers the
     * bytes of a `SpanStarted`: the span's `traceparent`. A span that is not
     * recording is ended at once and answers `recording: false` with no
     * `traceparent`, so the core mints its own ids.
     */
    start(bytes: Uint8Array): Uint8Array;
    /** Ends the span `start` returned, from the bytes of a `bones.sdk.v1.SpanEnd`. An unknown `traceparent` is ignored. */
    end(bytes: Uint8Array): void;
    /** The open span for `traceparent`, if the core has not ended it. */
    lookup(traceparent: string): Span | undefined;
}
