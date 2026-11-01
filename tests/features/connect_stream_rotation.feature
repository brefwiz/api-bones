# SPDX-License-Identifier: MIT
@library @connect
Feature: Stream rotation

  A long-lived server stream is replaced before the server cuts it, so the
  consumer sees one logical stream. When the server announces the cut with a
  drain notice, the client opens a replacement and switches to it only on
  proof that nothing falls between the two streams: in every domain either
  stream has checkpointed, and there must be at least one, the old stream's
  latest delivered-through position is at or above the live-from position of
  the replacement's first checkpoint, and both checkpoints are in the same
  epoch. Without that proof the client keeps the old stream until the server
  cuts it, and the consumer's stream ends as it does today, so the consumer's
  own loop reopens and reports the gap.

  The client holds back what the replacement delivers until the switch. At the
  switch it first delivers whatever the old stream had already received, then
  closes the old stream, then releases the held events, dropping any event
  whose key it has already delivered. It gives up on the rotation a fixed 2000
  ms before the cut. Every time below is on a virtual clock in milliseconds.

  The scenarios on a consumer drive the client's own driver, not the decision
  logic alone. A consumer opts in by declaring that its response type carries
  the stream control field, and only then does the client send the
  "bones-stream-control" header on the requests it makes. The client opens
  every stream, the first and each replacement, through the consumer's own open
  function, which the scenario scripts: it counts its calls, records each
  request's headers and returns scripted streams. No control frame reaches the
  consumer; it receives only the events.

  The steps use these phrasings for what a stream does at a time.
  - the old stream or the replacement "delivers event" an id
  - it "checkpoints domain" a name "in epoch" a number "live from" a position
    "delivered through" a position
  - it "drains cutting in" a number of milliseconds
  - it "sends a control that cannot be read"
  - it "ends" or "fails"

  Scenario: The client switches once the old stream's checkpoint covers the replacement
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    When the old stream drains cutting in 30000 ms at 1000 ms
    Then the client opens a replacement
    When the replacement delivers event "e-100" at 1100 ms
    And the replacement delivers event "e-101" at 1110 ms
    And the replacement delivers event "e-102" at 1120 ms
    And the replacement checkpoints domain "orders" in epoch 7 live from 100 delivered through 102 at 1130 ms
    Then the client does not switch
    And the consumer has received no events
    When the old stream delivers event "e-100" at 1200 ms
    And the old stream has already received event "e-101"
    And the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 100 at 1300 ms
    Then the client switches to the replacement
    And the consumer receives events "e-100, e-101, e-102" in order
    And the old stream is closed after the consumer received "e-101"
    And the old stream is closed before the consumer received "e-102"

  Scenario Outline: Without the proof the client keeps the old stream until the cut
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    When the old stream drains cutting in 30000 ms at 1000 ms
    And <replacement frame> at 1100 ms
    And the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 100 at 1200 ms
    Then the client does not switch
    And the client has not given up on the replacement
    When the clock reaches 29000 ms
    Then the client keeps the old stream
    And the replacement is closed
    And the client does not switch
    When the old stream ends at 30000 ms
    Then the consumer's stream ends

    Examples:
      | replacement frame |
      | the replacement checkpoints domain "orders" in epoch 7 live from 120 delivered through 130 |
      | the replacement checkpoints domain "billing" in epoch 7 live from 5 delivered through 5 |
      | the replacement delivers event "e-100" |

  Scenario Outline: An epoch change abandons the replacement at once
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    When the old stream drains cutting in 30000 ms at 1000 ms
    And <first frame> at 1100 ms
    And <second frame> at 1200 ms
    Then the client keeps the old stream
    And the replacement is closed
    And the client does not switch
    When the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 900 at 1300 ms
    Then the client does not switch

    Examples:
      | first frame | second frame |
      | the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 100 | the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 98 |
      | the replacement checkpoints domain "orders" in epoch 7 live from 100 delivered through 100 | the replacement checkpoints domain "orders" in epoch 7 live from 99 delivered through 100 |
      | the replacement checkpoints domain "orders" in epoch 8 live from 100 delivered through 100 | the replacement delivers event "e-100" |
      | the replacement checkpoints domain "orders" in epoch 7 live from 100 delivered through 100 | the replacement checkpoints domain "orders" in epoch 8 live from 101 delivered through 101 |

  Scenario: A proof completed after the deadline less the margin never switches
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    When the old stream drains cutting in 10000 ms at 1000 ms
    And the replacement checkpoints domain "orders" in epoch 7 live from 100 delivered through 100 at 1100 ms
    And the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 100 at 9000 ms
    Then the client does not switch
    And the client keeps the old stream
    And the replacement is closed

  Scenario Outline: A drain too close to its cut keeps the old stream without a replacement
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    When the old stream drains cutting in <cut> ms at 1000 ms
    Then the client opens no replacement
    And the client keeps the old stream
    And the client does not switch

    Examples:
      | cut |
      | 2000 |
      | 500 |

  Scenario Outline: A stream carrying two domains switches only when both are covered
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    And the old stream checkpoints domain "billing" in epoch 3 live from 40 delivered through 49 at 510 ms
    When the old stream drains cutting in 30000 ms at 1000 ms
    And the replacement checkpoints domain "orders" in epoch 7 live from 100 delivered through 100 at 1100 ms
    And the replacement checkpoints domain "billing" in epoch 3 live from 50 delivered through 50 at 1110 ms
    And the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through <orders through> at 1200 ms
    And the old stream checkpoints domain "billing" in epoch 3 live from 40 delivered through <billing through> at 1210 ms
    And the clock reaches 29000 ms
    Then <outcome>

    Examples:
      | orders through | billing through | outcome |
      | 100 | 50 | the client switches to the replacement |
      | 100 | 49 | the client keeps the old stream |
      | 99 | 50 | the client keeps the old stream |

  Scenario: A stream carrying two domains falls back when the replacement leaves one unannounced
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    And the old stream checkpoints domain "billing" in epoch 3 live from 40 delivered through 49 at 510 ms
    When the old stream drains cutting in 30000 ms at 1000 ms
    And the replacement checkpoints domain "orders" in epoch 7 live from 100 delivered through 100 at 1100 ms
    And the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 100 at 1200 ms
    Then the client does not switch
    When the clock reaches 29000 ms
    Then the client keeps the old stream
    And the replacement is closed
    And the client does not switch

  Scenario Outline: A failing replacement leaves the old stream in place
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    When the old stream drains cutting in 30000 ms at 1000 ms
    And the replacement <failure> at 1100 ms
    Then the client keeps the old stream
    And the replacement is closed
    When the old stream delivers event "e-1" at 1200 ms
    Then the consumer receives events "e-1" in order
    And the client does not switch

    Examples:
      | failure |
      | fails |
      | ends |
      | drains cutting in 30000 ms |
      | sends a control that cannot be read |

  Scenario: The old stream ending before the proof ends the consumer's stream as today
    Given the old stream checkpoints domain "orders" in epoch 7 live from 90 delivered through 99 at 500 ms
    When the old stream drains cutting in 30000 ms at 1000 ms
    And the replacement delivers event "e-100" at 1100 ms
    And the old stream ends at 1200 ms
    Then the consumer's stream ends
    And the replacement is closed
    And the consumer has received no events
    And the client does not switch

  Scenario Outline: The client declares stream control only for a response type that carries the control field
    Given a consumer whose response type <declaration> the stream control field
    When the consumer opens its stream through the client
    Then the open request <header> the "bones-stream-control" header

    Examples:
      | declaration      | header         |
      | declares         | carries        |
      | does not declare | does not carry |

  Scenario: The client opens every stream through the consumer's open function
    Given a consumer whose response type declares the stream control field
    When the consumer opens its stream through the client
    And the first stream drains cutting in 30000 ms
    Then the consumer's open function has been called once for each stream

  Scenario: No control frame reaches the consumer
    Given a consumer whose response type declares the stream control field
    When the consumer opens its stream through the client
    And the first stream delivers event "e-1", a checkpoint and a drain notice
    Then the consumer receives only event "e-1"
