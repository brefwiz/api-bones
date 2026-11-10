# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: The Connect package ships only what a page imports

  A browser page that imports one small export from the package must not carry
  the field-validation data (currency codes, validators, units) that only the
  field-validation API needs. The package declares itself free of import-time
  effects, so a bundler keeps just the modules the import reaches. The
  contract is read from the built package, which is what consumers resolve.

  Scenario: Importing the replay check leaves the field-validation data out
    Given a page that imports only "isReplayableTransportFailure" from the "root" entry
    When the page is bundled for the browser
    Then the bundle carries none of the field-validation data
    And the bundle is smaller than 450 bytes
    And the replay check still treats an unavailable failure as replayable

  Scenario: Importing the browser transport leaves the field-validation data out
    Given a page that imports only "configureConnectTransport" from the "web" entry
    When the page is bundled for the browser
    Then the bundle carries none of the field-validation data
    And the bundle is smaller than 16000 bytes
    And the browser transport still builds a callable transport

  # Leaving the data out of small imports must not remove it from the API that
  # needs it.
  Scenario: The field-validation API still carries its data
    Given a page that imports only "validateDefinition" from the "root" entry
    When the page is bundled for the browser
    Then the bundle carries the field-validation data
    And the field-validation API still reports a malformed definition key
