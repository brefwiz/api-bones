# SPDX-License-Identifier: MIT
@library @connect @batch @ts-sdk-only
Feature: Platform batch client construction

  TypeScript binds each generated client privately to its descriptor and prepared
  platform transport. Batching refuses clients and transports without that binding
  before any request reaches the server.

  Scenario Outline: Every platform entry point constructs a generated client that supports batching
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    And the platform entry point is "<entrypoint>"
    When the constructed platform client batch-gets "w1,w2"
    Then the constructed client returns the widgets "w1,w2"
    And the server received 1 "BatchGetWidgets" request

    Examples:
      | entrypoint |
      | default    |
      | web        |
      | node       |

  Scenario: A generated client without the private platform binding is refused before sending
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When batching wraps an ordinary generated client
    Then construction fails with "batching requires createPlatformClient"
    And the server received no request

  Scenario: An explicitly anonymous client sends no ambient credentials
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When an explicitly anonymous platform client batch-gets "w1,w2" carrying an ambient cookie
    Then anonymous batch requests omit credentials
    And every position fails with "unauthenticated"

  Scenario: An ordinary transport cannot acquire a platform client binding
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When a platform client is constructed with an ordinary Connect transport
    Then construction fails with "platform client requires a prepared platform transport"
    And the server received no request

  Scenario Outline: Descriptor and discovery drift refuses batch construction before sending
    Given the organization "acme" has widgets "w1,w2"
    And the caller is "ada" of organization "acme"
    When batching constructs a generated client with "<drift>" policy drift
    Then construction fails with "<message>"
    And the server received no request

    Examples:
      | drift                | message                                      |
      | unsupported schema   | is not supported                             |
      | empty discovery      | empty batch policy discovery                 |
      | missing single       | missing unary batch method                   |
      | missing batch        | missing unary batch method                   |
      | missing key          | missing batch field                          |
      | missing keys         | missing batch field                          |
      | missing results      | missing batch field                          |
      | non-string key       | batch keys must be string fields             |
      | scalar keys          | batch keys must be string fields             |
      | scalar results       | batch results must be repeated messages      |
      | wrong item response  | batch item must match single response        |
      | mutating batch       | must declare NO_SIDE_EFFECTS                 |
      | non-string result key | batch result key must be a string            |
      | wrong error type      | batch result error must be bones.v1.ItemError |
      | wrong item type       | batch item must match single response        |
      | missing outcome       | batch item must match single response        |
      | scalar group items    | batch groups must contain repeated items     |
      | wrong group item type | batch group items must match single List response |
