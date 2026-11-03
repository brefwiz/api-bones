# SPDX-License-Identifier: MIT
@library @connect @batch
Feature: Batched reads

  A BatchGet answers every requested id on its own, in request order: one
  id's expected refusal never fails the others. Unexpected server faults fail
  the whole call and pass through service error redaction. The SDK sends an
  explicit batch of any length as chunks of at most 200 ids and 1 MiB, and merges the single Gets of
  one tick into one batch request. Calls under different identities, headers,
  deadlines or non-id fields never merge. A BatchList answers one group per
  parent, and a group continues through the single List cursor.

  The server holds the widgets of several organizations and reads the caller's
  organization from its bearer token. It records every request, so a step reads
  what the SDK actually sent. At most 4 batch requests are in flight per client
  and at most 64 chunks queue per identity.

  Scenario: Explicit batch get returns one result per id in request order
    Given the organization "acme" has widgets "w1,w2,w3"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w3,w1,w2"
    Then the results are the widgets "w3,w1,w2"
    And the server received 1 "BatchGetWidgets" request

  Scenario: A provider refuses more than 200 ids with BATCH_TOO_LARGE
    Given the organization "acme" has widgets "w1"
    And the caller is "ada" of organization "acme"
    When the provider is asked for batch results with 201 ids
    Then the whole batch call fails with "invalid_argument" and ErrorInfo "BATCH_TOO_LARGE"
    And the provider performs no per-item work

  Scenario: An unexpected item authorization fault fails the whole batch with a redacted error
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And authorizing widget "w2" fails internally with "private authorization fault"
    When the caller requests a batch for "w1,w2" while authorization fails internally
    Then every item reports the same redacted internal error
    And no item error contains "private authorization fault" or the requested ids

  Scenario: Duplicate ids are fetched once and answered at every position
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w1,w2,w1"
    Then the results are the widgets "w1,w2,w1"
    And the requests carried the ids "w1,w2"

  Scenario: An empty id list returns no results and sends no request
    Given the organization "acme" has widgets "w1"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets ""
    Then there are no results
    And the server received no request

  Scenario: A malformed id fails only its own item with invalid_argument
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w1,bad id,w2"
    Then position 1 is the widget "w1"
    And position 2 fails with "invalid_argument"
    And position 2 carries the ErrorInfo "WIDGET_ID_MALFORMED"
    And position 3 is the widget "w2"

  Scenario: An item error carries the same code and ErrorInfo as the single Get
    Given the organization "acme" has widgets "w1"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w1,ghost"
    And the caller gets "ghost" with the unbatched client
    Then position 1 is the widget "w1"
    And position 2 fails with "not_found"
    And position 2 carries the code, message and ErrorInfo of the unbatched get

  Scenario: A cross-org id is answered exactly like an unknown id
    Given the organization "acme" has widgets "w1"
    And the organization "globex" has widgets "g1"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w1,g1,ghost"
    Then position 1 is the widget "w1"
    And position 2 fails with "not_found"
    And positions 2 and 3 fail identically

  Scenario: The SDK splits 450 ids into chunks of 200, 200 and 50 and keeps input order
    Given the organization "acme" has 450 widgets
    And the caller is "ada" of organization "acme"
    When the caller batch-gets the generated widgets
    Then the results are the generated widgets
    And the batch requests carried the id counts "200,200,50"

  Scenario: The SDK closes a chunk at 1 MiB encoded size before 200 ids
    Given the organization "acme" has 120 widgets with ids of 10000 bytes
    And the caller is "ada" of organization "acme"
    When the caller batch-gets the generated widgets
    Then the results are the generated widgets
    And the server received 2 "BatchGetWidgets" requests
    And the first batch request carried fewer than 200 ids
    And no request was larger than 1 MiB

  Scenario: An id whose request alone exceeds 1 MiB fails locally with BATCH_ITEM_TOO_LARGE
    Given the organization "acme" has widgets "w1"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w1" and one id of 1048600 bytes
    Then position 1 is the widget "w1"
    And position 2 fails with "invalid_argument"
    And position 2 carries the ErrorInfo "BATCH_ITEM_TOO_LARGE"
    And the requests carried the ids "w1"

  Scenario: The SDK keeps at most 4 wire batch requests in flight per client instance across coalesced windows and explicit chunks
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the server takes 100 ms to answer
    And 4 explicit batches and 3 coalesced windows of two gets are waiting
    When the final coalesced window 3 gets "w1,w2"
    Then all prepared batch calls have finished
    And every result is a widget
    And the server never had more than 4 batch requests in flight

  Scenario: A failed chunk fails only its own positions
    Given the organization "acme" has 450 widgets
    And the caller is "ada" of organization "acme"
    And the server fails the batch request carrying "w-250" with "internal"
    When the caller batch-gets the generated widgets
    Then positions 1 to 200 are the generated widgets
    And positions 201 to 400 fail with "internal"
    And positions 401 to 450 are the generated widgets
    And batch telemetry has exactly the wire outcomes "ok,error,ok"

  Scenario Outline: A response whose ids or parent ids do not match the request fails that chunk with BATCH_RESPONSE_MISMATCH
    Given the organization "acme" has widgets "w1,w2,w3"
    And the caller is "ada" of organization "acme"
    And the batch response has mismatched "<shape>"
    When the caller receives a malformed batch "<operation>" response
    Then every malformed batch response fails its chunk with "BATCH_RESPONSE_MISMATCH"
    And batch telemetry has exactly the wire outcomes "mismatch"

    Examples:
      | operation | shape |
      | get       | order |
      | get       | count |
      | list      | order |
      | list      | count |

  Scenario: Concurrent single gets in one tick reach the server as one BatchGet
    Given the organization "acme" has widgets "w1,w2,w3"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2,w3" at the same time
    Then the results are the widgets "w1,w2,w3"
    And the server received 1 "BatchGetWidgets" request
    And the server received 0 "GetWidget" requests
    And the requests carried the ids "w1,w2,w3"

  Scenario: A lone single get in a window uses the single Get RPC
    Given the organization "acme" has widgets "w1"
    And the caller is "ada" of organization "acme"
    When the callers get "w1" at the same time
    Then the results are the widgets "w1"
    And the server received 1 "GetWidget" request
    And the server received 0 "BatchGetWidgets" requests
    And no batch span or completion is emitted

  Scenario: Gets under different credentials are never merged
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2" under different tokens
    Then the results are the widgets "w1,w2"
    And the server received 2 "GetWidget" requests
    And the server received 0 "BatchGetWidgets" requests

  Scenario: Gets with different per-call headers or deadlines are never merged
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2" with the x-trace values "one,two"
    Then the results are the widgets "w1,w2"
    And the requests carried the x-trace values "one,two"
    And the server received 0 "BatchGetWidgets" requests
    When the callers get "w1,w2" with the deadlines "5000,6000" ms
    Then the results are the widgets "w1,w2"
    And the server received 4 "GetWidget" requests
    And the server received 0 "BatchGetWidgets" requests

  Scenario: Gets with different non-id request fields are never merged
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2" with the kinds "red,blue"
    Then the widget at position 1 echoes the kind "red"
    And the widget at position 2 echoes the kind "blue"
    And the server received 2 "GetWidget" requests
    And the server received 0 "BatchGetWidgets" requests

  Scenario: The same id requested twice in one window is fetched once for both callers
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w1,w2" at the same time
    Then the results are the widgets "w1,w1,w2"
    And the server received 1 "BatchGetWidgets" request
    And the requests carried the ids "w1,w2"

  Scenario: A window that reaches 200 ids flushes immediately
    Given the organization "acme" has 250 widgets
    And the caller is "ada" of organization "acme"
    When the callers get the generated widgets at the same time
    Then the results are the generated widgets
    And the batch requests carried the id counts "200,50" in any order
    And the server received 0 "GetWidget" requests

  Scenario: Aborting before the flush removes the caller's id from the batch
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the caller aborts the get of "w2" before the flush while "w1" waits
    Then the results are the widgets "w1"
    And the requests carried the ids "w1"
    And the server received 0 "BatchGetWidgets" requests

  Scenario: Aborting after the flush detaches only that caller
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the caller aborts the get of "w2" after the flush while "w1" waits
    Then the results are the widgets "w1"
    And the server received 1 "BatchGetWidgets" request
    And the requests carried the ids "w1,w2"

  Scenario: Aborting an explicit batch sends no further chunks
    Given the organization "acme" has 450 widgets
    And the caller is "ada" of organization "acme"
    When the caller aborts the batch-get of the generated widgets after its first chunk was sent
    Then there are no results
    And the server received 1 "BatchGetWidgets" request
    And batch telemetry has exactly the wire outcomes "cancelled"

  Scenario: A batch fetch retries only through the existing retry policy
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the server fails the first batch request with "unavailable"
    When the caller batch-gets "w1,w2"
    Then every result is a widget
    And the server received 2 "BatchGetWidgets" requests

  Scenario: A grouped batch list returns one group per parent in request order
    Given the organization "acme" has the widget "w1" with parts "a,b"
    And the organization "acme" has the widget "w2" with parts "c"
    And the caller is "ada" of organization "acme"
    When the caller batch-lists the parts of "w2,w1"
    Then group 1 is for "w2" and lists the parts "c"
    And group 2 is for "w1" and lists the parts "a,b"
    And the server received 1 "BatchListWidgetParts" request

  Scenario: A group continues through the page iterator with the single List cursor
    Given the organization "acme" has the widget "w1" with parts "a,b,c,d,e"
    And the part lists page 2 at a time
    And the caller is "ada" of organization "acme"
    And the caller supplies revision cache headers with ordinary metadata
    And the caller has batch-listed the parts of "w1"
    When the consumer continues parts group 1
    Then group 1 is for "w1" and lists the parts "a,b,c,d,e"
    And the server received 1 "BatchListWidgetParts" request
    And the single list requests carried the cursors "c2,c4"
    And group continuation requests omit revision cache headers and preserve ordinary metadata

  Scenario: An unknown parent yields a not_found group while the other groups succeed
    Given the organization "acme" has the widget "w1" with parts "a"
    And the organization "acme" has the widget "w2" with parts "b"
    And the caller is "ada" of organization "acme"
    When the caller batch-lists the parts of "w1,ghost,w2"
    Then group 1 is for "w1" and lists the parts "a"
    And group 2 fails with "not_found"
    And group 3 is for "w2" and lists the parts "b"

  Scenario: Each sent batch records one bones.batch span with its item count and merged-call count
    Given the organization "acme" has widgets "w1,w2,w3"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2,w3" at the same time
    Then the results are the widgets "w1,w2,w3"
    And one bones.batch record names "contract.v1.Widgets/BatchGetWidgets" with 3 items and 3 calls

  Scenario: A cross-org parent is answered exactly like an unknown parent
    Given the organization "acme" has the widget "w1" with parts "a"
    And the organization "globex" has the widget "g1" with parts "x,y"
    And the caller is "ada" of organization "acme"
    When the caller batch-lists the parts of "w1,g1,ghost"
    Then group 1 is for "w1" and lists the parts "a"
    And group 2 fails with "not_found"
    And groups 2 and 3 fail identically
    And the server received 0 "ListWidgetParts" requests

  Scenario: One identity's overflow fails only its own calls
    Given the organization "acme" has widgets "w1"
    And the caller is "ada" of organization "acme"
    And 72 calls of the first token and 4 calls of the second token are batch-getting "w1"
    When the second token makes its final batch-get of "w1"
    Then all prepared batch calls have finished
    And exactly 4 calls of the first token fail with "resource_exhausted"
    And every other call of the first token is the widget "w1"
    And every call of the second token is the widget "w1"
    And the server received one batch request for each call that was served

  Scenario: Wire slots are admitted fairly across identities
    Given the organization "acme" has widgets "w1"
    And the caller is "ada" of organization "acme"
    And the server takes 50 ms to answer
    And 12 calls of the first token and 1 calls of the second token are batch-getting "w1"
    When the second token makes its final batch-get of "w1"
    Then all prepared batch calls have finished
    And every call is the widget "w1"
    And the second token's requests were admitted before the first token's backlog drained

  @db
  Scenario: An object-level denial of the single Get is answered identically through a coalesced batch
    Given the organization "acme" has widgets "w1,w2"
    And the widget "w2" of organization "acme" is owned by "bob"
    And the caller is "ada" of organization "acme"
    When the callers get "w2,w1" at the same time
    Then the coalesced denial of "w2" has the single get's code, message and ErrorInfo
    And position 1 fails with "permission_denied"
    And position 2 is the widget "w1"
    And the stored owner of widget "w2" is "bob"
    And the server received 1 "GetWidget" request
    And the server received 1 "BatchGetWidgets" request

  Scenario: Metadata producers are captured before enqueue and never mutate a merged request
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the metadata producer attaches the x-trace value "captured"
    When the callers get "w1,w2" while the metadata producer changes to "changed" after enqueue
    Then the results are the widgets "w1,w2"
    And every request carried the captured x-trace value "captured"
    And the server received 1 "BatchGetWidgets" request

  Scenario: Effective deadline metadata separates otherwise identical gets
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the callers get "w1,w2" with the deadlines "5000,6000" ms
    Then the results are the widgets "w1,w2"
    And the single requests carried distinct timeout headers
    And the server received 0 "BatchGetWidgets" requests

  Scenario Outline: Per-call credential overrides are refused before a batch is sent
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w1,w2" with a per-call "<header>" credential override
    Then every position fails with "invalid_argument"
    And the server received no request

    Examples:
      | header                     |
      | authorization              |
      | brefwiz-relayed-credential |
