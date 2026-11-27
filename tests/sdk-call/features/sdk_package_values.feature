# SPDX-License-Identifier: MIT
@library @sdk-call @ts-sdk-only
Feature: The generated SDK package exports what a caller needs to build the values of its signatures

  A method of the generated TypeScript SDK takes and returns protobuf messages.
  The package exports each message as a type and its schema as a value, so a
  caller creates and decodes one without reaching into the generated sources;
  it exports each enum the messages carry as a value, which is also its type. A
  Rust caller names the same types by the crate path of the generated messages.

  @state:happy
  Scenario: The package exports the schema of every message its signatures name
    Given the widgets server has the widget "w1" named "Anvil"
    And the widget "w1" has the parts "bolt"
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the parts of the widget "w1" through the SDK
    Then every part the caller received is a message of the exported schema "WidgetPartSchema"
    And the package exports the schemas "GetWidgetArgsSchema,ListWidgetPartsArgsSchema,BatchGetWidgetsArgsSchema,WidgetPartSchema,WidgetSchema"
    And the schema "GetWidgetArgsSchema" describes the message "contract.sdk.v1.GetWidgetArgs"
    And the schema "WidgetSchema" describes the message "contract.v1.Widget"

  @state:happy
  Scenario: The package exports an enum a signature message carries as a value
    Given the widgets server has the widget "w1" named "Anvil"
    And the widget "w1" has the parts "bolt"
    And the SDK caller is "ada" of organization "acme"
    When the caller lists the parts of the widget "w1" through the SDK
    Then every part the caller received carries the member "UNSPECIFIED" of the exported enum "WidgetPartKind"
    And the package exports the enum "WidgetPartKind" with the member "FRAME" valued 1
