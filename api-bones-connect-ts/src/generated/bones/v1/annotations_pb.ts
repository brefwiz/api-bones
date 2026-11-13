// SPDX-License-Identifier: MIT
//
// Contract annotations for service authors.
// F1: Every bones.v1.TypedValue field carries typed_by naming the sibling
// string field holding its definition id.
//
// `bones.v1.errors` — declares the error variants an RPC may return.
// Variant names are ALL_CAPS_SNAKE strings referencing the service's
// companion error enum (the enum itself is service-owned; this
// option just enumerates which variants apply per RPC).
//
// Usage:
//
//   service AuthService {
//     rpc Login(LoginRequest) returns (LoginResponse) {
//       option (bones.v1.errors) = "INVALID_CREDENTIALS";
//       option (bones.v1.errors) = "ACCOUNT_LOCKED";
//       option (bones.v1.errors) = "MFA_REQUIRED";
//     }
//   }
//
// SDK generators consume the annotation to emit typed per-RPC error
// surfaces; undeclared runtime errors map to a catch-all variant
// keyed off `google.rpc.Status`.
//
// `bones.v1.authz` — declares what authority an RPC requires. The
// declaration belongs to the contract, so it travels with the
// contract: the RPC carries it, not the server implementation.
//
// Authority is capability, not principal class. `requires` names the
// CanonicalClaim scopes a caller must hold. The question an RPC asks
// is "does this principal hold the authority for this operation",
// never "what kind of thing is this principal". Scope strings are the
// platform's existing operation-authority vocabulary; this option
// introduces no second one.
//
// There is deliberately no service-vs-user axis here. A delegated
// token carries a user subject and a service actor simultaneously, so
// no single class describes it, and a class gate on such a token can
// only be made permissive rather than correct. Authenticating a
// caller establishes who they are; it never establishes what they may
// do.
//
// Usage:
//
//   service ReservationService {
//     rpc Reserve(ReserveRequest) returns (ReserveResponse) {
//       option (bones.v1.authz) = {
//         kind: AUTHZ_KIND_AUTHENTICATED,
//         requires: "reservation:write"
//       };
//     }
//     rpc GetHealth(GetHealthRequest) returns (GetHealthResponse) {
//       option (bones.v1.authz) = {
//         kind: AUTHZ_KIND_PUBLIC,
//         reason: "liveness probe, no tenant data"
//       };
//     }
//   }
//
// An RPC with no `authz` option is denied, and an authenticated RPC
// naming no `requires` scope is denied. A new RPC is therefore secure
// before anyone remembers it exists; declaring the authority it needs
// is the only way to make it reachable.
//
// `bones.v1.provided_capabilities` names the allocated capabilities one
// protobuf service serves. Enrollment tooling derives each capability's
// authority from the service's authenticated RPC `requires` values and from
// its `provided_broker_scopes` entries. Consumers name the capability; they
// never copy its provider-owned scope vocabulary.
//
// Naming one here does not make it yours. A capability name is allocated by
// the authority to an owner, and this option states which allocated names this
// service serves — a declaration that the authority verifies against the
// allocation, never one that creates it. A service naming a capability its
// owner does not hold is refused when its workload is declared.
//
// Tier-zero names are the exception, and are genesis rather than allocated:
// what every other workload attests against cannot hold a title issued by the
// authority it bootstraps.
//
// Plural because a service may serve more than one allocated name, and because
// which of them it serves at a given moment is not always all of them — an
// entitlement decides that, and a singular field cannot express a subset.
//
// Usage:
//
//   service ReservationService {
//     option (bones.v1.provided_capabilities) = "reservations.schedule";
//     option (bones.v1.provided_capabilities) = "reservations.waitlist";
//     // RPC declarations follow.
//   }
//
// `bones.v1.capability` is the singular predecessor. It carries the same
// meaning for one name and stays readable, so a service migrates when it
// chooses rather than when this file changes. A service setting both is
// refused rather than merged: two answers to what a service provides is the
// condition the allocation exists to remove.
//
// `bones.v1.required_capabilities` names the capabilities a service
// must have bound before it can serve — the same declaration, pointed
// the other way. A service already states what it provides here, and
// enrollment reads that from this descriptor; what it needs belongs
// beside it rather than in a second document the binary does not
// carry.
//
// The names are the platform's capability vocabulary, the one a
// service manifest already uses and a runtime intent is already
// compiled from. This option introduces no second one.
//
// Each entry carries the scopes the service intends to exercise against
// that capability, for the same reason an RPC carries `authz.requires`: a
// dependency is not "I reach this provider", it is "I reach this provider
// and hold what its surface asks of me". A bare name cannot be reconciled
// into a grant, so anything reconciling one had to read a second document,
// and a service admitted on a reachable-but-ungranted dependency reports
// itself able to serve and fails the first real call.
//
// The entry is the ask and never the answer. Which asks are granted, whether
// a grant is enabled, and at what extent are the authority's to decide, and
// they resolve when the credential refreshes rather than when the service was
// built. A declaration that stated its own grant would be the provider
// authorising itself — the condition an allocation exists to remove.
//
// The provided side carries no scopes but one kind: what a served capability
// requires of its callers is already declared once, on the RPCs that offer it,
// and deriving it from there is what keeps one answer. The one exception is a
// broker scope (`provided_broker_scopes` below): no RPC requires it, so there
// is nothing to derive it from.
//
// Absent means this service requires nothing to serve, which is the
// correct reading for a tier-zero component: what every other workload
// attests against cannot itself wait on one of them.
//
// Usage:
//
//   service ReservationService {
//     option (bones.v1.capability) = "reservations.schedule";
//     option (bones.v1.required_capabilities) = {
//       name: "platform.auth",
//       scopes: ["auth:introspect"]
//     };
//     option (bones.v1.required_capabilities) = { name: "platform.config" };
//     // RPC declarations follow.
//   }
//
// `bones.v1.capability_labels` and `bones.v1.permission_labels` let a
// provider label, in the same contract, the capabilities and scopes it
// defines. The identifiers above are for machines; the person who
// grants a capability or a scope reads a label, not the identifier.
// Declaring the label beside what defines it means no consumer keeps a
// map from identifier to text.
//
// Across one contract's descriptor set, every capability it provides
// (via `capability` or `provided_capabilities`) and every scope its
// authenticated RPCs require or it declares in `provided_broker_scopes`
// carries exactly one label, declared on a service that provides that
// capability, requires that scope, or declares it. A second label for the
// same name is refused, not merged, and a label for a name or scope the
// contract does not provide is refused: a provider labels only its own
// authority.
//
// A permission label says what a holder can do ("Create and change
// reservations"); a capability label names the area it belongs to
// ("Reservations"). Neither names an internal service.
//
// Labels are presentation, not authority: declaring one grants
// nothing and narrows nothing. What is granted stays exactly what
// `capability`, `provided_capabilities`, `AuthzRule.requires`, and
// `provided_broker_scopes` already say.
//
// Usage:
//
//   service ReservationService {
//     option (bones.v1.capability) = "reservations.schedule";
//     option (bones.v1.capability_labels) = {
//       name: "reservations.schedule",
//       label: { text: "Reservations", translations: { key: "fr" value: "Réservations" } }
//     };
//     option (bones.v1.permission_labels) = {
//       scope: "reservation:write",
//       label: { text: "Create and change reservations" }
//     };
//     // RPC declarations follow.
//   }

// `bones.v1.provided_broker_scopes` declares scopes of a capability this
// contract provides that no RPC requires, because a broker rather than an
// RPC enforces them: the right to subscribe to the provider's own events,
// or a broker's own provisioning authority. Enrollment derives a
// capability's scopes from RPC `requires` alone, so a capability over
// events, which serves no RPC, would otherwise have no scope to confer.
//
// Each entry names the provided capability it belongs to, the scope, and
// optionally `implied_by`. Its grant class is `capability` by construction,
// so the entry states none: the scope is granted by capability consumption
// or an explicit capability grant, exactly like an RPC-derived scope, and
// registered, granted, revoked and audited by the same authority, the
// registration authority. It names no enforcer either: the event form is
// recognized by its shape, and the enforcer of any other scope by the
// registration authority's enforcer registry. Declaring a scope without
// `implied_by` grants nothing.
//
// `implied_by` is a third grant path, for the
// `<ns>:events:<topic>:subscribe:own` form only: the platform gives the
// scope to every holder of the named capability with no grant record, and
// only while the holder holds that capability. When the holder's standing
// grant or consumption of that capability is withdrawn, the scope goes with
// it at the next mint, and the relay consent facts follow the same rule.
//
// A broker scope is a CanonicalClaim scope, byte-identical wherever it is
// named, and carries no subject. Its namespace is the text before its first
// `:`, the same parse the registration authority applies to every scope.
//
// Admission. A broker scope is admitted in exactly four forms:
//   - `<ns>:events:subscribe`, an event scope, declared by the workload whose
//     service name (the service segment of its SVID) is `<ns>`;
//   - `<ns>:events:<topic>:subscribe`, a topic event scope, declared by the
//     same workload; `<topic>` is a single token with the characters of a
//     namespace token and none of `:`, `.`, `*` or `>`;
//   - `<ns>:events:<topic>:subscribe:own`, an own-events scope, declared by
//     the same workload on a broker-only capability, with a single-token
//     `<topic>` as above; the broker compiles it to the holder's own events
//     only, never a sibling's, and `implied_by` grants it (S6-S8);
//   - `<ns>:<right>`, a broker's own right, declared by the workload the
//     enforcer registry names as the enforcer of `<ns>`.
// Every other shape names a scope no broker enforces, and is refused as a
// dead grant.
//
// The three event forms differ in how they are served. The namespace form
// `<ns>:events:subscribe` is served by the per-organization durables. A topic
// scope is served live only: per-replica ephemeral core subscriptions that the
// broker grants per granting organization, never a durable, with no queue
// group and no replay. A consumer holding a topic scope by a platform grant
// hears every organization. The `:own` form is served by a short-lived
// core-NATS subscription limited to the holder's own `/svc` segment, as the
// last subject segment, never wider and never a per-organization durable; the
// reconciler sets nothing up for it and creates no consumer object. A fact a
// consumer must never miss belongs on the namespace form.
//
// The event forms are recognized by shape; the enforcer is recognized by the
// registry, never by the scope. A registered enforcer declaring an event
// scope of any of the three event forms whose `<ns>` differs from its own
// service name is therefore still
// refused, under A1 below.
//
// For an event scope, `<ns>` is the provider's event-subject service segment,
// and the platform's event broker derives everything else from it, never from
// the scope's text: the platform's canonical stream name for that service
// (the service name uppercased, `-` replaced by `_`), the subscription on
// `<ns>.events.>` under the platform's event-subject prefix, anchored to the
// organization, and the durable operations the platform owns. Because A1
// refuses any other `<ns>`, a provider can never declare, and so never
// confer, another provider's events.
//
// A broker's own right lives in the broker's namespace, on a capability of
// the broker's own: for example `broker:accounts:provision` on
// `broker.provisioning`, declared by the broker's workload. Subject-shaped
// broker vocabulary (`<broker>:stream:subscribe:<pattern>`) is the broker
// operator's and is never declared here.
//
// Refusals. This is the complete list; the registration authority and every
// static validator apply it identically. Across one contract's descriptor
// set, registered by one provider workload, an entry is refused when any
// item below holds.
//
// Admission refusals:
//   A1. its `scope` is an event scope, exactly `<ns>:events:subscribe`,
//      `<ns>:events:<topic>:subscribe` or
//      `<ns>:events:<topic>:subscribe:own`, and `<ns>` is not the registering
//      workload's service name, even when that workload is a registered
//      enforcer;
//   A2. its `scope` is any scope other than an event scope of the three event forms, and the
//      registering workload is not the registered enforcer of its namespace
//      in the enforcer registry;
//   A3. its `scope` is an event scope of any of the three forms, and the registering workload's
//      canonical stream name equals that of another service already
//      registered, as `order-book` and `order_book` both yield `ORDER_BOOK`:
//      the first registrant keeps the stream, and the later registration is
//      refused.
//
// Structural well-formedness refusals:
//   S1. its `capability` is not one the contract provides;
//   S2. its `scope` has no namespace: it is empty, contains no `:`, or has
//      nothing before or nothing after its first `:`;
//   S3. its `scope` contains `.`, `*` or `>`: a broker scope carries no
//      subject;
//   S4. its `capability` also carries an RPC-required scope, that is some
//      service providing it has an RPC whose `authz.requires` is non-empty:
//      a broker-scope capability serves no RPC, so a grant of it can never
//      widen to an RPC surface;
//   S5. the same scope is declared twice, or is also required by an RPC: one
//      scope, one declaration.
//   S6. its `implied_by` is non-empty and is not a syntactically valid
//      capability name.
//   S7. its `implied_by` is non-empty and does not name a capability the same
//      contract or provider provides: a name of another provider's
//      capability, or an unknown one, is refused at declaration;
//   S8. its `implied_by` is non-empty and its `scope` is not of the form
//      `<ns>:events:<topic>:subscribe:own` with a single-token topic: only
//      that own-events form may be implied by a capability.
//
// Usage — a separate event capability, distinct from the RPC capability
// `reservations.schedule`, registered by the workload whose service name is
// `reservations`:
//
//   service ReservationEvents {
//     option (bones.v1.provided_capabilities) = "reservations.events";
//     option (bones.v1.provided_broker_scopes) = {
//       capability: "reservations.events",
//       scope: "reservations:events:subscribe"
//     };
//     option (bones.v1.capability_labels) = {
//       name: "reservations.events",
//       label: { text: "Reservation changes" }
//     };
//     option (bones.v1.permission_labels) = {
//       scope: "reservations:events:subscribe",
//       label: { text: "Follow reservation changes" }
//     };
//   }

// `bones.v1.capability_scopes` groups a provider's RPC scopes into
// task-level permission groups. Each entry is one capability and the set of
// scopes holding it confers. A scope is the unit a gate enforces; a group is
// the unit a person grants, named for a job an operator understands
// ("Take and change bookings") rather than for an area of the API. The
// provider declares its groups in its own contract, beside the RPCs they
// cover, so no second document maps a group to its scopes.
//
// Repeated, one entry per group, so one service may declare several groups.
// Each `capability` is a capability this contract provides, exactly as
// `capability` or `provided_capabilities` names it, and is labelled through
// `capability_labels` like any other. Each listed scope is labelled through
// `permission_labels`. A scope may appear in more than one group: grants are
// a union, so overlap widens nothing.
//
// This option carries no validation of its own. The rules below belong to the
// contract compiler and to the static check that runs on the contract, and
// both apply them identically. Across one contract's descriptor set, an
// entry set is refused when any item holds:
//   - a listed scope is required by no RPC and declared in no
//     `provided_broker_scopes` entry (a dead grant);
//   - an RPC-required scope appears in no group (an unreachable RPC);
//   - a group names a capability the contract does not provide, or lists no
//     scope, or two entries name the same capability;
//   - a group or a listed scope carries no label.
//
// Declaring a group grants nothing: which roles hold it is the authority's to
// decide, through the ordinary guarded grant path.
//
// Usage:
//
//   service ReservationService {
//     option (bones.v1.provided_capabilities) = "reservations.view";
//     option (bones.v1.provided_capabilities) = "reservations.manage";
//     option (bones.v1.capability_scopes) = {
//       capability: "reservations.view",
//       scopes: ["reservation:read"]
//     };
//     option (bones.v1.capability_scopes) = {
//       capability: "reservations.manage",
//       scopes: ["reservation:read", "reservation:write"]
//     };
//     option (bones.v1.capability_labels) = {
//       name: "reservations.manage",
//       label: { text: "Take and change reservations" }
//     };
//     // The other labels, and the RPC declarations, follow.
//   }

// `bones.v1.capability_audience` declares which organization roles a
// product's capability is granted to when an organization enables the
// product. The provider states it in the contract, beside the capability it
// serves, so enablement reads one descriptor and no second document maps a
// capability to its default audience.
//
//   ADMINISTRATORS -> admin
//   MEMBERS        -> admin, member
//   EVERYONE       -> admin, member, viewer
//
// `admin` is always granted explicitly. Unset, or
// CAPABILITY_AUDIENCE_UNSPECIFIED, means the capability is never part of a
// default grant: it is reachable only through an explicit role-capability
// grant. Declaring an audience is the provider's statement of who the
// capability is for, never a grant by itself; the authority applies it
// through the ordinary guarded grant path.
//
// Usage:
//
//   service ReservationService {
//     option (bones.v1.provided_capabilities) = "reservations.schedule";
//     option (bones.v1.capability_audience) = CAPABILITY_AUDIENCE_EVERYONE;
//   }

// `bones.v1.attributes` declares the resource attributes a credential can be
// narrowed on at this service, and `bones.v1.attr` marks the request fields
// that carry each one. The vocabulary is closed: a restriction naming a
// dimension the service did not declare is refused.
//
// A dimension has a name (lower_snake_case, unique within the service), a
// value type, a polarity and a matching mode.
//
//   GRANTED    nothing is permitted until the authority names a value, so an
//              absent dimension denies.
//   NARROWABLE everything the scopes reach is permitted until a restriction
//              names values, so an absent dimension permits.
//   Polarity UNSPECIFIED is invalid.
//
// Values match EXACT by default; UNSPECIFIED and EXACT both mean exact.
// SEGMENT_PREFIX is opt-in and requires a `separator`: a prefix is a list of
// segments, never a metacharacter. Glob and regular-expression matching do
// not exist. `separator` is required if and only if the matching is
// SEGMENT_PREFIX; it is exactly one character that is not whitespace, `*`,
// `?` or `>`.
//
// This option carries no validation of its own; the service runtime enforces
// the rules above at boot. A field naming a dimension through `attr` must name
// one declared in its service's `attributes`.
//
// Usage:
//
//   service ReservationService {
//     option (bones.v1.attributes) = {
//       name: "venue",
//       type: ATTRIBUTE_TYPE_UUID,
//       polarity: ATTRIBUTE_POLARITY_NARROWABLE
//     };
//     option (bones.v1.attributes) = {
//       name: "folder",
//       type: ATTRIBUTE_TYPE_STRING,
//       polarity: ATTRIBUTE_POLARITY_GRANTED,
//       matching: ATTRIBUTE_MATCHING_SEGMENT_PREFIX,
//       separator: "/"
//     };
//   }
//
//   message ListReservationsRequest {
//     string venue_id = 1 [(bones.v1.attr) = "venue"];
//   }
//
// `bones.v1.mcp` opts one RPC into the MCP projection of the contract. An RPC
// with no `mcp` option is not projected, and an RPC declaring
// AUTHZ_KIND_PUBLIC is never projected. Name and description come from the
// RPC and its comments; the option adds only an optional human title. The
// shape of the call is not an MCP concern: `bones.v1.call_shape` declares it
// once on the RPC and every projection reads that one declaration.
//
// Usage:
//
//   service ReservationService {
//     rpc Reserve(ReserveRequest) returns (ReserveResponse) {
//       option (bones.v1.mcp) = { title: "Reserve a table" };
//     }
//     rpc WatchReservations(WatchRequest) returns (stream Reservation) {
//       option (bones.v1.call_shape) = CALL_SHAPE_UNBOUNDED_STREAM;
//       option (bones.v1.mcp) = {};
//     }
//   }
//
// `bones.v1.call_shape` declares what kind of call an RPC is, in terms a
// descriptor cannot express: a unary RPC and a client stream are fully
// described by their signature, but a server stream may end, never end, or
// stand for a long-running operation. UNSPECIFIED means the shape is derived
// from the signature (unary or client stream); a server stream must declare
// FINITE_STREAM, UNBOUNDED_STREAM or LONG_RUNNING, and a bidirectional stream
// is never projectable. Every projection of the contract (MCP, infrastructure
// providers, SDKs) reads this one declaration.
//
// `bones.v1.list_stream_of` is set on a server-streaming RPC and names the
// unary List RPC of the same service whose request it accepts and whose items
// it streams. Its response message carries `repeated <Item> items = 1;
// bones.v1.PageResponse page = 2;`, one keyset batch per message, with
// `page.next_cursor` the resume cursor after that batch. The RPC also declares
// `call_shape = CALL_SHAPE_FINITE_STREAM`; `list_stream_of` on any other call
// shape is refused.
//
// `bones.v1.batch_of` names the single-item method of the same service that
// this RPC batches, so a consumer reads the batch form of an operation from
// the provider's surface instead of fanning out one call per item. It covers
// reads and writes alike: N independent singles, each with its own outcome. A
// batch method keeps the idempotency level, authorization and error set of the
// single it names, and each item's outcome is its own: one item's refusal
// never fails the others. The provider documents its maximum items per call on
// the RPC. A set of a parent and its children that commits or fails together
// is `nested_write_of`, not `batch_of`.
//
// A side-effecting batch runs its items in request order, each as the
// single's own transaction. A repeated id runs once and is answered at each of
// its positions. Pre-execution refusals, authentication and more than 200
// items, refuse the call. Once execution starts the call never fails whole: a
// failure that is not the item's own outcome answers that item `aborted`, and
// every item not run `aborted` too.
//
// Usage:
//
//   service SubjectKeyService {
//     rpc DeriveSubjectKey(DeriveSubjectKeyRequest) returns (DeriveSubjectKeyResponse);
//     rpc BatchDeriveSubjectKeys(BatchDeriveSubjectKeysRequest)
//         returns (BatchDeriveSubjectKeysResponse) {
//       option (bones.v1.batch_of) = "DeriveSubjectKey";
//     }
//   }
//
// `bones.v1.nested_write_of` is set on the children field of a parent write
// request and names, as a bare method name like `batch_of`, the single child
// write of the same service that each child passes through. On create the
// field is `repeated <Child> <children>`, at most 200. On update it is
// `optional <Child>Set <children>`, with `message <Child>Set { repeated <Child>
// items = 1; }`: unset leaves the stored children unchanged; set, even empty,
// is the complete set, so listed keys are upserted and every other key is
// deleted. `<Child>` is the message the named single writes: that method's
// request message itself, or the type of the one singular message field of
// that type in its request; a request with two such fields is ambiguous and
// refused. The request carrying `nested_write_of` is never the input of a
// read, an RPC declaring `NO_SIDE_EFFECTS`. Every RPC that has that request
// as its input resolves the name in its own service and passes the whole
// check; one refusal refuses the field. The cap of 200 is the server's to
// enforce; no option declares it. The parent and its children commit in one
// transaction, all or nothing: one refused child refuses the call and
// nothing commits, answered `invalid_argument` with
// `bones.v1.ValidationFailure.field` `<children>[i].<field>`. Each child
// passes the validation and authorization of the single child write. The
// server writes any number of children with a statement count independent of
// how many there are, and the response echoes the stored children. This is
// deliberately unlike `batch_of`, whose items each have their own outcome: a
// partial nested write would leave the caller to compensate.
//
// Usage:
//
//   message CreateWidgetRequest {
//     string name = 1;
//     repeated WidgetPart parts = 2 [(bones.v1.nested_write_of) = "PutWidgetPart"];
//   }
//   message PutWidgetPartRequest { string widget_id = 1; WidgetPart part = 2; }
//   message WidgetPartSet { repeated WidgetPart items = 1; }
//   message UpdateWidgetRequest {
//     string id = 1;
//     optional WidgetPartSet parts = 2 [(bones.v1.nested_write_of) = "PutWidgetPart"];
//   }
//   service WidgetService {
//     rpc PutWidgetPart(PutWidgetPartRequest) returns (WidgetPart);
//     rpc CreateWidget(CreateWidgetRequest) returns (CreateWidgetResponse);
//     rpc UpdateWidget(UpdateWidgetRequest) returns (UpdateWidgetResponse);
//   }
//
// `bones.v1.instance_of` names the message an instance message is
// instantiated from, by fully-qualified name: a slot names the schedule rule
// it is generated from, a booking line the price-list item it prices. An
// absent option means the message is not an instance of another message.
//
// `bones.v1.defaulted_from` names, on a field of such an instance, the field
// of the `instance_of` message that proposes this field's default, as
// `<package>.<Message>.<field>`. The instance's value stays overridable: the
// option records the origin of the proposal and never constrains the value.
// A consumer reads the origin to show where a default came from and to
// record who changed it.
//
// `bones.v1.resource` declares that a message is a managed resource: `type` is
// its Terraform noun in snake_case, `identity_field` names the field that
// identifies an instance, and `methods` optionally names the RPCs of the same
// service that implement each verb. An empty verb means the AIP convention:
// `<Verb><Message>`, and `List<Message>s` for list.
//
// Usage:
//
//   message Reservation {
//     option (bones.v1.resource) = {
//       type: "reservation",
//       identity_field: "id",
//       methods: { update: "ReplaceReservation" }
//     };
//     string id = 1;
//   }

// Automation surface. A product declares what organizations may automate
// over it with three options on the protos it already has, and with nothing
// else: no second document lists the surface.
//
//   `bones.v1.automation_trigger` marks a domain-event message as a trigger.
//   The trigger is identified by the message's full name.
//
//   `bones.v1.automation_action` marks a public RPC as an action. The action
//   is identified by the method's full name and is dispatched only as that
//   RPC of the declaring product, so there is no action kind to choose and no
//   way to declare a raw HTTP, proto, shell or SQL action. Its grant is
//   exactly the RPC's `authz.requires`; the option carries no scope.
//
//   `bones.v1.classification` classifies one field. Every field of a trigger
//   message, and every field of the response of an action whose effect is
//   PURE (a read action), carries one, at every depth: through each
//   message-typed field defined in the product's own files. A field whose
//   type is defined elsewhere (a well-known type, a bones.v1 message) is
//   classified at the field and not descended into.
//
//   `bones.v1.meaning` labels one field with what it denotes. It sits beside
//   `classification`: a core record's fields carry both, and the business
//   kernel's registry reads them instead of restating them. The value is a
//   term of the curated vocabulary, which is schema.org core plus the
//   extensions the platform curates. A product picks a term and never
//   defines one: a term outside the vocabulary is refused by the registry's
//   closed-vocabulary check, and the missing term is filed as a curation
//   request. A field with no meaning mapping is treated as
//   DATA_CLASSIFICATION_CONFIDENTIAL by the registry. The field's value type
//   is its proto type, so no option restates it.
//
// Guardrails are declaration flags on the action:
//   - `effect` is mandatory. POINT_OF_NO_RETURN marks an irreversible action;
//     it is never a second flag.
//   - `person_reserved` marks an action only a person's own signed press can
//     run. No automation step may call it.
//   - `planner_pinned` states that the action changes state the product's
//     own planner owns, and that the product pins an automation's change so
//     the planner never moves it behind the operator's back. A mutation of
//     planner-owned state without that guarantee is not declared at all.
//
// These options carry no validation of their own. The rules below belong to
// the surface compiler and to the static checks that run on the contract,
// and all apply them identically. Across one product's descriptor set a
// declaration is refused when any item holds:
//   T1. a trigger carries no label;
//   A1. an action carries no label, or its effect is UNSPECIFIED;
//   A2. the RPC is not AUTHZ_KIND_AUTHENTICATED with a non-empty `requires`;
//   A3. the RPC is not unary;
//   A4. the effect is not PURE and `idempotency` is UNSPECIFIED, or it is
//       KEYED and `idempotency_key_fields` is empty or names anything other
//       than a top-level field of the request;
//   A5. `resolves` is set on an action whose effect is not PURE, or names a
//       field the request or response does not have;
//   A6. the request or response reaches a `google.protobuf.Any`,
//       `google.protobuf.Struct` or `google.protobuf.Value` field: an untyped
//       payload is a generic dispatcher;
//   A7. `person_reserved` and `planner_pinned` are both set;
//   C1. a field the classification rule covers is unclassified: the option is
//       absent or DATA_CLASSIFICATION_UNSPECIFIED. The two read the same.
//
// Usage:
//
//   message DepartureConfirmed {
//     option (bones.v1.automation_trigger) = {
//       label: { text: "Departure confirmed" }
//     };
//     string departure_id = 1 [(bones.v1.classification) = DATA_CLASSIFICATION_INTERNAL];
//     string guide_name = 2 [(bones.v1.classification) = DATA_CLASSIFICATION_CONFIDENTIAL];
//   }
//
//   service DepartureService {
//     rpc SearchDepartures(SearchDeparturesRequest) returns (SearchDeparturesResponse) {
//       option (bones.v1.authz) = {
//         kind: AUTHZ_KIND_AUTHENTICATED,
//         requires: "departure:read"
//       };
//       option (bones.v1.automation_action) = {
//         label: { text: "Find departures" },
//         effect: ACTION_EFFECT_PURE,
//         resolves: {
//           query_field: "query",
//           results_field: "departures",
//           value_field: "id",
//           label_field: "title"
//         }
//       };
//     }
//     rpc ConfirmDeparture(ConfirmDepartureRequest) returns (ConfirmDepartureResponse) {
//       option (bones.v1.authz) = {
//         kind: AUTHZ_KIND_AUTHENTICATED,
//         requires: "departure:confirm"
//       };
//       option (bones.v1.automation_action) = {
//         label: { text: "Confirm a departure" },
//         effect: ACTION_EFFECT_POINT_OF_NO_RETURN,
//         person_reserved: true
//       };
//     }
//   }
//
// Templates a product ships, and the manifest a release signs, are in
// `bones/v1/automation_manifest.proto`.

// @generated by protoc-gen-es v2.16.0 with parameter "target=ts,import_extension=js"
// @generated from file bones/v1/annotations.proto (package bones.v1, syntax proto3)
/* eslint-disable */

import type { GenEnum, GenExtension, GenFile, GenMessage } from "@bufbuild/protobuf/codegenv2";
import { enumDesc, extDesc, fileDesc, messageDesc } from "@bufbuild/protobuf/codegenv2";
import type { FieldOptions, MessageOptions, MethodOptions, ServiceOptions } from "@bufbuild/protobuf/wkt";
import { file_google_protobuf_descriptor } from "@bufbuild/protobuf/wkt";
import type { Message } from "@bufbuild/protobuf";

/**
 * Describes the file bones/v1/annotations.proto.
 */
export const file_bones_v1_annotations: GenFile = /*@__PURE__*/
  fileDesc("Chpib25lcy92MS9hbm5vdGF0aW9ucy5wcm90bxIIYm9uZXMudjEiMgoSUmVxdWlyZWRDYXBhYmlsaXR5EgwKBG5hbWUYASABKAkSDgoGc2NvcGVzGAIgAygJIoMBCgVMYWJlbBIMCgR0ZXh0GAEgASgJEjcKDHRyYW5zbGF0aW9ucxgCIAMoCzIhLmJvbmVzLnYxLkxhYmVsLlRyYW5zbGF0aW9uc0VudHJ5GjMKEVRyYW5zbGF0aW9uc0VudHJ5EgsKA2tleRgBIAEoCRINCgV2YWx1ZRgCIAEoCToCOAEiPwoPQ2FwYWJpbGl0eUxhYmVsEgwKBG5hbWUYASABKAkSHgoFbGFiZWwYAiABKAsyDy5ib25lcy52MS5MYWJlbCJACg9QZXJtaXNzaW9uTGFiZWwSDQoFc2NvcGUYASABKAkSHgoFbGFiZWwYAiABKAsyDy5ib25lcy52MS5MYWJlbCJECgtCcm9rZXJTY29wZRISCgpjYXBhYmlsaXR5GAEgASgJEg0KBXNjb3BlGAIgASgJEhIKCmltcGxpZWRfYnkYAyABKAkiNgoQQ2FwYWJpbGl0eVNjb3BlcxISCgpjYXBhYmlsaXR5GAEgASgJEg4KBnNjb3BlcxgCIAMoCSJQCglBdXRoelJ1bGUSIQoEa2luZBgBIAEoDjITLmJvbmVzLnYxLkF1dGh6S2luZBIOCgZyZWFzb24YAiABKAkSEAoIcmVxdWlyZXMYAyADKAkiugEKEkF0dHJpYnV0ZURpbWVuc2lvbhIMCgRuYW1lGAEgASgJEiUKBHR5cGUYAiABKA4yFy5ib25lcy52MS5BdHRyaWJ1dGVUeXBlEi0KCHBvbGFyaXR5GAMgASgOMhsuYm9uZXMudjEuQXR0cmlidXRlUG9sYXJpdHkSLQoIbWF0Y2hpbmcYBCABKA4yGy5ib25lcy52MS5BdHRyaWJ1dGVNYXRjaGluZxIRCglzZXBhcmF0b3IYBSABKAkiKwoNTWNwUHJvamVjdGlvbhINCgV0aXRsZRgCIAEoCUoECAEQAlIFc2hhcGUiYgoOUmVzb3VyY2VPcHRpb24SDAoEdHlwZRgBIAEoCRIWCg5pZGVudGl0eV9maWVsZBgCIAEoCRIqCgdtZXRob2RzGAMgASgLMhkuYm9uZXMudjEuUmVzb3VyY2VNZXRob2RzIlwKD1Jlc291cmNlTWV0aG9kcxIOCgZjcmVhdGUYASABKAkSCwoDZ2V0GAIgASgJEg4KBnVwZGF0ZRgDIAEoCRIOCgZkZWxldGUYBCABKAkSDAoEbGlzdBgFIAEoCSIzChFBdXRvbWF0aW9uVHJpZ2dlchIeCgVsYWJlbBgBIAEoCzIPLmJvbmVzLnYxLkxhYmVsIocCChBBdXRvbWF0aW9uQWN0aW9uEh4KBWxhYmVsGAEgASgLMg8uYm9uZXMudjEuTGFiZWwSJgoGZWZmZWN0GAIgASgOMhYuYm9uZXMudjEuQWN0aW9uRWZmZWN0Ei4KC2lkZW1wb3RlbmN5GAMgASgOMhkuYm9uZXMudjEuSWRlbXBvdGVuY3lNb2RlEh4KFmlkZW1wb3RlbmN5X2tleV9maWVsZHMYBCADKAkSFwoPcGVyc29uX3Jlc2VydmVkGAUgASgIEhYKDnBsYW5uZXJfcGlubmVkGAYgASgIEioKCHJlc29sdmVzGAcgASgLMhguYm9uZXMudjEuRW50aXR5UmVzb2x2ZXIiZgoORW50aXR5UmVzb2x2ZXISEwoLcXVlcnlfZmllbGQYASABKAkSFQoNcmVzdWx0c19maWVsZBgCIAEoCRITCgt2YWx1ZV9maWVsZBgDIAEoCRITCgtsYWJlbF9maWVsZBgEIAEoCSqkAQoSQ2FwYWJpbGl0eUF1ZGllbmNlEiMKH0NBUEFCSUxJVFlfQVVESUVOQ0VfVU5TUEVDSUZJRUQQABImCiJDQVBBQklMSVRZX0FVRElFTkNFX0FETUlOSVNUUkFUT1JTEAESHwobQ0FQQUJJTElUWV9BVURJRU5DRV9NRU1CRVJTEAISIAocQ0FQQUJJTElUWV9BVURJRU5DRV9FVkVSWU9ORRADKo0BCglBdXRoektpbmQSGgoWQVVUSFpfS0lORF9VTlNQRUNJRklFRBAAEhUKEUFVVEhaX0tJTkRfUFVCTElDEAMSHAoYQVVUSFpfS0lORF9BVVRIRU5USUNBVEVEEAQiBAgBEAEiBAgCEAIqEkFVVEhaX0tJTkRfU0VSVklDRSoPQVVUSFpfS0lORF9VU0VSKmMKDUF0dHJpYnV0ZVR5cGUSHgoaQVRUUklCVVRFX1RZUEVfVU5TUEVDSUZJRUQQABIZChVBVFRSSUJVVEVfVFlQRV9TVFJJTkcQARIXChNBVFRSSUJVVEVfVFlQRV9VVUlEEAIqegoRQXR0cmlidXRlUG9sYXJpdHkSIgoeQVRUUklCVVRFX1BPTEFSSVRZX1VOU1BFQ0lGSUVEEAASHgoaQVRUUklCVVRFX1BPTEFSSVRZX0dSQU5URUQQARIhCh1BVFRSSUJVVEVfUE9MQVJJVFlfTkFSUk9XQUJMRRACKnwKEUF0dHJpYnV0ZU1hdGNoaW5nEiIKHkFUVFJJQlVURV9NQVRDSElOR19VTlNQRUNJRklFRBAAEhwKGEFUVFJJQlVURV9NQVRDSElOR19FWEFDVBABEiUKIUFUVFJJQlVURV9NQVRDSElOR19TRUdNRU5UX1BSRUZJWBACKrcBCglDYWxsU2hhcGUSGgoWQ0FMTF9TSEFQRV9VTlNQRUNJRklFRBAAEhQKEENBTExfU0hBUEVfVU5BUlkQARIcChhDQUxMX1NIQVBFX0NMSUVOVF9TVFJFQU0QAhIcChhDQUxMX1NIQVBFX0ZJTklURV9TVFJFQU0QAxIfChtDQUxMX1NIQVBFX1VOQk9VTkRFRF9TVFJFQU0QBBIbChdDQUxMX1NIQVBFX0xPTkdfUlVOTklORxAFKqcBCgxBY3Rpb25FZmZlY3QSHQoZQUNUSU9OX0VGRkVDVF9VTlNQRUNJRklFRBAAEhYKEkFDVElPTl9FRkZFQ1RfUFVSRRABEhsKF0FDVElPTl9FRkZFQ1RfRVBIRU1FUkFMEAISHQoZQUNUSU9OX0VGRkVDVF9DT01QRU5TQUJMRRADEiQKIEFDVElPTl9FRkZFQ1RfUE9JTlRfT0ZfTk9fUkVUVVJOEAQqbQoPSWRlbXBvdGVuY3lNb2RlEiAKHElERU1QT1RFTkNZX01PREVfVU5TUEVDSUZJRUQQABIcChhJREVNUE9URU5DWV9NT0RFX05BVFVSQUwQARIaChZJREVNUE9URU5DWV9NT0RFX0tFWUVEEAIqxQEKEkRhdGFDbGFzc2lmaWNhdGlvbhIjCh9EQVRBX0NMQVNTSUZJQ0FUSU9OX1VOU1BFQ0lGSUVEEAASHgoaREFUQV9DTEFTU0lGSUNBVElPTl9QVUJMSUMQARIgChxEQVRBX0NMQVNTSUZJQ0FUSU9OX0lOVEVSTkFMEAISJAogREFUQV9DTEFTU0lGSUNBVElPTl9DT05GSURFTlRJQUwQAxIiCh5EQVRBX0NMQVNTSUZJQ0FUSU9OX1JFU1RSSUNURUQQBDpCCgpjYXBhYmlsaXR5Eh8uZ29vZ2xlLnByb3RvYnVmLlNlcnZpY2VPcHRpb25zGI22twIgASgJUgpjYXBhYmlsaXR5OnUKFXJlcXVpcmVkX2NhcGFiaWxpdGllcxIfLmdvb2dsZS5wcm90b2J1Zi5TZXJ2aWNlT3B0aW9ucxiOtrcCIAMoCzIcLmJvbmVzLnYxLlJlcXVpcmVkQ2FwYWJpbGl0eVIUcmVxdWlyZWRDYXBhYmlsaXRpZXM6VwoVcHJvdmlkZWRfY2FwYWJpbGl0aWVzEh8uZ29vZ2xlLnByb3RvYnVmLlNlcnZpY2VPcHRpb25zGI+2twIgAygJUhRwcm92aWRlZENhcGFiaWxpdGllczpqChFjYXBhYmlsaXR5X2xhYmVscxIfLmdvb2dsZS5wcm90b2J1Zi5TZXJ2aWNlT3B0aW9ucxiQtrcCIAMoCzIZLmJvbmVzLnYxLkNhcGFiaWxpdHlMYWJlbFIQY2FwYWJpbGl0eUxhYmVsczpqChFwZXJtaXNzaW9uX2xhYmVscxIfLmdvb2dsZS5wcm90b2J1Zi5TZXJ2aWNlT3B0aW9ucxiRtrcCIAMoCzIZLmJvbmVzLnYxLlBlcm1pc3Npb25MYWJlbFIQcGVybWlzc2lvbkxhYmVsczpvChZwcm92aWRlZF9icm9rZXJfc2NvcGVzEh8uZ29vZ2xlLnByb3RvYnVmLlNlcnZpY2VPcHRpb25zGJK2twIgAygLMhUuYm9uZXMudjEuQnJva2VyU2NvcGVSFHByb3ZpZGVkQnJva2VyU2NvcGVzOnEKE2NhcGFiaWxpdHlfYXVkaWVuY2USHy5nb29nbGUucHJvdG9idWYuU2VydmljZU9wdGlvbnMYk7a3AiABKA4yHC5ib25lcy52MS5DYXBhYmlsaXR5QXVkaWVuY2VSEmNhcGFiaWxpdHlBdWRpZW5jZTprChFjYXBhYmlsaXR5X3Njb3BlcxIfLmdvb2dsZS5wcm90b2J1Zi5TZXJ2aWNlT3B0aW9ucxiUtrcCIAMoCzIaLmJvbmVzLnYxLkNhcGFiaWxpdHlTY29wZXNSEGNhcGFiaWxpdHlTY29wZXM6YAoKYXR0cmlidXRlcxIfLmdvb2dsZS5wcm90b2J1Zi5TZXJ2aWNlT3B0aW9ucxiWtrcCIAMoCzIcLmJvbmVzLnYxLkF0dHJpYnV0ZURpbWVuc2lvblIKYXR0cmlidXRlczo5CgZlcnJvcnMSHi5nb29nbGUucHJvdG9idWYuTWV0aG9kT3B0aW9ucxiLtrcCIAMoCVIGZXJyb3JzOkwKBWF1dGh6Eh4uZ29vZ2xlLnByb3RvYnVmLk1ldGhvZE9wdGlvbnMYjLa3AiABKAsyEy5ib25lcy52MS5BdXRoelJ1bGVSBWF1dGh6OkwKA21jcBIeLmdvb2dsZS5wcm90b2J1Zi5NZXRob2RPcHRpb25zGJe2twIgASgLMhcuYm9uZXMudjEuTWNwUHJvamVjdGlvblIDbWNwOlUKCmNhbGxfc2hhcGUSHi5nb29nbGUucHJvdG9idWYuTWV0aG9kT3B0aW9ucxiatrcCIAEoDjITLmJvbmVzLnYxLkNhbGxTaGFwZVIJY2FsbFNoYXBlOjwKCGJhdGNoX29mEh4uZ29vZ2xlLnByb3RvYnVmLk1ldGhvZE9wdGlvbnMYn7a3AiABKAlSB2JhdGNoT2Y6agoRYXV0b21hdGlvbl9hY3Rpb24SHi5nb29nbGUucHJvdG9idWYuTWV0aG9kT3B0aW9ucxictrcCIAEoCzIaLmJvbmVzLnYxLkF1dG9tYXRpb25BY3Rpb25SEGF1dG9tYXRpb25BY3Rpb246RwoObGlzdF9zdHJlYW1fb2YSHi5nb29nbGUucHJvdG9idWYuTWV0aG9kT3B0aW9ucxijtrcCIAEoCVIMbGlzdFN0cmVhbU9mOlgKCHJlc291cmNlEh8uZ29vZ2xlLnByb3RvYnVmLk1lc3NhZ2VPcHRpb25zGJm2twIgASgLMhguYm9uZXMudjEuUmVzb3VyY2VPcHRpb25SCHJlc291cmNlOm4KEmF1dG9tYXRpb25fdHJpZ2dlchIfLmdvb2dsZS5wcm90b2J1Zi5NZXNzYWdlT3B0aW9ucxibtrcCIAEoCzIbLmJvbmVzLnYxLkF1dG9tYXRpb25UcmlnZ2VyUhFhdXRvbWF0aW9uVHJpZ2dlcjpDCgtpbnN0YW5jZV9vZhIfLmdvb2dsZS5wcm90b2J1Zi5NZXNzYWdlT3B0aW9ucxihtrcCIAEoCVIKaW5zdGFuY2VPZjo0CgRhdHRyEh0uZ29vZ2xlLnByb3RvYnVmLkZpZWxkT3B0aW9ucxiYtrcCIAEoCVIEYXR0cjpmCg5jbGFzc2lmaWNhdGlvbhIdLmdvb2dsZS5wcm90b2J1Zi5GaWVsZE9wdGlvbnMYnba3AiABKA4yHC5ib25lcy52MS5EYXRhQ2xhc3NpZmljYXRpb25SDmNsYXNzaWZpY2F0aW9uOjoKB21lYW5pbmcSHS5nb29nbGUucHJvdG9idWYuRmllbGRPcHRpb25zGKC2twIgASgJUgdtZWFuaW5nOkcKDmRlZmF1bHRlZF9mcm9tEh0uZ29vZ2xlLnByb3RvYnVmLkZpZWxkT3B0aW9ucxiitrcCIAEoCVINZGVmYXVsdGVkRnJvbTo7Cgh0eXBlZF9ieRIdLmdvb2dsZS5wcm90b2J1Zi5GaWVsZE9wdGlvbnMYpLa3AiABKAlSB3R5cGVkQnk6SAoPbmVzdGVkX3dyaXRlX29mEh0uZ29vZ2xlLnByb3RvYnVmLkZpZWxkT3B0aW9ucxiltrcCIAEoCVINbmVzdGVkV3JpdGVPZmIGcHJvdG8z", [file_google_protobuf_descriptor]);

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
export const RequiredCapabilitySchema: GenMessage<RequiredCapability> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 0);

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
  translations: { [key: string]: string };
};

/**
 * Describes the message bones.v1.Label.
 * Use `create(LabelSchema)` to create a new message.
 */
export const LabelSchema: GenMessage<Label> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 1);

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
export const CapabilityLabelSchema: GenMessage<CapabilityLabel> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 2);

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
export const PermissionLabelSchema: GenMessage<PermissionLabel> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 3);

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
export const BrokerScopeSchema: GenMessage<BrokerScope> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 4);

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
export const CapabilityScopesSchema: GenMessage<CapabilityScopes> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 5);

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
export const AuthzRuleSchema: GenMessage<AuthzRule> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 6);

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
};

/**
 * Describes the message bones.v1.AttributeDimension.
 * Use `create(AttributeDimensionSchema)` to create a new message.
 */
export const AttributeDimensionSchema: GenMessage<AttributeDimension> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 7);

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
export const McpProjectionSchema: GenMessage<McpProjection> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 8);

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
export const ResourceOptionSchema: GenMessage<ResourceOption> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 9);

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
export const ResourceMethodsSchema: GenMessage<ResourceMethods> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 10);

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
export const AutomationTriggerSchema: GenMessage<AutomationTrigger> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 11);

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
export const AutomationActionSchema: GenMessage<AutomationAction> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 12);

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
export const EntityResolverSchema: GenMessage<EntityResolver> = /*@__PURE__*/
  messageDesc(file_bones_v1_annotations, 13);

/**
 * Who a product's capability is granted to when an organization enables the
 * product. Each value includes every role of the values before it.
 *
 * @generated from enum bones.v1.CapabilityAudience
 */
export enum CapabilityAudience {
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
}

/**
 * Describes the enum bones.v1.CapabilityAudience.
 */
export const CapabilityAudienceSchema: GenEnum<CapabilityAudience> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 0);

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
export enum AuthzKind {
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
  AUTHENTICATED = 4,
}

/**
 * Describes the enum bones.v1.AuthzKind.
 */
export const AuthzKindSchema: GenEnum<AuthzKind> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 1);

/**
 * The type of values a dimension carries.
 *
 * @generated from enum bones.v1.AttributeType
 */
export enum AttributeType {
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
  UUID = 2,
}

/**
 * Describes the enum bones.v1.AttributeType.
 */
export const AttributeTypeSchema: GenEnum<AttributeType> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 2);

/**
 * What an absent restriction on a dimension means.
 *
 * @generated from enum bones.v1.AttributePolarity
 */
export enum AttributePolarity {
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
  NARROWABLE = 2,
}

/**
 * Describes the enum bones.v1.AttributePolarity.
 */
export const AttributePolaritySchema: GenEnum<AttributePolarity> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 3);

/**
 * How a restriction value is compared with a request value.
 *
 * @generated from enum bones.v1.AttributeMatching
 */
export enum AttributeMatching {
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
  SEGMENT_PREFIX = 2,
}

/**
 * Describes the enum bones.v1.AttributeMatching.
 */
export const AttributeMatchingSchema: GenEnum<AttributeMatching> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 4);

/**
 * What kind of call an RPC is.
 *
 * @generated from enum bones.v1.CallShape
 */
export enum CallShape {
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
  LONG_RUNNING = 5,
}

/**
 * Describes the enum bones.v1.CallShape.
 */
export const CallShapeSchema: GenEnum<CallShape> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 5);

/**
 * What running an action changes in the world.
 *
 * @generated from enum bones.v1.ActionEffect
 */
export enum ActionEffect {
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
  POINT_OF_NO_RETURN = 4,
}

/**
 * Describes the enum bones.v1.ActionEffect.
 */
export const ActionEffectSchema: GenEnum<ActionEffect> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 6);

/**
 * How a repeated invocation of one business operation behaves.
 *
 * @generated from enum bones.v1.IdempotencyMode
 */
export enum IdempotencyMode {
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
  KEYED = 2,
}

/**
 * Describes the enum bones.v1.IdempotencyMode.
 */
export const IdempotencyModeSchema: GenEnum<IdempotencyMode> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 7);

/**
 * How sensitive a field's data is. Each value is more sensitive than the
 * values before it.
 *
 * @generated from enum bones.v1.DataClassification
 */
export enum DataClassification {
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
  RESTRICTED = 4,
}

/**
 * Describes the enum bones.v1.DataClassification.
 */
export const DataClassificationSchema: GenEnum<DataClassification> = /*@__PURE__*/
  enumDesc(file_bones_v1_annotations, 8);

/**
 * Singular predecessor of `provided_capabilities`, still read. Empty or
 * absent means this service is not enrolled as a capability provider.
 *
 * @generated from extension: string capability = 5102349;
 */
export const capability: GenExtension<ServiceOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 0);

/**
 * Capabilities this service must have bound before it can serve, with
 * the authority it intends to exercise against each. Absent means it
 * requires none. See file header for semantics.
 *
 * @generated from extension: repeated bones.v1.RequiredCapability required_capabilities = 5102350;
 */
export const required_capabilities: GenExtension<ServiceOptions, RequiredCapability[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 1);

/**
 * Allocated capabilities this service serves. Naming one does not allocate
 * it. Absent means this service is not enrolled as a capability provider.
 * See file header for semantics.
 *
 * @generated from extension: repeated string provided_capabilities = 5102351;
 */
export const provided_capabilities: GenExtension<ServiceOptions, string[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 2);

/**
 * Human labels for the capabilities this contract provides. See file header.
 *
 * @generated from extension: repeated bones.v1.CapabilityLabel capability_labels = 5102352;
 */
export const capability_labels: GenExtension<ServiceOptions, CapabilityLabel[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 3);

/**
 * Human labels for every scope this contract's RPCs require or it declares
 * in `provided_broker_scopes`. See file header.
 *
 * @generated from extension: repeated bones.v1.PermissionLabel permission_labels = 5102353;
 */
export const permission_labels: GenExtension<ServiceOptions, PermissionLabel[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 4);

/**
 * Scopes of a provided capability that a broker, not an RPC, enforces. See
 * file header.
 *
 * @generated from extension: repeated bones.v1.BrokerScope provided_broker_scopes = 5102354;
 */
export const provided_broker_scopes: GenExtension<ServiceOptions, BrokerScope[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 5);

/**
 * Organization roles this service's capabilities are granted to when an
 * organization enables the product. Absent or UNSPECIFIED means never part
 * of a default grant. See file header.
 *
 * @generated from extension: bones.v1.CapabilityAudience capability_audience = 5102355;
 */
export const capability_audience: GenExtension<ServiceOptions, CapabilityAudience> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 6);

/**
 * Task-level permission groups: each names a provided capability and the
 * scopes it confers. See file header.
 *
 * @generated from extension: repeated bones.v1.CapabilityScopes capability_scopes = 5102356;
 */
export const capability_scopes: GenExtension<ServiceOptions, CapabilityScopes[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 7);

/**
 * Resource attribute dimensions a credential can be narrowed on at this
 * service. Absent means none. See file header.
 *
 * @generated from extension: repeated bones.v1.AttributeDimension attributes = 5102358;
 */
export const attributes: GenExtension<ServiceOptions, AttributeDimension[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 8);

/**
 * Declared error variants for this RPC. Variant names are
 * ALL_CAPS_SNAKE and must match a value in the service's
 * companion error enum. See file header for full semantics.
 *
 * @generated from extension: repeated string errors = 5102347;
 */
export const errors: GenExtension<MethodOptions, string[]> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 9);

/**
 * Declared authorization requirement for this RPC. Absent option
 * means the RPC is denied to every caller. See file header.
 *
 * @generated from extension: bones.v1.AuthzRule authz = 5102348;
 */
export const authz: GenExtension<MethodOptions, AuthzRule> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 10);

/**
 * Opts this RPC into the MCP projection. Absent means not projected. See
 * file header.
 *
 * @generated from extension: bones.v1.McpProjection mcp = 5102359;
 */
export const mcp: GenExtension<MethodOptions, McpProjection> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 11);

/**
 * What kind of call this RPC is. Absent or UNSPECIFIED means derived from
 * the signature. Read by every projection. See file header.
 *
 * @generated from extension: bones.v1.CallShape call_shape = 5102362;
 */
export const call_shape: GenExtension<MethodOptions, CallShape> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 12);

/**
 * The single-item method of the same service this RPC batches. Absent means
 * the RPC is not a batch form. See file header.
 *
 * @generated from extension: string batch_of = 5102367;
 */
export const batch_of: GenExtension<MethodOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 13);

/**
 * Declares this RPC an action of the product's automation surface. Absent
 * means organizations cannot automate it. See file header.
 *
 * @generated from extension: bones.v1.AutomationAction automation_action = 5102364;
 */
export const automation_action: GenExtension<MethodOptions, AutomationAction> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 14);

/**
 * The unary List RPC of the same service whose request this server stream
 * accepts and whose items it streams. Absent means the RPC is not a list
 * stream. See file header.
 *
 * @generated from extension: string list_stream_of = 5102371;
 */
export const list_stream_of: GenExtension<MethodOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 15);

/**
 * Declares this message a managed resource. See file header.
 *
 * @generated from extension: bones.v1.ResourceOption resource = 5102361;
 */
export const resource: GenExtension<MessageOptions, ResourceOption> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 16);

/**
 * Declares this domain-event message a trigger of the product's automation
 * surface. Absent means no automation starts from it. See file header.
 *
 * @generated from extension: bones.v1.AutomationTrigger automation_trigger = 5102363;
 */
export const automation_trigger: GenExtension<MessageOptions, AutomationTrigger> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 17);

/**
 * The fully-qualified name of the message this message is instantiated
 * from. Absent means the message is not an instance of another message. See
 * file header.
 *
 * @generated from extension: string instance_of = 5102369;
 */
export const instance_of: GenExtension<MessageOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 18);

/**
 * The attribute dimension this field carries, naming one declared in the
 * service's `attributes`. See file header.
 *
 * @generated from extension: string attr = 5102360;
 */
export const attr: GenExtension<FieldOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 19);

/**
 * How sensitive this field's data is. Absent reads as
 * DATA_CLASSIFICATION_UNSPECIFIED, the unclassified state. See file header.
 *
 * @generated from extension: bones.v1.DataClassification classification = 5102365;
 */
export const classification: GenExtension<FieldOptions, DataClassification> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 20);

/**
 * What this field denotes, as one term of the curated vocabulary (schema.org
 * core plus the extensions the platform curates), written with its
 * `schema:` prefix: `schema:startDate`. See file header.
 *
 * @generated from extension: string meaning = 5102368;
 */
export const meaning: GenExtension<FieldOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 21);

/**
 * The fully-qualified field (`<package>.<Message>.<field>`) on the
 * `instance_of` message that proposes this field's default. The value stays
 * overridable; absent means the field has no proposed default. See file
 * header.
 *
 * @generated from extension: string defaulted_from = 5102370;
 */
export const defaulted_from: GenExtension<FieldOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 22);

/**
 * F1: The sibling string field holding the definition id that types this
 * bones.v1.TypedValue. Every TypedValue field names its definition this way.
 *
 * @generated from extension: string typed_by = 5102372;
 */
export const typed_by: GenExtension<FieldOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 23);

/**
 * The single child write of the same service that each child in this
 * field passes through when it is written with its parent. Absent means
 * the field is not a nested write. See file header.
 *
 * @generated from extension: string nested_write_of = 5102373;
 */
export const nested_write_of: GenExtension<FieldOptions, string> = /*@__PURE__*/
  extDesc(file_bones_v1_annotations, 24);

