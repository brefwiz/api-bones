# SPDX-License-Identifier: MIT
@library @error
Feature: Error type URI mode

  Every problem document carries a `type` URI. Unless the application chooses
  a scheme, errors are URNs in the `api-bones` namespace, and an application
  that sets a scheme gets exactly that scheme for every error slug.

  The mode is process-wide, so one scenario walks the whole sequence in order.

  Scenario: Error type URIs follow the default, then the scheme the application sets
    When the error type URI for "not-found" is rendered
    Then the error type URI is "urn:api-bones:error:not-found"
    Given the application sets the URN namespace "billing"
    When the error type URI for "not-found" is rendered
    Then the error type URI is "urn:billing:error:not-found"
    Given the application sets the base URL "https://docs.example.com/errors/"
    When the error type URI for "not-found" is rendered
    Then the error type URI is "https://docs.example.com/errors/not-found"
