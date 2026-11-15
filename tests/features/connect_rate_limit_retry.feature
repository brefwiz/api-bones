# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: Connect waits out a rate-limit refusal

  A call the server refuses for rate limiting is refused before its handler
  runs and names when to come back. A call waits that
  long and is served, and so is a write, because the refusal came before
  anything ran. A refusal that names no deadline is not repeated. A call that
  is not served rejects as a rate limit carrying how long to wait, which is
  never mistaken for a signed-out session. Interceptor-pipeline concept, so
  this surface is TypeScript-only: Rust has no client interceptor pipeline.

  Scenario: A read refused once is served on the next attempt
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the server refuses it 1 times with Retry-After "0"
    When the throttled call is sent through the retry interceptor
    Then the throttled call succeeds on attempt 2

  Scenario: A refused write is replayed once the wait is over and is applied once
    Given a throttled method declared "UNSPECIFIED"
    And the server refuses it 1 times with Retry-After "0"
    When the throttled call is sent through the retry interceptor
    Then the throttled call succeeds on attempt 2

  Scenario: A write refused without Retry-After is not replayed
    Given a throttled method declared "UNSPECIFIED"
    And the server refuses it 1 times without Retry-After
    When the throttled call is sent through the retry interceptor
    Then the throttled call is rejected as a plain refusal on attempt 1

  Scenario: A read refused on every attempt is rejected as a rate limit
    Given a throttled method declared "IDEMPOTENT"
    And the server refuses it 9 times with Retry-After "0"
    When the throttled call is sent through the retry interceptor
    Then the throttled call is rejected as a rate limit after 0 ms on attempt 4

  Scenario: A pushback longer than the longest wait is rejected without waiting
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the server refuses it 1 times with Retry-After "6"
    When the throttled call is sent through the retry interceptor
    Then the throttled call is rejected as a rate limit after 6000 ms on attempt 1

  Scenario: A read refused once through a relative base URL is served on the next attempt
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the transport base URL is "/api"
    And the server refuses it 1 times with Retry-After "0"
    When the throttled call is sent through the retry interceptor
    Then the throttled call succeeds on attempt 2
