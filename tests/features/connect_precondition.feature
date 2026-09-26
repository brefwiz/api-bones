# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect default precondition

  A Connect handler enforcing `check_if_match` rejects a mutating call that
  carries no `if-match` header. Api-bones ships the default that closes that
  gap twice, once in Rust and once in TypeScript. Both implementations read
  the same generated policy artifact and answer the scenarios below the same
  way, so a row only one language satisfies is a divergence.

  Scenario Outline: A method's declared idempotency decides its default precondition
    Given a unary method declared "<idempotency>"
    And a call to that method carrying "<caller_header>" as its own if-match header
    When the call is sent
    Then the call carries "<sent_header>" as its if-match header

    # A mutating method gets the wildcard precondition by default.
    Examples: idempotency decides the default
      | idempotency     | caller_header | sent_header |
      | IDEMPOTENT       | none          | *           |
      | UNSPECIFIED      | none          | *           |
      | NO_SIDE_EFFECTS  | none          | none        |

    # A caller that already tracked a real ETag is never second-guessed.
    Examples: a caller-supplied precondition is never overridden
      | idempotency  | caller_header | sent_header  |
      | IDEMPOTENT   | real-etag     | real-etag    |

  Scenario: An unannotated method is left exactly as it was before this existed
    Given a method with no policy entry at all
    And a call to that method carrying "none" as its own if-match header
    When the call is sent
    Then the call carries "none" as its if-match header
