# SPDX-License-Identifier: MIT
@library @connect
Feature: Paged lists without batch siblings

  A service that declares unary List RPCs and no batch method binds to the
  platform client and pages through the generated iterator. A service that
  declares neither batch nor paging methods has nothing the platform client
  can drive and is refused before any request is sent.

  Scenario: A list-only service pages through every page without a batch method
    Given a list-only service whose pages are "1,2|3,4|5"
    When the caller reads every item of the list-only service through its platform client
    Then the caller receives the numbers "1,2,3,4,5"
    And the list-only service answered 3 list requests

  Scenario: A service with neither batch nor paging methods is refused
    Given a service with neither batch nor paging methods
    When the SDK constructs a platform client for that service
    Then platform client construction fails with "empty batch policy discovery"
