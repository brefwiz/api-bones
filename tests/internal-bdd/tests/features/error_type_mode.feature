# SPDX-License-Identifier: MIT
@library @error
Feature: Error type URI mode

  Every problem document carries a `type` URI. Unless the application chooses
  a scheme, errors are URNs in the `api-bones` namespace, and an application
  that sets a scheme gets exactly that scheme for every error slug.

  Scenario: Errors are URNs in the api-bones namespace by default
    When the error type URI for "not-found" is rendered
    Then the error type URI is "urn:api-bones:error:not-found"

  Scenario: An application URN namespace applies to every slug
    Given the application sets the URN namespace "billing"
    When the error type URI for "not-found" is rendered
    Then the error type URI is "urn:billing:error:not-found"

  Scenario: An application base URL applies to every slug
    Given the application sets the base URL "https://docs.example.com/errors/"
    When the error type URI for "not-found" is rendered
    Then the error type URI is "https://docs.example.com/errors/not-found"
