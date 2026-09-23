# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect client bearer header

  `ConnectConfigExt::with_bearer` sets the client's `authorization` default
  header. The underlying `ClientConfig::with_default_header` appends, so a
  naive delegation would leave a stale credential behind every time a caller
  rotates a token, and `HeaderMap::get` returns the first value -- so every
  request after a rotation would keep sending the old one instead of the new.

  Scenario: Setting the bearer token twice replaces rather than appends
    Given a fresh client config
    When the bearer token is set to "first"
    And the bearer token is set to "second"
    Then the config carries exactly one authorization header, "Bearer second"

  Scenario: Setting the bearer token replaces a pre-existing authorization header
    Given a client config already carrying an authorization header of "Bearer stale"
    When the bearer token is set to "fresh"
    Then the config carries exactly one authorization header, "Bearer fresh"

  Scenario: Setting the bearer token preserves unrelated default headers
    Given a fresh client config
    And the config carries an "x-org-id" header of "org-abc"
    When the bearer token is set to "tok123"
    Then the config carries exactly one authorization header, "Bearer tok123"
    And the config still carries an "x-org-id" header of "org-abc"
