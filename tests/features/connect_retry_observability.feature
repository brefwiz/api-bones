# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: Connect retry observability

  A caller can attach `onRetry` to either transport to see each retried
  attempt -- its method, 1-based attempt number, Connect code and the delay
  before the next try -- as the interceptor makes it, rather than only the
  outcome of the whole retrying call. Interceptor-pipeline concept, so this
  surface is TypeScript-only: Rust has no equivalent to attach to.

  Scenario: Each retried attempt is reported before its sleep
    Given a retryable method that fails twice with code "unavailable" before succeeding
    When the call is sent with a retry observer attached
    Then the call succeeds
    And the observer recorded 2 retried attempts
    And recorded attempt 1 carries code "unavailable"
    And recorded attempt 2 carries code "unavailable"

  Scenario: A call that succeeds on its first attempt reports nothing
    Given a retryable method that succeeds immediately
    When the call is sent with a retry observer attached
    Then the call succeeds
    And the observer recorded 0 retried attempts

  Scenario: A method the policy does not mark retryable reports nothing
    Given a non-retryable method that fails with code "unavailable"
    When the call is sent with a retry observer attached
    Then the call fails
    And the observer recorded 0 retried attempts

  Scenario: An observer that throws never breaks the retried call
    Given a retryable method that fails once with code "unavailable" before succeeding
    When the call is sent with an observer that throws
    Then the call succeeds
