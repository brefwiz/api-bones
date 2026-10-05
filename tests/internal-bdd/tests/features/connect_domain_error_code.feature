# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect domain error code

  A domain error that names its error code is mapped to a Connect refusal
  that carries the code as a `bones.v1.ErrorInfo` detail, whatever kind of
  refusal it maps to. The emitter is not known at this layer, so the detail
  carries the code alone.

  Scenario Outline: A coded domain error carries its code
    Given a domain error of kind "<kind>" with error code "GRANT_MISSING"
    When the domain error is mapped to a Connect error
    Then the Connect error has code "<connect_code>"
    And the Connect error carries error code "GRANT_MISSING"
    And the Connect error carries no emitter

    Examples: every kind
      | kind      | connect_code   |
      | not_found | not_found      |
      | conflict  | already_exists |
      | internal  | internal       |

  Scenario: A domain error without an error code carries no detail
    Given a domain error of kind "not_found" with no error code
    When the domain error is mapped to a Connect error
    Then the Connect error carries no error code

  Scenario: An internal error never surfaces its detail
    Given a domain error of kind "internal" with error code "GRANT_MISSING"
    When the domain error is mapped to a Connect error
    Then the Connect error message is "internal error"
