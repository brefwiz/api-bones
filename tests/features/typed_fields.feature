# SPDX-License-Identifier: MIT
@library @connect @typed-fields
Feature: Typed fields preserve their definition and option identities

  Owners define fields once and validate every new value against its resolved
  definition. A refusal carries a JSON pointer and a stable validation rule.
  Labels may change while definition and option identities remain stable.
  Owners check stored values before accepting narrower constraints.

  Scenario Outline: Values match their declared field type and constraints
    Given a typed field for "<type>"
    When the owner validates value "<value>"
    Then the SDK field result has rule "<rule>"

    Examples:
      | type             | value                          | rule              |
      | boolean          | boolean:true                   |                   |
      | boolean          | text:true                      | type_mismatch     |
      | number           | number:12.25                   |                   |
      | number           | number:100.01                  | range             |
      | number           | number:NaN                     | non_finite        |
      | number           | number:inf                     | non_finite        |
      | number           | number:12.345                  | fraction_digits   |
      | integer          | integer:100                    |                   |
      | integer          | integer:-1                     | range             |
      | enum             | enum:ready                     |                   |
      | enum             | enum:unknown                   | unknown_option    |
      | enum             | enum:retired                   | option_archived   |
      | text             | text:hello                     |                   |
      | text             | text:hello!                    | max_length        |
      | text             | text:ééééé                     |                   |
      | email            | text:ada@example.org           |                   |
      | email            | text:not-an-email              | format            |
      | uri              | text:https://example.org/a     |                   |
      | uri              | text:relative/path             | format            |
      | phone            | text:+14155552671              |                   |
      | phone            | text:4155552671                | format            |
      | date             | date:2024-02-29                |                   |
      | date             | date:2023-02-29                | format            |
      | timestamp        | timestamp:0:0                  |                   |
      | timestamp        | timestamp:0:1000000000         | format            |
      | duration         | duration:5:100                 |                   |
      | duration         | duration:5:-100                | format            |
      | money            | money:150:USD                  |                   |
      | money            | money:150:EUR                  | currency_mismatch |
      | money            | money:10001:USD                | range             |
      | person_reference | person:person-1                |                   |
      | person_reference | person:                        | format            |
      | record_link      | record:record-1                |                   |
      | record_link      | record:                        | format            |
      | boolean          | unset                          | type_mismatch     |

  Scenario: Archived definitions reject new writes
    Given a typed field for "boolean"
    And the field setting "archived" is "true"
    When the owner validates value "boolean:true"
    Then the SDK field result has rule "definition_archived"

  Scenario Outline: Definition schemas reject malformed constraints
    Given a typed field for "<type>"
    And the field setting "<setting>" is "<value>"
    When the owner validates the definition
    Then the SDK field result has rule "<rule>"

    Examples:
      | type        | setting          | value            | rule          |
      | number      | unit             | kg/m2            |               |
      | number      | unit             | {seat}           |               |
      | number      | unit             | {}               |               |
      | number      | unit             | mg/dL            |               |
      | number      | unit             | km/h             |               |
      | number      | unit             | Cel              |               |
      | number      | unit             | cel              | format        |
      | number      | unit             |                  | format        |
      | number      | unit             | {seat            | format        |
      | number      | unit             | {{seat}          | format        |
      | number      | unit             | {seat count}     | format        |
      | number      | unit             | {seat😀}         | format        |
      | number      | unit             | made_up_unit     | format        |
      | number      | key              | Bad Key          | format        |
      | number      | label            |                  | format        |
      | number      | classification   | 0                | format        |
      | number      | min              | 101              | range         |
      | number      | min              | NaN              | non_finite    |
      | number      | constraints      | none             | type_mismatch |
      | enum        | option_id        | ready            | format        |
      | enum        | option_count     | 0                | range         |
      | enum        | option_count     | 201              | range         |
      | text        | max_length       | 0                | range         |
      | text        | max_length       | 4097             | range         |
      | text        | max_length       | absent           | range         |
      | money       | currency         | ZZZ              | format        |
      | record_link | target_type      |                  | format        |

  Scenario Outline: Definition changes preserve stable schema identities
    Given a typed field for "<type>"
    When the owner changes field setting "<setting>" to "<value>"
    Then the SDK field result has rule "<rule>"
    And the SDK field change is "<change>"

    Examples:
      | type   | setting       | value        | rule              | change     |
      | number | label         | Renamed      |                   | compatible |
      | number | label         | Example      |                   | unchanged  |
      | number | id            | field-2      | format            |            |
      | number | key           | other        | format            |            |
      | number | type          | integer      | type_mismatch     |            |
      | number | max           | 200          |                   | compatible |
      | number | max           | 50           |                   | narrowing  |
      | number | min           | 10           |                   | narrowing  |
      | number | fraction      | 1            |                   | narrowing  |
      | text   | max_length    | 4            |                   | narrowing  |
      | money  | currency      | EUR          | currency_mismatch |            |
      | money  | max           | 5000         |                   | narrowing  |
      | enum   | remove_option | ready        | unknown_option    |            |
      | enum   | archive_option| ready        |                   | compatible |
      | enum   | option_label  | Renamed      |                   | compatible |
      | enum   | add_option    | pending      |                   | compatible |

  Scenario Outline: Renaming an enum label preserves the accepted option id
    Given a typed field for "enum"
    And the field setting "option_label" is "Renamed"
    When the owner validates value "enum:<value>"
    Then the SDK field result has rule "<rule>"

    Examples:
      | value   | rule           |
      | ready   |                |
      | Renamed | unknown_option |
