# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect If-Match precondition (RFC 9110 weak comparison)

  `check_if_match` enforces the `If-Match` precondition on a Connect request
  against a server-derived `ETag`. Comparison is weak per RFC 9110 §8.8.3.2,
  so a strong-quoted client tag with the same value as a weak current one
  still matches, and `etag_from_updated_at` derives the same hex-millis value
  the HTTP-side `service_kit::etag::etag_from_updated_at` does, so a record
  carries the same ETag whichever transport served it.

  Scenario: A missing If-Match header is rejected as a precondition failure
    Given a current ETag derived from updated_at 1700000000000
    And no If-Match header on the request
    When the If-Match precondition is checked
    Then the check fails with failed_precondition

  Scenario: An empty If-Match header is rejected as a precondition failure
    Given a current ETag derived from updated_at 1700000000000
    And an If-Match header of ""
    When the If-Match precondition is checked
    Then the check fails with failed_precondition

  Scenario: A wildcard If-Match header always matches
    Given a current ETag derived from updated_at 1700000000000
    And an If-Match header of "*"
    When the If-Match precondition is checked
    Then the check succeeds

  Scenario: An exact If-Match header matches the current ETag
    Given a current ETag derived from updated_at 1700000000000
    And an If-Match header equal to the current ETag
    When the If-Match precondition is checked
    Then the check succeeds

  Scenario: A strong-quoted If-Match header matches a weak current ETag with the same value
    Given a current ETag derived from updated_at 1700000000000
    And an If-Match header equal to the current ETag's value, strongly quoted
    When the If-Match precondition is checked
    Then the check succeeds

  Scenario: A non-matching If-Match header is rejected as a conflict
    Given a current ETag derived from updated_at 1700000000000
    And an If-Match header equal to the ETag derived from updated_at 1700000001000
    When the If-Match precondition is checked
    Then the check fails with aborted

  Scenario: A malformed If-Match header is rejected as invalid
    Given a current ETag derived from updated_at 1700000000000
    And an If-Match header of "not-a-valid-etag"
    When the If-Match precondition is checked
    Then the check fails with invalid_argument
