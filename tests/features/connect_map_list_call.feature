# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: A list call adapted to domain types

  A caller that reads a list wants domain values, not wire messages. Mapping a
  list call converts each item as the caller reads it and leaves everything
  else about the list alone: the page boundaries, the revision, the total and
  the cursor state. A failure of the underlying list reaches the caller
  unchanged, with the pages and items already delivered counted.

  Scenario: Mapped items arrive as domain values across every page
    Given a source of pages "1,2" "3" served through a list call
    When the caller maps each item to its doubled value and reads the items
    Then the mapped items are "2,4,6"

  Scenario: Mapped pages keep the page metadata
    Given a source of pages "1,2" "3" served through a list call
    When the caller maps each item to its doubled value and reads the pages
    Then the mapped pages hold "2,4" and "6"
    And every mapped page keeps the revision of its source page
    And every mapped page keeps the total of its source page
    And no mapped page is reported as read from the cache
    And the mapped call reports the span of its source call
    And only the first mapped page reports a next page

  Scenario: The first mapped page is read without reading the rest
    Given a source of pages "1,2" "3" served through a list call
    When the caller maps each item to its doubled value and reads the first page
    Then the first mapped page holds "2,4" and reports a next page
    And the server was asked for 1 page

  Scenario: An empty list maps to an empty page
    Given a source of pages "" served through a list call
    When the caller maps each item to its doubled value and reads the first page
    Then the first mapped page holds no items and reports no next page

  Scenario: A failing list reaches the caller with its delivered count
    Given a source of pages "1,2" "3" served through a list call whose second page fails
    When the caller maps each item to its doubled value and reads the items
    Then the read ends with a page error after 1 pages and 2 items
