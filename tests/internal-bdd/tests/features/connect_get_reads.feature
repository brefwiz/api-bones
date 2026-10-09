# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect GET for policy-eligible reads

  A generated Connect client sends every unary call as a POST. A read whose
  generated policy declares it side-effect free, non-sensitive, privately
  cacheable and bounded may travel as a Connect GET instead, so a cache or a
  GET-only lane can serve it. A read the policy declares public goes to the
  product's public lane as an anonymous GET. The choice is made once, where
  the client's transport is built, and from the generated policy alone: a
  caller never selects it per call. The browser transport of the same package
  makes the same decision, and a policy it would reject grants nothing here.

  Scenario: An eligible read is sent as a Connect GET
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    When the client calls "Get" with a protobuf message
    Then the request is a GET to "/pkg.v1.Svc/Get"
    And the query names "connect=v1", "encoding=proto" and a base64url message
    And the request has no content type

  Scenario: A JSON message is sent as base64url too
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    When the client calls "Get" with a JSON message
    Then the request is a GET to "/pkg.v1.Svc/Get"
    And the query names "connect=v1", "encoding=json" and a base64url message

  Scenario: A compressed message names its compression
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    When the client calls "Get" with a protobuf message compressed with "gzip"
    Then the request is a GET to "/pkg.v1.Svc/Get"
    And the query names "compression=gzip", "encoding=proto" and a base64url message

  Scenario Outline: A method the policy does not grant as a credentialed read keeps its POST
    Given a policy declaring "<method>" as "<idempotency>" with sensitivity "<sensitivity>", cache scope "<scope>" and a URL budget of 512
    When the client calls "<method>" with a protobuf message
    Then the request is a POST to "/pkg.v1.Svc/<method>"
    And the request keeps its content type "application/proto"

    Examples: methods the policy does not grant
      | method  | idempotency     | sensitivity   | scope    |
      | Update  | IDEMPOTENT      | NON_SENSITIVE | PRIVATE  |
      | Secret  | NO_SIDE_EFFECTS | SENSITIVE     | PRIVATE  |
      | Cold    | NO_SIDE_EFFECTS | NON_SENSITIVE | NO_STORE |
      | Future  | FUTURE_VALUE    | NON_SENSITIVE | PRIVATE  |

  Scenario: An unlisted method keeps its POST
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    When the client calls "Unknown" with a protobuf message
    Then the request is a POST to "/pkg.v1.Svc/Unknown"

  Scenario: A streaming call is never sent as a GET
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    When the client opens a streaming call to "Get"
    Then the request is a POST to "/pkg.v1.Svc/Get"

  Scenario: A call whose URL is over the method's budget keeps its POST
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 200
    When the client calls "Get" with a protobuf message of 400 bytes
    Then the request is a POST to "/pkg.v1.Svc/Get"
    And the request keeps its content type "application/proto"

  Scenario: A policy with one malformed entry grants no read at all
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    And the policy also holds an entry "Broken" with no cache policy
    When the client calls "Get" with a protobuf message
    Then the request is a POST to "/pkg.v1.Svc/Get"

  Scenario: A URL budget above the ceiling grants no read at all
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 4097
    When the client calls "Get" with a protobuf message
    Then the request is a POST to "/pkg.v1.Svc/Get"

  Scenario Outline: A GET read carries no precondition whichever transport wraps the other
    Given a policy declaring "Get" as "NO_SIDE_EFFECTS" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    And a policy declaring "Update" as "IDEMPOTENT" with sensitivity "NON_SENSITIVE", cache scope "PRIVATE" and a URL budget of 512
    And the default precondition transport with the GET transport as its <layer>
    When the client calls "Get" with a protobuf message
    And the client calls "Update" with a protobuf message
    Then the call to "Get" was a GET with no if-match header
    And the call to "Update" was a POST with if-match "*"

    Examples: nesting orders
      | layer |
      | outer |
      | inner |

  Scenario: A public read goes to the anonymous lane and carries no credential
    Given a public read "Open" served at the mount "/product"
    When the client calls "Open" with a protobuf message and a bearer token, a cookie, a CSRF token and a product header
    Then the request is a GET to "/public/product/pkg.v1.Svc/Open"
    And the request went to the anonymous lane
    And the request carries no authorization, cookie, x-csrf-token or x-product header

  Scenario: A public read is never sent as a credentialed GET
    Given a public read "Open" served at the mount "/product"
    When the client calls "Open" with a protobuf message and a bearer token on the credentialed transport
    Then the request is a POST to "/product/pkg.v1.Svc/Open"

  Scenario: A malformed public read never reaches the lane
    Given a public read "Open" served at the mount "/product"
    And the public read declaration is malformed
    When the client calls "Open" with a protobuf message and a bearer token, a cookie, a CSRF token and a product header
    Then the request is a POST to "/product/pkg.v1.Svc/Open"
    And the request went to the credentialed transport
