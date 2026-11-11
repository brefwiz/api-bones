# SPDX-License-Identifier: MIT
@library @connect @batch @rust-sdk-only
Feature: Batched reads over the fleet transport

  A generated platform client the Rust SDK builds for a batched service runs on
  the fleet's one Connect client transport. A batch read through it answers
  every requested id on its own, in request order: an id the server does not
  hold fails only its own position. TypeScript has no fleet client, so this
  contract is Rust alone.

  Scenario: A batch read through the fleet transport answers each id on its own
    Given the organization "acme" has widgets "w1,w3"
    And the caller is "ada" of organization "acme"
    When the caller batch-gets "w3,ghost,w1"
    Then position 1 is the widget "w3"
    And position 2 fails with "not_found"
    And position 3 is the widget "w1"
    And the server received 1 "BatchGetWidgets" request
