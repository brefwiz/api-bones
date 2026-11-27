# SPDX-License-Identifier: MIT
@library @connect
Feature: A refusal names when to come back

  A service that refuses a call because the caller must slow down states how
  long to wait by one rule: whole seconds, rounded up, never less than one.
  The wait travels three ways: the `Retry-After` header, a
  `google.rpc.RetryInfo` detail, and `Cache-Control: no-store`, so no cache
  holds the refusal. The refusal's message is free text. A client reads the
  wait from the detail first and from the header when there is no detail. A
  header that is not whole non-negative seconds names no wait. Rust and
  TypeScript answer the same file; a row only one language satisfies is a
  divergence.

  Scenario Outline: A refused call carries its wait through the real Connect stack
    Given a service that refuses a call and asks the caller to wait <wait_ms> ms
    When the refused call is sent
    And the client reads the wait
    Then the call is refused as "resource_exhausted"
    And the refusal's message is "slow down"
    And the refusal's Retry-After is "<seconds>"
    And the refusal's Cache-Control is "no-store"
    And the refusal's RetryInfo is <seconds> seconds
    And the wait the client reads is "<read_ms>"

    Examples: sub-second and exact waits
      | wait_ms | seconds | read_ms |
      | 0       | 1       | 1000    |
      | 1       | 1       | 1000    |
      | 999     | 1       | 1000    |
      | 1000    | 1       | 1000    |

    Examples: waits rounded up
      | wait_ms | seconds | read_ms |
      | 1001    | 2       | 2000    |
      | 1200    | 2       | 2000    |
      | 7000    | 7       | 7000    |

  Scenario Outline: A client reads the detail before the header
    Given a refusal with Retry-After "<header>" and RetryInfo "<info>"
    When the client reads the wait
    Then the wait the client reads is "<read_ms>"

    # `none` stands for an absent header or detail; a blank header is present
    # and empty.
    Examples: the detail wins, the header is the fallback
      | header | info | read_ms |
      | 5      | 3    | 3000    |
      | 5      | none | 5000    |
      | none   | 3    | 3000    |
      | 0      | none | 0       |
      | none   | none | none    |

    Examples: a header that is not whole non-negative seconds names no wait
      | header                        | info | read_ms |
      | Wed, 21 Oct 2015 07:28:00 GMT | none | none    |
      | -1                            | none | none    |
      | 1.5                           | none | none    |
      |                               | none | none    |
      | soon                          | none | none    |

    Examples: the detail still wins over a header that names no wait
      | header                        | info | read_ms |
      | Wed, 21 Oct 2015 07:28:00 GMT | 4    | 4000    |
      | -1                            | 4    | 4000    |

  Scenario: The detail is the bytes the platform limiter sends
    Given a refusal with the RetryInfo wire value "CgIIBw"
    When the client reads the wait
    Then the wait the client reads is "7000"
    And a RetryInfo of 7 seconds is written as "CgIIBw"

  Scenario: A detail written for a wait over a hundred seconds is read
    Given a refusal with the RetryInfo wire value "CgMIrAI"
    When the client reads the wait
    Then the wait the client reads is "300000"
    And a RetryInfo of 300 seconds is written as "CgMIrAI"

  Scenario: A detail that does not decode names no wait
    Given a refusal with the RetryInfo wire value "CgUI"
    When the client reads the wait
    Then the wait the client reads is "none"
