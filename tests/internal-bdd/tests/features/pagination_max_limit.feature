# SPDX-License-Identifier: MIT
@library @pagination
Feature: One platform maximum page size

  `api_bones::pagination::MAX_LIMIT` is the single named ceiling every list
  endpoint validates a page size against. `connect::page`'s offset-page
  builder re-exports the same constant instead of declaring its own, so a
  provider and its Connect adapter never disagree about how large a page a
  caller may request.

  Scenario: PaginationParams accepts a limit at the platform maximum
    When a caller requests offset pagination with limit 200
    Then the request is accepted with limit 200

  Scenario: PaginationParams rejects a limit above the platform maximum
    When a caller requests offset pagination with limit 201
    Then the request is rejected as out of range

  Scenario: The Connect page builder clamps an oversized limit to the platform maximum
    When a caller requests a Connect offset page with limit 9999
    Then the built page reports limit 200
