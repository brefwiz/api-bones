# SPDX-License-Identifier: MIT
@library @connect @rust-sdk-only
Feature: Fleet connection bound

  A workload that opens a channel to a peer inside the fleet gives up on
  establishing the connection after a fixed bound, well inside a one second
  call budget, instead of waiting out a transport default measured in
  seconds. The failure says it happened while connecting, so a caller can tell
  it from one that happened after the request was on the wire.

  Scenario: A peer that accepts the connection and never answers is given up on within the budget
    Given a fleet peer that accepts connections and never answers
    When the workload calls it over a fleet client
    Then the call fails within one second
    And the failure is reported as a failure to connect

  Scenario: A peer that refuses the connection fails fast
    Given a fleet peer that refuses connections
    When the workload calls it over a fleet client
    Then the call fails within one second
    And the failure is reported as a failure to connect

  Scenario: A failure that is not about connecting is not reported as one
    Given a fleet peer that answers every call with a failure
    When the workload calls it over a fleet client
    Then the failure is not reported as a failure to connect
