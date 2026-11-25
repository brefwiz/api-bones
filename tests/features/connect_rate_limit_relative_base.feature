# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: Connect waits out a rate-limit refusal through a relative base URL

  # A relative base URL is a browser notion: only the TypeScript transport
  # resolves one against the page, so this contract has no Rust counterpart.
  Scenario: A read refused once through a relative base URL is served on the next attempt
    Given a throttled method declared "NO_SIDE_EFFECTS"
    And the transport base URL is "/api"
    And the server refuses it 1 times with Retry-After "0"
    When the throttled call is sent
    Then the throttled call succeeds on attempt 2
