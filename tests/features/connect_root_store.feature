# SPDX-License-Identifier: MIT
@library @connect @rust-sdk-only
Feature: Connect root store

  A Connect client verifies a TLS server against the root certificates
  `api_bones::connect::root_store()` returns. An operator who injects a private
  CA names it in `SSL_CERT_FILE`, and the store holds exactly the certificates
  that file lists, so a dev or e2e peer is trusted and nothing else is.
  The TypeScript package leaves root trust to its runtime, so this surface is
  Rust alone.

  Scenario Outline: The certificates an operator names are the roots the client trusts
    Given a PEM file holding <count> CA certificates
    When the root store is read with SSL_CERT_FILE naming that file
    Then the store trusts exactly <count> root certificates

    Examples:
      | count |
      | 1     |
      | 2     |
