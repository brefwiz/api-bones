# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: Connect replays requests that never left the client

  A call is replayed only when its method is declared safe to replay, because
  a failure can reach the client after the server already acted. One class of
  failure is exempt: when the connection itself proves no byte of the request
  was ever sent -- the connection never opened, or the TLS handshake had not
  completed -- no server saw the
  call, so it is replayed whatever the method declares. The replay stays within
  the same bounded number of retries as any other. Interceptor-pipeline
  concept, so this surface is TypeScript-only: Rust has no client interceptor
  pipeline, and a Rust caller owns its own replay decision.

  Scenario: A failure before the handshake is replayed for an undeclared method
    Given a method declared "UNSPECIFIED"
    And its connection fails 2 times before the TLS handshake completed
    When the call is sent through the retry interceptor
    Then the call succeeds on attempt 3

  Scenario Outline: A connection that never opened is replayed for an undeclared method
    Given a method declared "UNSPECIFIED"
    And its connection never opened 1 times with "<failure>"
    When the call is sent through the retry interceptor
    Then the call succeeds on attempt 2

    Examples:
      | failure                          |
      | connect ECONNREFUSED 10.0.0.1:443 |
      | getaddrinfo ENOTFOUND svc         |
      | getaddrinfo EAI_AGAIN svc         |
      | connect ETIMEDOUT 10.0.0.1:443    |
      | connect EHOSTUNREACH 10.0.0.1:443 |
      | connect ENETUNREACH 10.0.0.1:443  |

  Scenario: A reset after the request was sent is not replayed for an undeclared method
    Given a method declared "UNSPECIFIED"
    And its connection fails 1 times after the request was sent
    When the call is sent through the retry interceptor
    Then the call fails on attempt 1

  Scenario: A declared idempotent method keeps replaying any connection failure
    Given a method declared "IDEMPOTENT"
    And its connection fails 1 times after the request was sent
    When the call is sent through the retry interceptor
    Then the call succeeds on attempt 2

  Scenario: The retry budget still bounds replays of undelivered requests
    Given a method declared "UNSPECIFIED"
    And its connection fails 10 times before the TLS handshake completed
    When the call is sent through the retry interceptor
    Then the call fails on attempt 4
