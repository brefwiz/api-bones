# SPDX-License-Identifier: MIT
@library @protos
Feature: Overridable default origins

  An instance message (a slot of its schedule rule, a booking line of its
  price-list item) declares what it is instantiated from with
  `bones.v1.instance_of`, and each field whose default is proposed by that
  origin names the origin field with `bones.v1.defaulted_from`. The instance's
  value stays overridable: the options record where a default came from and
  never constrain it. The descriptor reads the same in Rust and in
  TypeScript; a row only one language satisfies is a divergence.

  Scenario Outline: An instance field defaulted from its origin reads both options back
    Given an instance "<instance>" of "<origin>" whose field "<field>" is defaulted from "<origin_field>"
    When the instance is read back through descriptor reflection
    Then the instance reads back as an instance of "<origin>"
    And the field default reads back as originating from "<origin_field>"

    Examples:
      | instance | origin                   | field      | origin_field                        |
      | Slot     | contract.v1.ScheduleRule | start_time | contract.v1.ScheduleRule.start_time |
      | Line     | contract.v1.PriceItem    | unit_price | contract.v1.PriceItem.unit_price    |

  Scenario: A message that is not an instance carries no origin
    Given a plain message "Slot" with a field "start_time"
    When the instance is read back through descriptor reflection
    Then the message carries no instance origin
    And the field carries no default origin

  Scenario: An instance whose field has no default origin carries only the instance origin
    Given an instance "Slot" of "contract.v1.ScheduleRule" whose field "start_time" has no default origin
    When the instance is read back through descriptor reflection
    Then the instance reads back as an instance of "contract.v1.ScheduleRule"
    And the field carries no default origin
