# SPDX-License-Identifier: MIT
@library @protos
Feature: Field labels

  A product labels the fields of its core records in its proto. Beside the
  `bones.v1.classification` option, `bones.v1.meaning` names what a field
  denotes, as one term of the curated schema.org-based vocabulary, so the
  business kernel's registry reads the labels from the descriptor instead of
  restating them. A field's value type is its proto type, so no option
  restates it. The descriptor reads the same in Rust and in TypeScript; a
  row only one language satisfies is a divergence.

  Scenario Outline: A field carrying a meaning and a classification reads both back
    Given a record field "<field>" of type "<type>" labelled with meaning "<meaning>" and classification "<classification>"
    When the record is read back through descriptor reflection
    Then the field meaning reads back as "<meaning>"
    And the field classification reads back as "<classification>"
    And the field value type reads back as "<type>"

    Examples:
      | field            | type   | meaning                        | classification               |
      | start_date       | string | schema:startDate               | DATA_CLASSIFICATION_INTERNAL |
      | max_participants | int32  | schema:maximumAttendeeCapacity | DATA_CLASSIFICATION_PUBLIC   |
      | price_cents      | int64  | schema:price                   | DATA_CLASSIFICATION_CONFIDENTIAL |
      | is_open          | bool   | schema:isAccessibleForFree     | DATA_CLASSIFICATION_RESTRICTED |

  Scenario: A field with no meaning mapping carries no meaning
    Given a record field "notes" of type "string" labelled with classification "DATA_CLASSIFICATION_INTERNAL" only
    When the record is read back through descriptor reflection
    Then the field carries no meaning
    And the field classification reads back as "DATA_CLASSIFICATION_INTERNAL"

  Scenario: A field with a meaning and no classification carries no classification
    Given a record field "title" of type "string" labelled with meaning "schema:name" only
    When the record is read back through descriptor reflection
    Then the field meaning reads back as "schema:name"
    And the field carries no classification

  Scenario: The shipped field vocabulary labels meaning and classification and restates no value type
    Given the shipped field vocabulary
    When the vocabulary is read back through descriptor reflection
    Then the field options it declares are "attr, classification, meaning"
    And the classification enums it declares are "bones.v1.DataClassification"
