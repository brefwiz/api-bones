# SPDX-License-Identifier: MIT
@library @connect @db
Feature: Paginated lists

  A List RPC returns one page of items and the cursor of the next. The SDK's
  iterator follows the cursor to the end, so a caller reads a whole list as one
  stream of items or of pages. Widget parts live in PostgreSQL, with database
  row isolation and generated SDK reads and writes. The list revision is the
  stored row version; remembered revision names refer to observed database
  versions rather than values supplied to the provider.

  The iterator fetches at most one page ahead of the consumer and aborts that
  fetch when the consumer stops. It repeats a page fetch only when the RPC is
  declared free of side effects or idempotent, and only after a failure that is
  safe to repeat. When a fetch fails after items were delivered, the delivered
  items stay delivered and the stream ends with one error that counts them.

  The server clamp above the maximum is proven in service-kit's keyset helper
  e2e, not here.

  Scenario Outline: Collection total kinds reach the page consumer with canonical wire and JSON names
    Given the server reports a total of kind <number>
    When the caller fetches the first page with a list call
    Then the page total kind is "<kind>"
    And the total kind travels as <number> on the wire and "<name>" in JSON

    Examples:
      | number | name          | kind     |
      | 1      | KIND_ESTIMATE | estimate |
      | 2      | KIND_EXACT    | exact    |

  Scenario: The iterator follows the cursor to the last page
    Given a list of pages "1,2" "3,4" "5"
    When the consumer reads every item through the platform list call
    Then the consumer receives items "1,2,3,4,5"
    And the server saw the cursors ",c2,c4"
    And the stream ended without an error

  Scenario: The pages keep their boundaries and their revision
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1.abc"
    When the consumer reads every page
    Then the consumer receives pages "1,2|3,4|5"
    And every page carries the revision "r1.abc"

  Scenario: Platform composition shares retained list pages across clones and isolates new clients
    Given a list of pages "1,2" "3,4" "5"
    When the platform client batch-gets the parent and replays its list call across a clone
    Then the consumer receives items "1,2,3,4,5"
    And an independently constructed platform client fetches its own pages

  Scenario: The page size hint reaches the server
    Given a list of pages "1,2" "3"
    When the consumer reads every item with page size 50
    Then the server saw the page size 50 on every request

  Scenario: No page size hint reaches the server as no size
    Given a list of pages "1,2" "3"
    When the consumer reads every item
    Then the server saw no page size on every request
    And the server saw 2 requests

  Scenario: A page size of 0 reaches the server as no size
    Given a list of pages "1,2" "3"
    When the consumer reads every item with page size 0
    Then the server saw no page size on every request
    And the server saw 2 requests

  Scenario: One page ahead is fetched while the consumer holds the first
    Given a list of pages "1" "2" "3" "4"
    When the consumer reads one page and stops
    Then the server saw at most 2 requests

  Scenario: No page is fetched ahead when prefetch is off
    Given a list of pages "1" "2" "3"
    When the consumer reads one page with prefetch off and stops
    Then the consumer receives pages "1"
    And the server saw 1 request

  Scenario: Stopping aborts the fetch that is ahead
    Given a list of pages "1" "2"
    And the stored list read for page 2 waits on another transaction
    When the consumer reads one page and stops
    Then the fetch for page 2 was aborted
    And the list completion records cancelled true without full-list time

  Scenario: A failure after delivered items ends the stream with one error
    Given a list of pages "1,2" "3,4" "5"
    And the consumer has delivered 2 stored list pages
    And the owner transfers the stored widget to "bob"
    When the consumer resumes the stored list
    Then the consumer receives items "1,2,3,4"
    And the stream ended with an error after 2 pages and 4 items
    And the error is an RPC failure with code "permission_denied"
    And the error is not a rejected cursor
    And the list completion records cancelled false without full-list time
    And the stored widget owner is "bob"

  Scenario Outline: A refused cursor is a typed error
    Given a list of pages "1" "2"
    And the consumer has delivered 1 stored list pages
    And a Connect refusal carries cursor code "<code>"
    When the consumer classifies the cursor refusal as a page error
    Then the consumer receives items "1"
    And the cursor refusal is classified as kind "<kind>"
    And the classified refusal preserves delivery context of 1 pages and 1 items

    Examples:
      | code                  | kind                  |
      | CURSOR_INVALID        | cursor_invalid        |
      | CURSOR_EXPIRED        | cursor_expired        |
      | CURSOR_QUERY_MISMATCH | cursor_query_mismatch |

  Scenario: A repeatable failure of a side-effect-free list is retried
    Given a list of pages "1" "2"
    And the RPC is declared "NO_SIDE_EFFECTS"
    And the widgets provider stops before page 2 and restarts after 2 failed attempts
    When the consumer reads every item
    Then the consumer receives items "1,2"
    And the SDK made 2 failed page attempts
    And the server saw 1 requests for the cursor "c1"
    And the stream ended without an error

  Scenario: An idempotent list is retried
    Given a list of pages "1" "2"
    And the RPC is declared "IDEMPOTENT"
    And the widgets provider stops before page 2 and restarts after 1 failed attempts
    When the consumer reads every item
    Then the consumer receives items "1,2"
    And the SDK made 1 failed page attempts
    And the server saw 1 requests for the cursor "c1"

  Scenario: Retrying gives up after a fixed number of repeats
    Given a list of pages "1" "2"
    And the RPC is declared "NO_SIDE_EFFECTS"
    And the widgets provider stops before page 2 and restarts after 10 failed attempts
    When the consumer reads every item
    Then the stream ended with an error after 1 pages and 1 items
    And the SDK made 3 failed page attempts
    And the server saw 0 requests for the cursor "c1"

  Scenario: A failure that is not safe to repeat is not retried
    Given a list of pages "1" "2"
    And the consumer has delivered 1 stored list pages
    And the owner transfers the stored widget to "bob"
    When the consumer resumes the stored list
    Then the server saw 1 requests for the cursor "c1"
    And the stream ended with an error after 1 pages and 1 items
    And the stored widget owner is "bob"

  Scenario Outline: A list not declared free of side effects is never retried
    Given a list of pages "1" "2"
    And the RPC is declared "<contract>"
    And the widgets provider stops before page 1 and restarts after 1 failed attempts
    When the consumer reads every item
    Then the SDK made 1 failed page attempts
    And the server saw 0 requests for the cursor ""
    And the stream ended with an error after 0 pages and 0 items

    Examples:
      | contract       |
      | NON_IDEMPOTENT |
      | UNSPECIFIED    |

  Scenario: A single page is one call
    Given a list of pages "1,2" "3"
    When the consumer reads the first page only
    Then the consumer receives pages "1,2"
    And the server saw 1 request

  Scenario: An iteration continues from a first page already in hand
    Given a list of pages "1,2" "3,4" "5"
    And the caller holds the first stored list page
    When the consumer continues from the held first page
    Then the consumer receives pages "1,2|3,4|5"
    And the server saw the cursors "c2,c4"

  Scenario: An iteration from a first page with no next cursor makes no call
    Given a list of pages "1,2"
    And the caller holds the first stored list page
    When the consumer continues from the held first page
    Then the consumer receives pages "1,2"
    And the server saw 0 requests

  Scenario: A generated list replays retained pages after revision revalidation
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    When the consumer lists again
    Then every generated list page is marked from cache true
    And the first revalidation carried if-none-match "\"r1\""
    And the server saw 4 requests
    And the list completion records 3 pages and 1 not-modified hits

  Scenario: A chain fetched across a write is never replayed
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page across a write changing revision to "r2"
    When the consumer lists again
    Then every generated list page is marked from cache false
    And the first revalidation carried if-none-match ""
    And every generated list page carries revision "r2"
    And the stored widget parts are "1,2,3,4,5,6"
    And the server saw 6 requests

  Scenario: A changed revision replaces the retained chain
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    Given the owner replaces stored widget parts with "1,2,3,4,5,6" and remembers revision as "r2"
    When the consumer lists again
    Then every generated list page is marked from cache false
    And the first revalidation carried if-none-match "\"r1\""
    And every generated list page carries revision "r2"
    And the stored widget parts are "1,2,3,4,5,6"
    And the server saw 6 requests

  Scenario: Released page holders cannot be replayed
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    Given the consumer releases retained list pages
    When the consumer lists again
    Then every generated list page is marked from cache false
    And the first revalidation carried if-none-match ""
    And the server saw 6 requests

  Scenario: Changing identity clears cached pages before revalidation
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    Given the caller identity changes to "bob:org-b"
    When the consumer lists again
    Then every generated list page is marked from cache false
    And the first revalidation carried if-none-match ""
    And the server saw 6 requests

  Scenario: An unauthenticated response clears cached chains
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    Given the producer credential is no longer valid
    When the consumer lists again
    Then the revision cache is empty
    And the paused replay ends with an unauthenticated error

  Scenario: Identity changes stop a paused cached replay
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    And the consumer has paused a cached replay
    When the consumer changes identity to "bob:org-b" and resumes the cached replay
    Then the paused replay ends with an unauthenticated error
    And the revision cache is empty

  Scenario: Clearing a cache stops a paused cached replay
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    And the consumer has paused a cached replay
    When the consumer clears the revision cache and resumes the cached replay
    Then the paused replay ends with an unauthenticated error
    And the revision cache is empty

  Scenario: A matching conditional revision without opt-in returns a normal page
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    When the caller makes a list call with a matching revision without the list cache opt-in
    Then the consumer receives items "1,2"
    And the server saw 1 request

  Scenario: Batch-seeded chains bypass revision-cache reads and writes
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the caller holds the first grouped widget list page
    When the consumer continues a batch-seeded generated list
    Then every generated list page is marked from cache false
    And the batch continuation sent no conditional revision
    And the revision cache is empty
    And the server saw 2 requests

  Scenario: A batch-seeded chain leaves an existing revision chain unchanged
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    Given the owner replaces stored widget parts with "1,2,3,4,5,6" and remembers revision as "r2"
    Given the caller holds the first grouped widget list page
    And the consumer has continued a batch-seeded generated list
    When the consumer lists again
    Then every generated list page is marked from cache false
    And the first revalidation carried if-none-match "\"r1\""
    And the server saw 8 requests
    And the stored widget parts are "1,2,3,4,5,6"

  Scenario: Explicit cache bypass preserves a previously retained revision
    Given a list of pages "1,2" "3,4" "5"
    And the current stored list revision is remembered as "r1"
    And the generated list client uses a revision cache
    Given the consumer retains every generated list page
    Given the owner replaces stored widget parts with "1,2,3,4,5,6" and remembers revision as "r2"
    And the consumer has listed bypassing the revision cache
    When the consumer lists again
    Then every generated list page is marked from cache false
    And the first revalidation carried if-none-match "\"r1\""
    And the server saw 9 requests
    And the stored widget parts are "1,2,3,4,5,6"

  Scenario Outline: Identity changes during a network page discard that response
    Given a list of pages "1,2" "3,4" "5"
    And the generated list client uses a revision cache
    And the stored list read for page <page> waits on another transaction
    When the consumer changes identity to "bob:org-b" while page <page> is in flight with prefetch <prefetch>
    Then the stream ended with an error after <delivered> pages and <items> items
    And the paused replay ends with an unauthenticated error
    And the revision cache is empty

    Examples:
      | page | prefetch | delivered | items |
      | 1    | 0        | 0         | 0     |
      | 2    | 1        | 1         | 2     |

  Scenario Outline: Clearing the cache during a network page discards that response
    Given a list of pages "1,2" "3,4" "5"
    And the generated list client uses a revision cache
    And the stored list read for page <page> waits on another transaction
    When the consumer clears the revision cache while page <page> is in flight with prefetch <prefetch>
    Then the stream ended with an error after <delivered> pages and <items> items
    And the paused replay ends with an unauthenticated error
    And the revision cache is empty

    Examples:
      | page | prefetch | delivered | items |
      | 1    | 0        | 0         | 0     |
      | 2    | 1        | 1         | 2     |
