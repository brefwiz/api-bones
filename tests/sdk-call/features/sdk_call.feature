# SPDX-License-Identifier: MIT
@library @sdk-call
Feature: Every Layer 2 call runs through one helper

  A Layer 2 method never decides its call budget, its trace context or its
  caller identity check itself. One helper decides them once, in Rust: every
  send states the time left of one thirty-second budget, carries a W3C
  traceparent that continues the caller's trace, and is refused when the
  caller's identity changes before the send or before the answer is delivered.
  A list owns one budget per page and one client span per page, and a walk
  that starts as one caller never continues as another. A batch owns one
  budget for the whole call. A stream owns the budget only until its response
  head arrives; its ticks after that are bound by nothing but the stream's own
  lifetime. A server that refuses the caller as unauthenticated is refreshed
  once and replayed: the credential source is asked to refresh, and a renewed
  credential for the same caller is sent in a replay that the retry budget does
  not count. Calls refused on the same credential at once share that one
  refresh. The host is told only when the refresh fails to cure the refusal,
  and the refusal drops the caller's revision cache; so does a changed
  identity, and so does the end of the session: a credential source that
  rejects or answers no credential after a sign-out drops every page the
  session held. A refusal before a stream's first tick is the open's and is
  refreshed and replayed once; a watch that has delivered a tick is never
  replayed, and its refusal is told to the host. A
  revision cache never replays the pages of one server address to a client of
  another. A credential source that cannot answer fails the call closed,
  before anything is sent. Every call opens one client span when it starts and ends it with its
  outcome. Every send carries the credential read just before it, and an
  anonymous caller sends none. A list, a batch or a stream refuses a read that
  starts while another is pending without disturbing the pending one, and
  cancelling it abandons the pending read, gives nothing after it and ends its
  walk once. A list walks its items as well as its pages, in order, and a
  caller who leaves an item walk early releases the list. A cursor the server
  refuses as expired ends the walk with a page error that says how many pages
  and items were delivered; a page that cannot be decoded ends it too. The
  first page of a list can be read on its own, empty or not, and says whether
  a next page follows. An open list exposes the span of its walk while the
  tracer records, and none when it does not. A caller who leaves a stream
  early releases it. A client built with a default page size asks the server
  for pages of that size when a list call names none. A server that throttles
  a call and names when it will be ready is waited out and asked again, inside
  the same budget; a wait longer than five seconds is not sat out, and the call
  is refused as rate limited.
  A static export and a constant export need no client and no server: the
  function is called by its name and the constant is read through a function
  that takes nothing, the same in Rust and in the generated package once its
  module is loaded.

  @state:happy
  Scenario: The parts of a widget are listed page by page through the SDK
    Given the widgets server has the widget "w1" named "Anvil"
    And the widget "w1" has the parts "bolt,nut|washer"
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the parts of the widget "w1" through the SDK
    Then the caller received the part pages "bolt,nut|washer"
    And the server saw 2 request(s)
    And every request the server saw carried a valid traceparent

  @state:happy
  Scenario: The parts listed are those of the widget the request names
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server has the widget "w2" named "Hammer"
    And the widget "w1" has the parts "bolt,nut"
    And the widget "w2" has the parts "handle"
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the parts of the widget "w2" through the SDK
    Then the caller received the part pages "handle"
    And the server saw 1 request(s)

  @state:happy
  Scenario: A part list the server confirms unchanged is replayed from the revision cache
    Given the widgets server has the widget "w1" named "Anvil"
    And the widget "w1" has the parts "bolt,nut|washer"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the parts of the widget "w1"
    When the caller lists the parts of the widget "w1" through the SDK
    Then the caller received the part pages "bolt,nut|washer"
    And every page the caller received came from the revision cache
    And the server saw 1 request(s)

  @state:error
  Scenario: A part cursor the server refuses as expired ends the walk with a PageError
    Given the widgets server has the widget "w1" named "Anvil"
    And the widget "w1" has the parts "bolt|nut|washer"
    And the widgets server refuses the cursor of part page 3 as expired
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the parts of the widget "w1" through the SDK
    Then the caller received the part pages "bolt|nut"
    And the walk is refused with a PageError of kind "cursor_expired" after 2 pages and 2 items

  @state:happy
  Scenario: A part list asks the server for pages of the size the caller names
    Given the widgets server has the widget "w1" named "Anvil"
    And the widget "w1" has the parts "bolt,nut|washer"
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the parts of the widget "w1" with the page size 2 through the SDK
    Then the caller received the part pages "bolt,nut|washer"
    And every request the server saw asked for the part page size 2

  @state:error
  Scenario: The parts of a widget the server does not know are refused as not found
    Given the widgets server has the widget "w1" named "Anvil"
    And the widget "w1" has the parts "bolt,nut|washer"
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the parts of the widget "w9" through the SDK
    Then the call is refused with the platform code "RESOURCE_NOT_FOUND"
    And the caller received the part pages ""
    And the server saw 1 request(s)

  @state:happy
  Scenario: A call with no trace context starts a new trace
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the server saw 1 request(s)
    And every request the server saw carried a valid traceparent

  @state:happy
  Scenario: A call inside a caller trace continues that trace
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the caller is inside the trace "0af7651916cd43dd8448eb211c80319c" at the span "b7ad6b7169203331"
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And every request the server saw carried a traceparent in the trace "0af7651916cd43dd8448eb211c80319c" under a span other than "b7ad6b7169203331"

  @state:happy
  Scenario: Each call states the call budget
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And every request the server saw stated a call budget above 29000 ms and at most 30000 ms

  @state:happy
  Scenario: A batch's call budget starts when the batch is first read
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server has the widget "w2" named "Bolt"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a batch of the widgets "w1,w2" through the SDK
    When the caller waits 1100 ms
    And the caller reads the batch
    Then the batch delivered the widgets "w1=Anvil,w2=Bolt" in request order
    And the server saw 1 request(s)
    And every request the server saw stated a call budget above 29000 ms and at most 30000 ms

  @state:happy
  Scenario: A retried call keeps one deadline
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server refuses the first 2 requests as unavailable
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the server saw 3 request(s)
    And the call budget the server saw shrank from each request to the next

  @state:happy
  Scenario: A throttled call is asked again after the wait the server named
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server refuses the first 1 requests as resource exhausted with a Retry-After of 1 seconds
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the server saw 2 request(s)

  @state:error
  Scenario: A wait past five seconds is not sat out
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server refuses the first 1 requests as resource exhausted with a Retry-After of 6 seconds
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the platform code "RATE_LIMITED"
    And the refusal names a wait of 6000 ms
    And the server saw 1 request(s)

  @state:error
  Scenario: A failure that is not transient is not asked again
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server fails the first 2 requests as internal
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the platform code "INTERNAL_SERVER_ERROR"
    And the server saw 1 request(s)

  @state:error
  Scenario: An identity change before the send refuses the call and sends nothing
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the caller's identity changes to "bob" before the first send
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "IDENTITY_CHANGED"
    And the server saw 0 request(s)

  @state:error
  Scenario: An identity change after the response refuses the delivery
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the server switches the caller to "bob" while it handles a request
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "IDENTITY_CHANGED"
    And the server saw 1 request(s)

  @state:happy
  Scenario: A list gives each page its own call budget
    Given the numbers server has the pages "1,2|3,4|5"
    And the numbers server takes 1100 ms to answer each page
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller has read the next page of the list
    And the caller has read the next page of the list
    When the caller reads the next page of the list
    Then the caller received the pages "1,2|3,4|5"
    And the server saw 3 request(s)
    And every request the server saw stated a call budget above 29000 ms and at most 30000 ms

  @state:happy
  Scenario: A list opens one client span per page under the span of the walk
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller has read the next page of the list
    And the caller has read the next page of the list
    When the caller reads the next page of the list
    Then the caller received the pages "1,2|3,4|5"
    And the walk is over
    And the tracer recorded 3 client spans named "contract.v1.NumbersService/ListNoSideEffects" under the span of the walk
    And every span the tracer recorded ended

  @state:happy
  Scenario: A list walk gives its items in order
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    When the caller walks the numbers list by items through the SDK
    Then the caller received the items "1,2,3,4,5"
    And the walk ended without refusal

  @state:happy
  Scenario: Leaving an item walk early releases the list
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    When the caller walks the numbers list by items and leaves after 3 items
    Then the caller received the items "1,2,3"
    And the list gives nothing after the caller left

  @state:error
  Scenario: A cursor the server refuses as expired ends the walk with a PageError
    Given the numbers server has the pages "1,2|3,4|5"
    And the numbers server refuses the cursor of page 3 as expired
    And the SDK caller is "ada" of organization "acme"
    When the caller walks the numbers list by pages through the SDK
    Then the caller received the pages "1,2|3,4"
    And the walk is refused with a PageError of kind "cursor_expired" after 2 pages and 4 items

  @state:error
  Scenario: A page the core cannot decode ends the walk
    Given the numbers server answers pages that cannot be decoded
    And the SDK caller is "ada" of organization "acme"
    When the caller walks the numbers list by pages through the SDK
    Then the caller received the pages ""
    And the walk is refused

  @state:happy
  Scenario: The first page of a list is read on its own
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    When the caller reads the first page of the numbers list through the SDK
    Then the first page holds the numbers "1,2"
    And the first page says a next page follows

  @state:happy
  Scenario: The first page of a list reads exactly one page
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    When the caller reads the first page of the numbers list through the SDK
    And the caller waits 200 ms
    Then the first page holds the numbers "1,2"
    And the list ends after its first page
    And the server saw 1 request(s)

  @state:happy
  Scenario: A list opened without fetching ahead reads only the pages asked for
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers without fetching ahead through the SDK
    When the caller reads the next page of the list
    And the caller waits 200 ms
    Then the caller received the pages "1,2"
    And the server saw 1 request(s)

  @state:happy
  Scenario: A list asked to fetch ahead overrides the call that chose not to
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers without fetching ahead and then asked it to fetch ahead through the SDK
    When the caller reads the next page of the list
    And the caller waits 200 ms
    Then the caller received the pages "1,2"
    And the server saw 2 request(s)

  @state:happy
  Scenario: The first page of a list asked to fetch ahead is still read alone
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    When the caller reads the first page of the numbers list through the SDK having asked to fetch ahead
    And the caller waits 200 ms
    Then the first page holds the numbers "1,2"
    And the list ends after its first page
    And the server saw 1 request(s)

  @state:happy
  Scenario: A client built with a default page size asks the server for pages of that size
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the numbers client defaults to the page size 2
    When the caller reads the first page of the numbers list through the SDK
    Then the first page holds the numbers "1,2"
    And every request the server saw asked for the page size 2

  @state:happy
  Scenario: A list with no item gives an empty first page
    Given the numbers server has the pages ""
    And the SDK caller is "ada" of organization "acme"
    When the caller reads the first page of the numbers list through the SDK
    Then the first page holds the numbers ""
    And the first page says no next page follows

  @state:happy
  Scenario: A list exposes the span of its walk
    Given the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    When the caller reads the next page of the list
    Then the numbers list exposes the span of its walk

  @state:happy
  Scenario: A list exposes no span when the tracer does not record
    Given the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the tracer does not record
    And the caller has opened a list of the numbers through the SDK
    When the caller reads the next page of the list
    Then the numbers list exposes no span

  @state:error
  Scenario: An identity change between pages refuses the next page and ends the walk
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the server switches the caller to "bob" while it handles the request for page 2
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    When the caller reads the next page of the list
    Then the caller received the pages "1,2"
    And the call is refused with the code "IDENTITY_CHANGED"
    And the span of the walk ended once with the status "error"
    And the server saw 2 request(s)

  @state:error
  Scenario: A page fetched ahead is refused when the caller switched after it was fetched
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the page after it has been fetched ahead
    And the caller switches to "bob" of organization "acme"
    When the caller reads the next page of the list
    Then the caller received the pages "1,2"
    And the call is refused with the code "IDENTITY_CHANGED"
    And the span of the walk ended once with the status "error"
    And the server saw 2 request(s)

  @state:happy
  Scenario: A batch owns one call budget for the whole call
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server has the widget "w2" named "Bolt"
    And the widgets server refuses the first 2 requests as unavailable
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widgets "w1,w2" through the SDK
    Then the batch delivered the widgets "w1=Anvil,w2=Bolt" in request order
    And the server saw 3 request(s)
    And the call budget the server saw shrank from each request to the next
    And every request the server saw stated a call budget above 29000 ms and at most 30000 ms

  @state:error
  Scenario: An identity change during a batch refuses the chunk
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server has the widget "w2" named "Bolt"
    And the SDK caller is "ada" of organization "acme"
    And the server switches the caller to "bob" while it handles a request
    When the caller gets the widgets "w1,w2" through the SDK
    Then the call is refused with the code "BATCH_IDENTITY_CHANGED"
    And the server saw 1 request(s)

  @state:happy
  Scenario: A stream delivers its ticks one by one
    Given the events server streams 3 ticks
    And the events server waits 25 ms before each tick
    And the SDK caller is "ada" of organization "acme"
    When the caller watches the events through the SDK
    Then the caller received the ticks "0,1,2"
    And the watch was not refused
    And the server saw 1 request(s)
    And the request the server saw stated no call budget
    And the wrapper lists "watch" as a "stream" method of the surface "sdk-call"

  @state:happy
  Scenario: A stream keeps delivering ticks after the call budget has passed
    Given the events server streams 4 ticks
    And the events server waits 8000 ms before each tick
    And the SDK caller is "ada" of organization "acme"
    When the caller watches the events through the SDK
    Then the caller received the ticks "0,1,2,3"
    And the watch lasted longer than the call budget
    And the watch was not refused
    And the server saw 1 request(s)

  @state:happy
  Scenario: Leaving a stream early releases it
    Given the events server streams 5 ticks
    And the SDK caller is "ada" of organization "acme"
    When the caller watches the events through the SDK and leaves after 2 ticks
    Then the caller received the ticks "0,1"
    And the watch gives nothing after the caller left

  @state:happy
  Scenario: A stream refused in its first frame is renewed and replayed once
    Given the events server streams 3 ticks
    And the events server refuses the first 1 requests as unauthenticated
    And the SDK caller is "ada" of organization "acme"
    When the caller watches the events through the SDK
    Then the caller received the ticks "0,1,2"
    And the watch was not refused
    And the server saw 2 request(s)
    And the credential source was asked to refresh 1 time(s)
    And the host was told 0 time(s) that a server refused the caller as unauthenticated

  @state:error
  Scenario: A stream that cannot open is refused when it opens
    Given the events server streams 3 ticks
    And the events server refuses the first 2 requests as unauthenticated
    And the SDK caller is "ada" of organization "acme"
    When the caller watches the events through the SDK
    Then the caller received the ticks ""
    And the call is refused as unauthenticated
    And the server saw 2 request(s)
    And the credential source was asked to refresh 1 time(s)
    And the host was told 1 time(s) that a server refused the caller as unauthenticated

  @state:error
  Scenario: A stream ends with an identity change when the caller switches between ticks
    Given the events server streams 3 ticks
    And the SDK caller is "alice" of organization "acme"
    And the caller has opened a watch of the events through the SDK
    And the caller has read the first tick of the watch
    And the caller switches to "alice" of organization "globex"
    When the caller reads the next tick of the watch
    Then the caller received the ticks "0"
    And the call is refused with the code "IDENTITY_CHANGED"
    And the call is refused as unauthenticated
    And the watch has ended

  @state:happy
  Scenario: A token refresh for the same identity keeps a stream flowing
    Given the events server streams 3 ticks
    And the SDK caller is "alice" of organization "acme"
    And the caller has opened a watch of the events through the SDK
    And the caller has read the first tick of the watch
    And the caller's token is refreshed
    And the caller has read the next tick of the watch
    And the caller has read the next tick of the watch
    When the caller reads the next tick of the watch
    Then the caller received the ticks "0,1,2"
    And the watch was not refused
    And the watch has ended

  @state:error
  Scenario: A server that refuses the refreshed credential as unauthenticated is told to the host and drops the revision cache
    Given the widgets server has the widget "w1" named "Anvil"
    And the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the widgets server refuses the first 2 requests as unauthenticated
    When the caller gets the widget "w1" through the SDK
    Then the call is refused as unauthenticated
    And the server saw 2 request(s)
    And the credential source was asked to refresh 1 time(s)
    And the host was told 1 time(s) that a server refused the caller as unauthenticated
    And the caller's revision cache is empty
    And the tracer ended every span it started with the status "error"

  @state:error
  Scenario: A changed identity drops the revision cache without telling the host a server refused
    Given the widgets server has the widget "w1" named "Anvil"
    And the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the caller's identity changes to "bob" before the first send
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "IDENTITY_CHANGED"
    And the server saw 0 request(s)
    And the caller's revision cache is empty
    And the host was told 0 time(s) that a server refused the caller as unauthenticated

  @state:error
  Scenario: A refusal of one caller leaves another caller's revision cache intact
    Given the widgets server has the widget "w1" named "Anvil"
    And the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the caller switches to "bob" of organization "acme"
    And the revision cache holds the numbers list
    And the widgets server refuses the first 2 requests as unauthenticated
    When the caller gets the widget "w1" through the SDK
    Then the call is refused as unauthenticated
    And the credential source was asked to refresh 1 time(s)
    And the host was told 1 time(s) that a server refused the caller as unauthenticated
    And the revision cache still holds the numbers list of the other caller

  @state:happy
  Scenario: A list the server confirms unchanged is replayed from the revision cache
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller has read the next page of the list
    And the caller has read the next page of the list
    When the caller reads the next page of the list
    Then the caller received the pages "1,2|3,4|5"
    And every page the caller received came from the revision cache
    And the server saw 1 request(s)

  @state:happy
  Scenario: A list is replayed from the revision cache after the caller dropped every page and the host collected its garbage
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the caller dropped every page and the host collected its garbage
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller has read the next page of the list
    And the caller has read the next page of the list
    When the caller reads the next page of the list
    Then the caller received the pages "1,2|3,4|5"
    And every page the caller received came from the revision cache
    And the server saw 1 request(s)

  @state:happy
  Scenario: A list whose items encode to more than 1 MiB is never replayed from the revision cache
    Given the numbers server has 2 pages of 200000 numbers each
    And the SDK caller is "ada" of organization "acme"
    And the caller has walked the numbers list through the SDK
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    When the caller reads the next page of the list
    Then no page the caller received came from the revision cache
    And the server saw 2 request(s)

  @state:happy
  Scenario: A list takes its page size from the call's options
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the numbers with the page size 2 through the SDK
    Then the caller received the pages "1,2|3,4|5"
    And every request the server saw asked for the page size 2

  @state:happy
  Scenario: A list the server confirms unchanged is not replayed to a client of another server address
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the caller has opened a list of the numbers at another address of the server through the SDK
    And the caller has read the first page of the list
    And the caller has read the next page of the list
    When the caller reads the next page of the list
    Then the caller received the pages "1,2|3,4|5"
    And no page the caller received came from the revision cache
    And the server saw 3 request(s)

  @state:error
  Scenario: A changed identity while a cached list is replayed refuses the next page and drops the revision cache
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller switches to "bob" of organization "acme"
    When the caller reads the next page of the list
    Then the call is refused with the code "IDENTITY_CHANGED"
    And the caller received the pages "1,2"
    And the caller's revision cache is empty
    And the host was told 0 time(s) that a server refused the caller as unauthenticated

  @state:error
  Scenario: A changed identity while a cached list is replayed drops only the revision cache of the caller who was replaying
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "bob" of organization "acme"
    And the revision cache holds the numbers list
    And the caller switches to "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller switches to "carol" of organization "acme"
    When the caller reads the next page of the list
    Then the call is refused with the code "IDENTITY_CHANGED"
    And the caller received the pages "1,2"
    And the revision cache no longer holds the numbers list of the caller who was replaying
    And the revision cache still holds the numbers list of the other caller
    And the host was told 0 time(s) that a server refused the caller as unauthenticated

  @state:error
  Scenario: A server that refuses a cached list as unauthenticated is told to the host and drops the revision cache
    Given the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the numbers server refuses the first 2 requests as unauthenticated
    And the caller has opened a list of the numbers through the SDK
    When the caller reads the next page of the list
    Then the call is refused as unauthenticated
    And the server saw 2 request(s)
    And the credential source was asked to refresh 1 time(s)
    And the host was told 1 time(s) that a server refused the caller as unauthenticated
    And the caller's revision cache is empty

  @state:error
  Scenario: A signed-out session drops its held pages
    Given the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the caller's session holds the numbers list
    And the caller signs out
    When the caller reads the numbers list through the same session
    And the caller signs in again
    Then the call is refused with the platform code "SERVICE_UNAVAILABLE"
    And the host was told 0 time(s) that a server refused the caller as unauthenticated
    And the caller's revision cache is empty

  @state:error
  Scenario: A credential source that cannot answer fails the call closed
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the caller's credential source cannot answer
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the platform code "SERVICE_UNAVAILABLE"
    And the server saw 0 request(s)

  @state:error
  Scenario: A credential source that rejects fails the call closed
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the caller's credential source rejects
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the platform code "SERVICE_UNAVAILABLE"
    And the server saw 0 request(s)

  @state:happy
  Scenario: A credential refused as unauthenticated is refreshed once and the call replayed
    Given the widgets server has the widget "w1" named "Anvil"
    And the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the revision cache holds the numbers list
    And the widgets server refuses the first 1 requests as unauthenticated
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the server saw 2 request(s)
    And the requests the server saw carried the credentials "Bearer caller,Bearer refreshed" in order
    And the credential source was asked to refresh 1 time(s)
    And the host was told 0 time(s) that a server refused the caller as unauthenticated
    And the revision cache still holds the numbers list

  @state:happy
  Scenario: Concurrent calls refused on one credential renew it once
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the caller's credential source answers after 50 ms
    And the widgets server refuses the first 4 requests as unauthenticated
    When the caller gets the widget "w1" through the SDK in 4 concurrent calls
    Then every concurrent call answered the widget "w1" named "Anvil"
    And the server saw 8 request(s)
    And the credential source was asked to refresh 1 time(s)
    And the host was told 0 time(s) that a server refused the caller as unauthenticated

  @state:error
  Scenario: A refresh that answers the same credential is not replayed
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the widgets server refuses the first 1 requests as unauthenticated
    And the caller's credential source answers the same credential when asked to refresh
    When the caller gets the widget "w1" through the SDK
    Then the call is refused as unauthenticated
    And the server saw 1 request(s)
    And the credential source was asked to refresh 1 time(s)
    And the host was told 1 time(s) that a server refused the caller as unauthenticated

  @state:error
  Scenario: A refresh that answers another identity refuses with IDENTITY_CHANGED
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the widgets server refuses the first 1 requests as unauthenticated
    And the caller's credential source answers "bob" of organization "acme" when asked to refresh
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "IDENTITY_CHANGED"
    And the server saw 1 request(s)
    And the credential source was asked to refresh 1 time(s)
    And the host was told 0 time(s) that a server refused the caller as unauthenticated

  @state:happy
  Scenario: A credential answered later is awaited before the first send
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    And the caller's credential source answers after 300 ms
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the server saw 1 request(s)
    And every request the server saw carried the caller's credential

  @state:error
  Scenario: A list cancelled while its credential is awaited sends nothing
    Given the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    And the caller's credential source answers after 5000 ms
    And the caller has opened a list of the numbers through the SDK
    And a read of the first page of the list is waiting for the caller's credential
    When the caller cancels the list
    Then the pending read is refused with the code "CANCELED"
    And the server saw 0 request(s)

  @state:error
  Scenario: A watch refused after it delivered a tick is not replayed
    Given the events server streams 3 ticks
    And the events server refuses the watch as unauthenticated after its first tick
    And the SDK caller is "ada" of organization "acme"
    When the caller watches the events through the SDK
    Then the caller received the ticks "0"
    And the call is refused as unauthenticated
    And the server saw 1 request(s)
    And the credential source was asked to refresh 0 time(s)
    And the host was told 1 time(s) that a server refused the caller as unauthenticated

  @state:happy
  Scenario: A call opens one client span when it starts and ends it with its outcome
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the tracer started 1 client span named "contract.v1.WidgetsService/GetWidget"
    And the tracer ended every span it started with the status "ok"
    And every request the server saw carried the traceparent of a span the tracer ended

  @state:happy
  Scenario: Every send carries the caller's credential
    Given the widgets server has the widget "w1" named "Anvil"
    And the SDK caller is "ada" of organization "acme"
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And every request the server saw carried the caller's credential

  @state:happy
  Scenario: A retried call carries the credential read just before each send
    Given the widgets server has the widget "w1" named "Anvil"
    And the widgets server refuses the first 1 requests as unavailable
    And the SDK caller is "ada" of organization "acme"
    And the server refreshes the caller's token to "Bearer refreshed" while it handles a request
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the server saw 2 request(s)
    And the requests the server saw carried the credentials "Bearer caller,Bearer refreshed" in order

  @state:happy
  Scenario: An anonymous caller sends no credential and still carries a trace context
    Given the widgets server has the widget "w1" named "Anvil"
    And the caller is anonymous
    When the caller gets the widget "w1" through the SDK
    Then the response is the widget "w1" named "Anvil"
    And the server saw 1 request(s)
    And no request the server saw carried a credential
    And every request the server saw carried a valid traceparent

  @state:error
  Scenario: A read started while another is pending is refused and leaves the pending read alone
    Given the numbers server has the pages "1,2|3"
    And the numbers server takes 500 ms to answer each page
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    And a read of the next page of the list is pending
    When the caller reads the next page of the list
    Then the call is refused with the code "HANDLE_BUSY"
    And the pending read completes with the page "1,2"

  @state:error
  Scenario: A list cancelled while a page is pending abandons that read
    Given the numbers server has the pages "1,2|3,4|5"
    And the numbers server takes 500 ms to answer each page
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    And a read of the next page of the list is pending
    When the caller cancels the list
    Then the pending read is refused with the code "CANCELED"
    And the span of the walk ended once with the status "unset"

  @state:error
  Scenario: A list cancelled between pages gives no page after that
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller has cancelled the list
    When the caller reads the next page of the list
    Then the caller received the pages "1,2"
    And the walk is over
    And the span of the walk ended once with the status "unset"

  @state:happy
  Scenario: A list cancelled twice ends its walk once
    Given the numbers server has the pages "1,2|3,4|5"
    And the SDK caller is "ada" of organization "acme"
    And the caller has opened a list of the numbers through the SDK
    And the caller has read the first page of the list
    And the caller has cancelled the list
    When the caller cancels the list
    Then the span of the walk ended once with the status "unset"

  @state:happy
  Scenario: An SDK opened from the caller's session serves every client with its credential
    Given the widgets server has the widget "w1" named "Anvil"
    And the numbers server has the pages "1,2|3"
    And the SDK caller is "ada" of organization "acme"
    When the caller opens the Api Bones SDK from the caller's session at the address of the server
    And the caller gets the widget "w1" through the opened SDK
    And the caller lists the numbers through the opened SDK
    Then the response is the widget "w1" named "Anvil"
    And the caller received the pages "1,2|3"
    And every request the server saw carried the caller's credential

  @state:error
  Scenario: An SDK opened without a base address outside a page is refused
    Given the SDK caller is "ada" of organization "acme"
    When the caller opens the Api Bones SDK from the caller's session without a base address
    Then the call is refused with the platform code "VALIDATION_FAILED"
    And the refusal says "no page origin; pass baseUrl"

  @state:happy
  Scenario: A static function of the SDK answers without a client or a server
    When the caller clamps 5 between 1 and 10 through the SDK
    Then the static answer is 5
    And the wrapper lists "clamp" as a "sync" method of the surface "sdk-call"

  @state:happy
  Scenario: A static function holds a value above its bounds at the upper one
    When the caller clamps 50 between 1 and 10 through the SDK
    Then the static answer is 10

  @state:happy
  Scenario: A static function holds a value below its bounds at the lower one
    When the caller clamps 0 between 1 and 10 through the SDK
    Then the static answer is 1

  @state:happy
  Scenario: A constant of the SDK is read through a function that takes nothing
    When the caller reads the largest page size through the SDK
    Then the static answer is 1000
    And the wrapper lists "MAX_PAGE_SIZE" as a "sync" method of the surface "sdk-call"
