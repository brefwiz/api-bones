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
    Then the field options it declares are "attr, classification, defaulted_from, format, meaning, nested_write_of, references, typed_by"
    And the classification enums it declares are "bones.v1.DataClassification"

  Scenario: The shipped value formats are exactly a UUID and an absolute URI
    Given the shipped field vocabulary
    When the vocabulary is read back through descriptor reflection
    Then the enum "bones.v1.ValueFormat" declares exactly the values "VALUE_FORMAT_UNSPECIFIED=0, VALUE_FORMAT_UUID=1, VALUE_FORMAT_URI=2"

  Scenario: The shipped field reference names exactly a field or a principal of the caller's organization
    Given the shipped field vocabulary
    When the vocabulary is read back through descriptor reflection
    Then the message "bones.v1.FieldReference" declares the oneof "target" holding exactly "field, principal"

  Scenario Outline: A field labelled with a value format reads the format back
    Given a record field "<field>" of type "string" labelled with value format "<format>"
    When the record is read back through descriptor reflection
    Then the field value format reads back as "<format>"

    Examples:
      | field   | format           |
      | user_id | VALUE_FORMAT_UUID |
      | website | VALUE_FORMAT_URI |

  Scenario: A field labelled with a reference to a field reads the reference back
    Given a record field "event_id" of type "string" labelled with a reference to the field "acme.v1.Event.id"
    When the record is read back through descriptor reflection
    Then the field reference reads back as the field "acme.v1.Event.id"

  Scenario: A field labelled with a principal of the caller's organization reads the reference back
    Given a record field "owner_id" of type "string" labelled as a principal of the caller's organization
    When the record is read back through descriptor reflection
    Then the field reference reads back as a principal of the caller's organization

  Scenario: An enum value labelled output-only reads back as output-only and its siblings stay settable
    Given a contract enum "Scope" whose value "SCOPE_SPECIFIC" is labelled output-only beside "SCOPE_UNSPECIFIED, SCOPE_SELF"
    When the record is read back through descriptor reflection
    Then the enum value "SCOPE_SPECIFIC" of "contract.v1.Scope" reads back as output-only
    And the enum value "SCOPE_SELF" of "contract.v1.Scope" is not output-only
    And the enum value "SCOPE_UNSPECIFIED" of "contract.v1.Scope" is not output-only

  Scenario: An error enum value declaring typed arguments reads them back in order and its siblings declare none
    Given a contract enum "SeatError" whose value "SEAT_ERROR_LIMIT" declares the error arguments "limit:int64, plan:string, ratio:double, ends_at:timestamp" beside "SEAT_ERROR_UNSPECIFIED, SEAT_ERROR_ARCHIVED"
    When the record is read back through descriptor reflection
    Then the enum value "SEAT_ERROR_LIMIT" of "contract.v1.SeatError" reads back the error arguments "limit:int64, plan:string, ratio:double, ends_at:timestamp"
    And the enum value "SEAT_ERROR_ARCHIVED" of "contract.v1.SeatError" reads back the error arguments ""

  Scenario: The shipped enum value options are exactly output-only and error arguments
    Given the shipped field vocabulary
    When the vocabulary is read back through descriptor reflection
    Then the enum value options it declares are "error_args, output_only"

  @state:error
  Scenario: A field reference naming both a field and a principal is refused at compile
    Given a record field "owner_id" of type "string" labelled with a reference to both a field and a principal
    When the contract is compiled
    Then the contract is refused at compile because "field" and "principal" are mutually exclusive in "references"

  Scenario Outline: Every field of a shipped page response is classified internal
    Given the shipped pagination
    When the vocabulary is read back through descriptor reflection
    Then every field of the message "<message>" is classified "DATA_CLASSIFICATION_INTERNAL"

    Examples:
      | message                   |
      | bones.v1.PageResponse     |
      | bones.v1.Total            |
      | bones.v1.OffsetPageResponse |

  Scenario: The shipped stream control is exactly a drain or a checkpoint
    Given the shipped stream control
    When the vocabulary is read back through descriptor reflection
    Then the message "bones.v1.StreamControl" declares the oneof "signal" holding exactly "drain, checkpoint"
    And the message "bones.v1.Drain" declares exactly the fields "cut_in_ms"
    And the message "bones.v1.Checkpoint" declares exactly the fields "domain, live_from, delivered_through, epoch"
