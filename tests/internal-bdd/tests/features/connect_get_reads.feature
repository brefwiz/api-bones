# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect GET for policy-eligible reads

  A generated Connect client sends every unary call as a POST. A read whose
  generated policy declares it side-effect free, non-sensitive and bounded may
  travel as a Connect GET instead, so a cache or a GET-only lane can serve it.
  The choice is made once, where the client's transport is built, and from the
  generated policy alone: a caller never selects it per call. The browser
  transport of the same package makes the same decision.

  Scenario: An eligible read is sent as a Connect GET
    Given a client transport built from a policy declaring "Get" a side-effect-free non-sensitive read
    When the client calls "Get" with a protobuf message
    Then the request is a GET to "/pkg.v1.Svc/Get"
    And the query names "connect=v1", "encoding=proto" and a base64url message
    And the request has no content type

  Scenario Outline: Any other method keeps its POST
    Given a client transport built from a policy declaring "Get" a side-effect-free non-sensitive read
    When the client calls "<method>" with a protobuf message
    Then the request is a POST to "/pkg.v1.Svc/<method>"
    And the request keeps its content type

    Examples: methods the policy does not grant as reads
      | method  |
      | Update  |
      | Secret  |
      | Unknown |

  Scenario: A streaming call is never sent as a GET
    Given a client transport built from a policy declaring "Get" a side-effect-free non-sensitive read
    When the client opens a streaming call to "Get"
    Then the request is a POST to "/pkg.v1.Svc/Get"

  Scenario: A GET read carries no precondition
    Given a client transport built from a policy declaring "Get" a side-effect-free non-sensitive read
    And the transport also attaches the default precondition
    When the client calls "Get" with a protobuf message
    Then the request is a GET to "/pkg.v1.Svc/Get"
    And the request carries no if-match header
