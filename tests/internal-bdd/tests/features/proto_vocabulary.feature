# SPDX-License-Identifier: MIT
@library @proto
Feature: Proto vocabulary shipped to contract authors

  Contract authors compile against the proto files this crate ships. The
  vocabulary they declare there -- resources, call shapes, MCP opt-in, and
  the automation surface of triggers, actions, classifications and templates
  -- reads back exactly as declared, the retired MCP shape field stays
  refused, no generic dispatch kind exists, and the manifest a release signs
  carries the declared surface unchanged.

  Scenario: The shipped files include the field behavior vocabulary
    Then the shipped files include "google/api/field_behavior.proto"
    And the shipped files include "bones/v1/annotations.proto"
    And the shipped files include "bones/v1/automation_manifest.proto"

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
      | bones.v1.resource             | 5102361 |
      | bones.v1.call_shape           | 5102362 |
      | bones.v1.automation_trigger   | 5102363 |
      | bones.v1.automation_action    | 5102364 |
      | bones.v1.classification       | 5102365 |
      | bones.v1.automation_templates | 5102366 |

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

  Scenario: A domain event declared as a trigger reads back its label
    Given a contract declaring its event a trigger labelled "Departure confirmed"
    When the contract is compiled against the shipped files
    Then the trigger label is "Departure confirmed"

  Scenario Outline: A public RPC declared as an action reads back its effect
    Given a contract declaring its RPC an action labelled "Confirm a departure" with effect "<effect>"
    When the contract is compiled against the shipped files
    Then the action label is "Confirm a departure"
    And the action effect is "<effect>"

    Examples:
      | effect                           |
      | ACTION_EFFECT_PURE               |
      | ACTION_EFFECT_EPHEMERAL          |
      | ACTION_EFFECT_COMPENSABLE        |
      | ACTION_EFFECT_POINT_OF_NO_RETURN |

  Scenario: An action's grant is its RPC's authz requirement and nothing else
    Given a contract declaring its RPC an action labelled "Confirm a departure" with effect "ACTION_EFFECT_COMPENSABLE"
    When the contract is compiled against the shipped files
    Then the action's RPC requires exactly "departure:confirm"
    And the action declaration has no field naming a scope, grant or requirement

  Scenario Outline: A guardrail is a declaration flag on the action
    Given a contract declaring its RPC an action with the guardrail "<flag>"
    When the contract is compiled against the shipped files
    Then the action guardrail "<flag>" is set
    And the action guardrail "<other>" is not set

    Examples:
      | flag            | other           |
      | person_reserved | planner_pinned  |
      | planner_pinned  | person_reserved |

  Scenario: A read action declares how a form picks one of its entities
    Given a contract declaring a read action resolving entities by "query" into "departures"
    When the contract is compiled against the shipped files
    Then the action resolves search text in "query" to candidates in "departures"

  Scenario Outline: A classified field reads back its classification
    Given a contract whose field is classified "<classification>"
    When the contract is compiled against the shipped files
    Then the field classification reads back as "<classification>"

    Examples:
      | classification                   |
      | DATA_CLASSIFICATION_PUBLIC       |
      | DATA_CLASSIFICATION_INTERNAL     |
      | DATA_CLASSIFICATION_CONFIDENTIAL |
      | DATA_CLASSIFICATION_RESTRICTED   |

  Scenario: A field left unclassified stays distinguishable from every classification
    Given a contract declaring its event a trigger labelled "Departure confirmed"
    When the contract is compiled against the shipped files
    Then the field carries no classification
    And the field classification reads back as "DATA_CLASSIFICATION_UNSPECIFIED"
    And no classified value of "bones.v1.DataClassification" has number 0

  Scenario: A template the product ships reads back its trigger and steps
    Given a contract shipping a template "notify_on_confirmed" that calls "contract.v1.Svc.Do" when "contract.v1.Thing" fires
    When the contract is compiled against the shipped files
    Then the template "notify_on_confirmed" is triggered by "contract.v1.Thing"
    And the first step of template "notify_on_confirmed" calls "contract.v1.Svc.Do"
    And the first step of template "notify_on_confirmed" binds "id" from the trigger field "id"

  Scenario Outline: A template step of a generic dispatch kind does not compile
    Given a contract shipping a template whose step is of kind "<kind>"
    When the contract is compiled against the shipped files
    Then compilation fails

    Examples:
      | kind      |
      | raw_http  |
      | raw_proto |
      | shell     |
      | sql       |

  Scenario: The signed manifest carries the product and its whole surface unchanged
    Given a contract shipping a template "notify_on_confirmed" that calls "contract.v1.Svc.Do" when "contract.v1.Thing" fires
    When the contract is compiled against the shipped files
    And a manifest for product "itinerwiz" is compiled from the contract and served with its signature
    Then the served manifest bytes are the bytes that were signed
    And the served manifest names product "itinerwiz"
    And the served manifest carries the trigger "contract.v1.Thing", the action "contract.v1.Svc.Do" and the template "notify_on_confirmed"
    And the action "contract.v1.Svc.Do" in the served manifest has the grant "departure:confirm"
    And the served manifest schema classifies "contract.v1.Thing" field "id" as "DATA_CLASSIFICATION_INTERNAL"
