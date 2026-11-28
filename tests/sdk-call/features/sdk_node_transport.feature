# SPDX-License-Identifier: MIT
@library @sdk-call @node-transport @ts-sdk-only
Feature: The SDK under Node reaches the fleet with its workload identity and the public net without it

  Under Node the SDK's wasm core sends every request through a host exchange
  that holds the sockets while the core decides the policy. A URL that names a
  workload of the fleet (a single DNS label, or one ending in `.svc` or
  `.svc.cluster.local`) is called over https with the caller's workload
  identity, or not at all: no call to the fleet falls back to the public net.
  Any other URL is called with the host's default roots and never presents a
  client certificate. Aborting a call resets its stream with CANCEL.

  @state:error
  Scenario: A call to the fleet over plain http is refused before it is sent
    Given the caller is anonymous
    And the SDK is opened at "http://billing.prod.svc"
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "INSECURE_IN_FLEET"
    And the call is refused with the platform code "PRECONDITION_FAILED"
    And the call is refused with the Connect code "failed_precondition"
    And the call is refused by the emitter "bones"

  @state:error
  Scenario: A call to the fleet with no Workload API socket is refused
    Given the caller is anonymous
    And the host names no Workload API socket
    And the SDK is opened at "https://billing.prod.svc"
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "WORKLOAD_IDENTITY_UNAVAILABLE"
    And the call is refused with the platform code "SERVICE_UNAVAILABLE"
    And the call is refused with the Connect code "unavailable"
    And the call is refused by the emitter "bones"

  @state:error
  Scenario: A call to the fleet whose Workload API socket nothing listens on is refused
    Given the caller is anonymous
    And the host names a Workload API socket nothing listens on
    And the SDK is opened at "https://billing.prod.svc"
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "WORKLOAD_IDENTITY_UNAVAILABLE"
    And the call is refused with the platform code "SERVICE_UNAVAILABLE"
    And the call is refused with the Connect code "unavailable"
    And the call is refused by the emitter "bones"

  @state:error
  Scenario Outline: A host that is not Node refuses a call to the fleet
    Given the caller is anonymous
    And the host defines the global "<global>"
    And the SDK is opened at "https://billing.prod.svc"
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "WORKLOAD_IDENTITY_UNAVAILABLE"
    And the call is refused by the emitter "bones"

    Examples:
      | global            |
      | Deno              |
      | Bun               |
      | window            |
      | WorkerGlobalScope |

  @state:error
  Scenario: A host that is not Node refuses a call to the fleet over plain http
    Given the caller is anonymous
    And the host defines the global "Deno"
    And the SDK is opened at "http://billing.prod.svc"
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the code "INSECURE_IN_FLEET"

  @state:happy
  Scenario: A host that is not Node still reaches the public net
    Given the caller is anonymous
    And the host defines the global "Deno"
    And a public HTTP/1.1 peer that answers every request with 404
    When the caller gets the widget "w1" through the SDK
    Then the peer served 1 request
    And the call is refused with the Connect code "unimplemented"

  @state:error
  Scenario: A Node host without the api-bones exchange refuses the call and does not fall back
    Given the caller is anonymous
    And a public HTTP/1.1 peer that answers every request with 404
    And the host has no api-bones exchange
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the platform code "PRECONDITION_FAILED"
    And the call is refused with the Connect code "failed_precondition"
    And the call is refused by the emitter ""
    And the refusal says "exchange"
    And the peer served 0 requests

  @state:error
  Scenario: A Node host without crypto.getRandomValues refuses a call to the fleet
    Given the caller is anonymous
    And the tracer does not record
    And the SDK is opened at "https://billing.prod.svc"
    And the host loses crypto.getRandomValues
    When the caller gets the widget "w1" through the SDK
    Then the call is refused with the platform code "PRECONDITION_FAILED"
    And the call is refused with the Connect code "failed_precondition"
    And the call is refused by the emitter ""
    And the refusal says "getRandomValues"

  @state:error
  Scenario: A host that loses its secure random source mid-call does not replay the call
    Given the caller is anonymous
    And a public peer that answers unavailable once and then the host loses its random source
    When the caller gets the widget "w1" through the SDK
    Then the peer served 1 request
    And the call is refused with the Connect code "failed_precondition"
    And the call is refused by the emitter ""
    And the refusal says "getRandomValues"

  @state:happy
  Scenario: A call to the public net presents no client certificate
    Given the caller is anonymous
    And a public https peer that asks every client for a certificate
    When the caller gets the widget "w1" through the SDK
    Then the peer served one connection
    And the peer saw no client certificate
    And the call is refused with the Connect code "unimplemented"

  @state:happy
  Scenario: Aborting a call resets its stream with CANCEL
    Given the caller is anonymous
    And a public peer that never answers
    And the caller has opened a list of the numbers through the SDK
    And a read of the first page of the list from that peer is pending
    When the caller cancels the list
    Then the peer saw its stream reset with CANCEL
    And the abandoned read gives no page
