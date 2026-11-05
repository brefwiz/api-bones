# SPDX-License-Identifier: MIT
@library @connect @batch
Feature: Batched reads under an identity guard

  A window of coalesced gets, or the unsent chunks of an explicit batch, are
  keyed to the caller identity they were enqueued under: the credential subject
  and organization. The SDK re-reads the credential before each send and again
  before the answer is delivered. A changed identity fails every caller of the
  window or chunk as unauthenticated with BATCH_IDENTITY_CHANGED, sends nothing
  further, and keeps the results already delivered.

  @db
  Scenario Outline: A <failure> during a grouped continuation preserves delivered items only
    Given the organization "acme" has the widget "complete" with parts "kept"
    And the organization "acme" has the widget "continuing" with parts "prefix,discarded,unsent"
    And the caller is "ada" of organization "acme"
    And the part lists page 1 at a time
    And the caller has started the grouped parts continuation
    When a grouped single-List continuation encounters a "<failure>" in flight
    Then the grouped continuation retains its delivered prefix and independent group
    And the server received 1 "BatchListWidgetParts" request
    And the server received 1 "ListWidgetParts" request

    Examples:
      | failure        |
      | identity change |
      | source failure  |

  Scenario: An identity change before the flush fails the whole window as unauthenticated and sends nothing
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2" at the same time and the token refresh changes the subject before the flush
    Then every position fails with "unauthenticated"
    And every position carries the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received no request

  Scenario: An identity change between chunks fails the unsent chunks and keeps results already delivered after a successful response-time guard
    Given the organization "acme" has 450 widgets
    And the caller is "ada" of organization "acme"
    And the token refresh moves to subject "bob" of organization "acme" once the first chunk is delivered
    When the caller batch-gets the generated widgets
    Then positions 1 to 200 are the generated widgets
    And positions 201 to 450 fail with "unauthenticated"
    And positions 201 to 450 carry the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received 1 "BatchGetWidgets" request

  Scenario: An unchanged identity flushes the window normally
    Given the organization "acme" has widgets "w1,w2,w3"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2,w3" at the same time
    Then the results are the widgets "w1,w2,w3"
    And the server received 1 "BatchGetWidgets" request

  Scenario: An identity change while a chunk is in flight discards the response
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the server switches the caller to subject "bob" of organization "acme" while it handles a batch request
    When the caller batch-gets "w1,w2"
    Then every position fails with "unauthenticated"
    And every position carries the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received 1 "BatchGetWidgets" request

  Scenario: An identity change during an unavailable batch request prevents its retry
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the server switches the caller to subject "bob" of organization "acme" while it handles a batch request
    And the server fails the first batch request with "unavailable"
    When the caller batch-gets "w1,w2"
    Then every position fails with "unauthenticated"
    And every position carries the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received 1 "BatchGetWidgets" request

  Scenario Outline: A token refresh that changes the <what> between chunks fails the unsent chunks with BATCH_IDENTITY_CHANGED
    Given the organization "acme" has 450 widgets
    And the caller is "ada" of organization "acme"
    And the token refresh changes the <what> once the first chunk is delivered
    When the caller batch-gets the generated widgets
    Then positions 1 to 200 are the generated widgets
    And positions 201 to 450 carry the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received 1 "BatchGetWidgets" request

    Examples:
      | what         |
      | subject      |
      | organization |

  Scenario: An identity change before a group continuation fails that continuation and sends nothing
    Given the organization "acme" has the widget "w1" with parts "a,b"
    And the caller is "ada" of organization "acme"
    And the part lists page 1 at a time
    And the caller has started the parts of "w1"
    And the caller is "bob" of organization "acme"
    When the caller continues the seeded parts group
    Then the group continuation fails with "unauthenticated" and ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received 1 "BatchListWidgetParts" request
    And the server received 0 "ListWidgetParts" requests

  Scenario: A client whose credentials are not bound to the batch runtime is refused at construction
    Given the caller is "ada" of organization "acme"
    When the SDK constructs a batched client with credentials outside its session binding
    Then batch client construction is refused
    And the server received no request

  Scenario: A batch method without a declared side-effect-free contract is refused at construction
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the SDK constructs a batched client with "mutating batch" policy drift
    Then construction fails with "must declare NO_SIDE_EFFECTS"
    And the server received no request

  Scenario: A session source failure before flush fails closed without sending
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2" and the session source fails before flush
    Then every position fails with "unauthenticated"
    And every position carries the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received no request

  Scenario: A session source failure during a request discards its response
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the session source fails while the server handles a batch request
    When the caller batch-gets "w1,w2"
    Then every position fails with "unauthenticated"
    And every position carries the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received 1 "BatchGetWidgets" request

  Scenario Outline: A same-identity source returning <failure> before flush sends nothing
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the bound session supplies a relayed credential
    When the callers get "w1,w2" and the source returns "<failure>" before flush
    Then every position fails with "unauthenticated"
    And every position carries the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received no request

    Examples:
      | failure          |
      | missing bearer   |
      | missing relay    |
      | malformed bearer |
      | malformed relay  |

  Scenario Outline: A same-identity source returning <failure> during a request discards its response
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the bound session supplies a relayed credential
    And the source returns "<failure>" while the server handles a batch request
    When the caller batch-gets "w1,w2"
    Then every position fails with "unauthenticated"
    And every position carries the ErrorInfo "BATCH_IDENTITY_CHANGED"
    And the server received 1 "BatchGetWidgets" request

    Examples:
      | failure          |
      | missing bearer   |
      | missing relay    |
      | malformed bearer |
      | malformed relay  |
