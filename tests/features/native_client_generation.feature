# SPDX-License-Identifier: MIT
Feature: Generate callable native clients from proto descriptors
  Native operations share proto source with RPC contracts without becoming RPCs.

  Scenario: Generated clients publish opaque bytes with stable identity
    Given a generated native client backed by a recording transport
    When it publishes opaque bytes with stable message identity
    Then transport receives one typed publication

  Scenario: Generated durable deliveries expose acknowledgement handles
    Given a generated native client backed by a recording transport
    When it receives one durable delivery
    Then delivery can be acknowledged

  Scenario: Generated durable deliveries can be negatively acknowledged
    Given a generated native client backed by a recording transport
    When it receives one durable delivery
    Then delivery can be negatively acknowledged

  Scenario: Negative acknowledgement handles are one shot
    Given a generated native client backed by a recording transport
    When it receives one durable delivery
    And it negatively acknowledges the delivery
    Then reusing the negatively acknowledged handle is refused without dispatch

  Scenario: Acknowledgement handles are one shot
    Given a generated native client backed by a recording transport
    When it receives one durable delivery
    And it acknowledges the delivery
    Then reusing the acknowledgement handle is refused

  Scenario: Failed acknowledgement leaves handle retryable
    Given a generated native client backed by a transport that fails one acknowledgement
    When it receives one durable delivery
    And first acknowledgement attempt fails
    Then acknowledgement retry succeeds with same handle

  Scenario: Interrupted acknowledgement leaves handle retryable
    Given a generated native client backed by a transport that interrupts one acknowledgement
    When it receives one durable delivery
    And first acknowledgement attempt is interrupted
    Then acknowledgement retry succeeds with same handle

  Scenario: Failed negative acknowledgement leaves handle retryable
    Given a generated native client backed by a transport that fails one negative acknowledgement
    When it receives one durable delivery
    And first negative acknowledgement attempt fails
    Then negative acknowledgement retry succeeds with same handle

  Scenario: Provider denial remains typed
    Given a generated native client backed by a denying transport
    When it attempts publication
    Then generated client reports permission denied

  Scenario: Cancellation and deadline reach native transport
    Given a generated native client backed by a recording transport
    When it publishes with cancellation and deadline options
    Then transport receives cancellation and deadline options

  Scenario: Already cancelled calls never dispatch
    Given a generated native client backed by a recording transport
    When it publishes with an already cancelled option
    Then generated client reports cancellation
    And native transport receives no call

  Scenario: Expired calls never dispatch
    Given a generated native client backed by a recording transport
    When it publishes with an expired deadline
    Then generated client reports deadline exceeded
    And native transport receives no call

  Scenario: Native declarations create no RPC methods
    Given a proto descriptor with native operations and no RPC methods
    When native clients are generated twice
    Then generated outputs are byte identical
    And descriptor RPC inventory remains empty

  Scenario: Malformed native declarations fail generation
    Given an ambiguous native descriptor
    When native client generation is attempted
    Then generation fails with a descriptor error

  Scenario: Stale generated clients are refused
    Given a proto descriptor with native operations and no RPC methods
    When checked generated output differs from descriptor generation
    Then stale output is reported for both targets

  Scenario: Cancellation is an asynchronous client result
    Given a generated native client backed by a recording transport
    When it starts publication with an already cancelled option
    Then cancellation is returned by the async result

  Scenario: Generated identifiers are safe and collision free
    Given native descriptors with invalid and colliding identifiers
    When native client generation validates identifiers
    Then unsafe identifiers are rejected and package names disambiguate clients

  Scenario: Buf plugin generated clients are callable
    Given a generated native client backed by a recording transport
    When it publishes through the buf plugin generated client
    Then transport receives one typed publication

  Scenario: Buf plugin ignores ordinary RPC-only inputs
    Given an ordinary RPC descriptor without native annotations
    When native plugin generation is attempted
    Then no native client files are emitted for either target

  Scenario: Buf plugin combines native declarations across directories
    Given native declarations selected from multiple source directories
    When native plugin generation is attempted
    Then one output per target contains every selected native client
