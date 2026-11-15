# SPDX-License-Identifier: MIT
@library @protos @rust-sdk-only
Feature: Keyed writes

  A contract author marks a write as keyed with `bones.v1.idempotency_key`.
  The marker applies to a single write and to a batch of it alike: the
  request holds a top-level singular `string idempotency_key`, the call
  declares `IDEMPOTENT` and `ALREADY_EXISTS`, and it is never a read. A batch
  and the single it names are keyed together, so the batch keeps the single's
  level and errors as it does for any other pair. The marker is read back from
  the compiled contract and the rules run over it. The surface is Rust alone:
  the rules ship in api-bones-automation and the option in api-bones-protos.

  @state:happy
  Scenario: A keyed single and its keyed batch read back as keyed and conform
    Given a single declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "string idempotency_key = 2;" and a batch of it declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS,INVALID_ARGUMENT" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the single and the batch read back as keyed
    And every keyed write conforms

  @state:happy
  Scenario: An unkeyed single and its unkeyed batch conform
    Given a single declaring "NO_SIDE_EFFECTS" whose request holds "none" and a batch of it declaring "NO_SIDE_EFFECTS" whose request holds "none"
    When the contract is compiled and the keyed write rules are checked
    Then the single and the batch read back as not keyed
    And every keyed write conforms

  @state:error
  Scenario: A keyed batch over an unkeyed single is refused
    Given a single declaring "IDEMPOTENT,ALREADY_EXISTS" whose request holds "none" and a batch of it declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS,INVALID_ARGUMENT" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the batch rule refuses it as "is keyed but the method it names is not"

  @state:error
  Scenario: A keyed single with an unkeyed batch is refused
    Given a single declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "string idempotency_key = 2;" and a batch of it declaring "IDEMPOTENT,ALREADY_EXISTS" whose request holds "none"
    When the contract is compiled and the keyed write rules are checked
    Then the batch rule refuses it as "is not keyed but the method it names is"

  @state:error
  Scenario Outline: A keyed single without a top-level singular string idempotency_key is refused
    Given a single declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "<field>" and a batch of it declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS,INVALID_ARGUMENT" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule refuses it as "is keyed but its request has no top-level string idempotency_key"

    Examples:
      | field                                |
      | none                                 |
      | optional string idempotency_key = 2; |
      | repeated string idempotency_key = 2; |
      | int64 idempotency_key = 2;           |

  @state:error
  Scenario: A keyed batch is refused when only the items carry the key
    Given a single declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "string idempotency_key = 2;" and a batch of it declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "none"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule refuses it as "is keyed but its request has no top-level string idempotency_key"

  @state:error
  Scenario: A keyed single that is not IDEMPOTENT is refused
    Given a single declaring "KEYED,ALREADY_EXISTS" whose request holds "string idempotency_key = 2;" and a batch of it declaring "KEYED,ALREADY_EXISTS,INVALID_ARGUMENT" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule refuses it as "is keyed but declares IDEMPOTENCY_UNKNOWN, not IDEMPOTENT"

  @state:error
  Scenario: A keyed read is refused
    Given a single declaring "KEYED,NO_SIDE_EFFECTS,ALREADY_EXISTS" whose request holds "string idempotency_key = 2;" and a batch of it declaring "KEYED,NO_SIDE_EFFECTS,ALREADY_EXISTS,INVALID_ARGUMENT" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule refuses it as "is keyed but declares NO_SIDE_EFFECTS"

  @state:error
  Scenario: A keyed single that does not declare ALREADY_EXISTS is refused
    Given a single declaring "KEYED,IDEMPOTENT" whose request holds "string idempotency_key = 2;" and a batch of it declaring "KEYED,IDEMPOTENT,INVALID_ARGUMENT" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule refuses it as "is keyed but does not declare ALREADY_EXISTS for a reused key"

  @state:happy
  Scenario: A method that is not keyed passes whatever its request holds
    Given a single declaring "IDEMPOTENT" whose request holds "optional string idempotency_key = 2;" and a batch of it declaring "IDEMPOTENT" whose request holds "none"
    When the contract is compiled and the keyed write rules are checked
    Then every keyed write conforms

  @state:error
  Scenario: A keyed request embedding a keyed request without declaring INVALID_ARGUMENT is refused
    Given a single declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "string idempotency_key = 2;" and a batch of it declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule refuses it as "is keyed and embeds the request of a keyed method but does not declare INVALID_ARGUMENT for a non-empty embedded key"

  @state:error
  Scenario: A keyed write whose request holds nothing but the key and declares ALREADY_EXISTS is refused
    Given a single declaring "KEYED,IDEMPOTENT,ALREADY_EXISTS" whose request holds "bodyless" and a batch of it declaring "none" whose request holds "none"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule refuses it as "is keyed with a request holding nothing but the key but declares ALREADY_EXISTS, which cannot occur"

  @state:happy
  Scenario: A keyed write whose request holds nothing but the key conforms without ALREADY_EXISTS
    Given a single declaring "KEYED,IDEMPOTENT" whose request holds "bodyless" and a batch of it declaring "none" whose request holds "none"
    When the contract is compiled and the keyed write rules are checked
    Then the key rule accepts it

  @state:error
  Scenario: A keyed batch over a single whose request holds nothing but the key is refused
    Given a single declaring "KEYED,IDEMPOTENT" whose request holds "bodyless" and a batch of it declaring "KEYED,IDEMPOTENT,INVALID_ARGUMENT" whose request holds "string idempotency_key = 2;"
    When the contract is compiled and the keyed write rules are checked
    Then the batch rule refuses it as "is keyed but the request of the method it names holds nothing but the key"
