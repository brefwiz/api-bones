# SPDX-License-Identifier: MIT
@library @ts-sdk-only
Feature: The test service answers a browser preflight for loopback pages only

  # The TypeScript test service is the loopback server a browser page under
  # test calls; only a page served from this machine may call it, so it allows
  # a preflight from a loopback origin, with the headers the page asks for, and
  # refuses any other origin without a single CORS header. The Rust test
  # service serves no browser, so this contract has no Rust counterpart.
  @state:happy
  Scenario: A preflight from a loopback page is allowed with the headers it asks for
    Given the test service is running
    When a page at "http://localhost:5173" asks to call it with the headers "content-type, connect-protocol-version"
    Then the preflight is answered with status 204
    And the preflight allows the origin "http://localhost:5173"
    And the preflight allows the headers "content-type, connect-protocol-version"

  @state:error
  Scenario: A preflight from another origin is refused with no CORS header
    Given the test service is running
    When a page at "https://evil.example" asks to call it with the headers "content-type"
    Then the preflight is answered with status 403
    And the preflight allows no origin

  # The control port is the steps process's alone: a rebound DNS name or a
  # cross-site form must not reconfigure the service.
  @state:error
  Scenario: A control request under another Host is refused
    Given the test service is running
    When the control port is asked for the seen calls under the Host "rebound.example"
    Then the control port answers with status 403

  @state:error
  Scenario: A non-JSON configuration is refused and leaves the service unchanged
    Given the test service is running
    When the control port is sent the widget "refused-widget" named "Ghost" as "text/plain"
    Then the control port answers with status 415
    And the service answers a request for the widget "refused-widget" with status 404

  @state:happy
  Scenario: A JSON configuration is applied
    Given the test service is running
    When the control port is sent the widget "applied-widget" named "Real" as "application/json"
    Then the control port answers with status 200
    And the service answers a request for the widget "applied-widget" with status 200
