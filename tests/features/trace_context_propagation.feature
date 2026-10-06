# SPDX-License-Identifier: MIT
@library @propagation
Feature: Telemetry cannot supply request identity
  Tracing preserves the caller's credential and organization context while
  carrying trace context, baggage and ordinary custom propagation families.

  Scenario Outline: Propagation respects identity ownership
    Given propagation identity headers are "<state>"
    And a propagator carrying trace context, baggage and custom context
    When telemetry is injected through the "<entry>" entry
    Then request identity headers remain "<state>"
    And trace context, baggage and custom context reach the request

    Examples:
      | state   | entry   |
      | present | helper  |
      | absent  | helper  |
      | present | wrapper |
      | absent  | wrapper |

  Scenario Outline: Instrumentation failure leaves identity intact
    Given propagation identity headers are "present"
    And a propagator that fails after writing permitted context
    When telemetry is injected through the "<entry>" entry
    Then request identity headers remain "present"
    And permitted context written before failure reaches the request

    Examples:
      | entry   |
      | helper  |
      | wrapper |
