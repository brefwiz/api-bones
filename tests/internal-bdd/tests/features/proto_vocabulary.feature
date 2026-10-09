# SPDX-License-Identifier: MIT
@library @proto
Feature: Proto vocabulary shipped to contract authors

  Contract authors compile against the proto files this crate ships. The
  vocabulary they declare there -- resources, call shapes, MCP opt-in -- reads
  back exactly as declared, and the retired MCP shape field stays refused.

  Scenario: The shipped files include the field behavior vocabulary
    Then the shipped files include "google/api/field_behavior.proto"
    And the shipped files include "bones/v1/annotations.proto"

  Scenario: A resource declaration reads back its type, identity and method overrides
    Given a contract declaring a resource "reservation" identified by "id" with update "ReplaceReservation"
    When the contract is compiled against the shipped files
    Then the resource type is "reservation"
    And the resource identity field is "id"
    And the resource update method is "ReplaceReservation"

  Scenario: A resource without overrides leaves every method name empty
    Given a contract declaring a resource "bare" identified by "name" with no overrides
    When the contract is compiled against the shipped files
    Then the resource type is "bare"
    And the resource identity field is "name"
    And the resource declares no method names

  Scenario Outline: Every call shape reads back
    Given a contract declaring a method with call shape "<shape>"
    When the contract is compiled against the shipped files
    Then the method call shape is "<shape>"

    Examples:
      | shape                       |
      | CALL_SHAPE_UNARY            |
      | CALL_SHAPE_CLIENT_STREAM    |
      | CALL_SHAPE_FINITE_STREAM    |
      | CALL_SHAPE_UNBOUNDED_STREAM |
      | CALL_SHAPE_LONG_RUNNING     |

  Scenario Outline: The extension numbers are the reserved ones
    Given a contract using the shipped vocabulary
    When the contract is compiled against the shipped files
    Then the extension "<extension>" has number <number>

    Examples:
      | extension           | number  |
      | bones.v1.resource   | 5102361 |
      | bones.v1.call_shape | 5102362 |

  Scenario Outline: The call shape enum values are fixed
    Given a contract using the shipped vocabulary
    When the contract is compiled against the shipped files
    Then the enum "bones.v1.CallShape" declares "<name>" as <number>
    And the enum "bones.v1.CallShape" declares exactly 6 values

    Examples:
      | name                        | number |
      | CALL_SHAPE_UNSPECIFIED      | 0      |
      | CALL_SHAPE_UNARY            | 1      |
      | CALL_SHAPE_CLIENT_STREAM    | 2      |
      | CALL_SHAPE_FINITE_STREAM    | 3      |
      | CALL_SHAPE_UNBOUNDED_STREAM | 4      |
      | CALL_SHAPE_LONG_RUNNING     | 5      |

  Scenario Outline: Field behavior declared on a contract field reads back
    Given a contract whose field is declared "<behavior>"
    When the contract is compiled against the shipped files
    Then the field behavior reads back as "<behavior>"

    Examples:
      | behavior    |
      | REQUIRED    |
      | OUTPUT_ONLY |
      | IMMUTABLE   |
      | INPUT_ONLY  |

  Scenario: The MCP projection carries only the opt-in and a title
    Given a contract declaring a method with MCP title "Reserve a table"
    When the contract is compiled against the shipped files
    Then the method MCP title is "Reserve a table"
    And the MCP projection has only the field "title"

  Scenario: A contract setting the retired MCP shape does not compile
    Given a contract declaring a method with the retired MCP shape set
    When the contract is compiled against the shipped files
    Then compilation fails
