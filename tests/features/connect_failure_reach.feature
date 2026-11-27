# SPDX-License-Identifier: MIT
@library @connect @rust-sdk-only
Feature: Connect call-failure reach

  A client that replays a failed call can run it twice, so a failure is
  classified by how far the call got, and only a call proven never to have
  reached the server may be replayed. Anything ambiguous is unanswered and is
  not replayed. The TypeScript package has no fleet transport to classify
  from, so this surface is Rust alone.

  Scenario: A dial failure is unreached and replayable
    Given a peer that refuses connections
    When the workload classifies the failure of a fleet call to it
    Then the failure reach is "unreached"
    And the failure is replayable

  Scenario: A reset after the request was sent is unanswered and not replayable
    Given a peer that resets the connection once it has the request
    When the workload classifies the failure of a fleet call to it
    Then the failure reach is "unanswered"
    And the failure is not replayable

  Scenario: A server error is answered and not replayable
    Given a peer that answers every call with an error
    When the workload classifies the failure of a fleet call to it
    Then the failure reach is "answered"
    And the failure is not replayable
