# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect waits out a rate-limit refusal

  A call the server refuses for rate limiting is refused before its handler
  runs and names when to come back. A call waits that long and is served, and
  so is a write, because the refusal came before anything ran. A refusal that
  names no deadline is not repeated. A wait never outlasts the call's deadline
  or its abort. A call that is not served rejects as a rate limit carrying how
  long to wait, which is never mistaken for a signed-out session. A rate limit
  is a refusal with the resource-exhausted code that carries a retry-after.

  Scenario: A read refused once is served on the next attempt
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the server refuses it 1 times with Retry-After "0"
    When the throttled call is sent
    Then the throttled call succeeds on attempt 2

  Scenario: A refused write is replayed once the wait is over and is applied once
    Given a throttled method declared "UNSPECIFIED"
    And the server refuses it 1 times with Retry-After "0"
    When the throttled call is sent
    Then the throttled call succeeds on attempt 2

  Scenario: A refused call is replayed no sooner than its Retry-After
    Given a throttled method declared "UNSPECIFIED"
    And the server refuses it 1 times with Retry-After "1"
    When the throttled call is sent
    Then the throttled call succeeds on attempt 2
    And the replay arrived at least 1000 ms after the refusal

  Scenario: A write refused without Retry-After is not replayed
    Given a throttled method declared "UNSPECIFIED"
    And the server refuses it 1 times without Retry-After
    When the throttled call is sent
    Then the throttled call is rejected as a plain refusal on attempt 1

  Scenario: A read refused on every attempt is rejected as a rate limit
    Given a throttled method declared "IDEMPOTENT"
    And the server refuses it 9 times with Retry-After "0"
    When the throttled call is sent
    Then the throttled call is rejected as a rate limit after 0 ms on attempt 4

  Scenario: A pushback longer than the longest wait is rejected without waiting
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the server refuses it 1 times with Retry-After "6"
    When the throttled call is sent
    Then the throttled call is rejected as a rate limit after 6000 ms on attempt 1

  Scenario: A pushback named as an HTTP-date is handed back as the wait left until that date
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the server refuses it 1 times with a Retry-After date 8 seconds ahead
    When the throttled call is sent
    Then the throttled call is rejected as a rate limit naming a wait between 6000 and 8000 ms on attempt 1

  Scenario: A pushback named in the trailers is handed back as a Retry-After
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the server refuses it 1 times with Retry-After "6" in the trailers
    When the throttled call is sent
    Then the throttled call is rejected as a rate limit after 6000 ms on attempt 1

  Scenario: A pushback longer than the remaining deadline ends the call without waiting it out
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the throttled call has a deadline of 500 ms
    And the server refuses it 1 times with Retry-After "3"
    When the throttled call is sent
    Then the throttled call ends as deadline exceeded before its Retry-After on attempt 1

  Scenario: An abort during the wait ends the call as canceled
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the server refuses it 1 times with Retry-After "3"
    When the throttled call is sent and abandoned during the wait
    Then the throttled call ends as canceled before its Retry-After on attempt 1
