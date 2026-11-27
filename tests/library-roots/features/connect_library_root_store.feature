# SPDX-License-Identifier: MIT
@library @connect @rust-sdk-only
Feature: Connect library root store

  A library crate (api-bones-page, api-bones-batch) verifies a TLS server
  against the bundled webpki roots and never the operating system store; a
  binary that wants the system store enables `api-bones/connect`. An operator
  who names a private CA in `SSL_CERT_FILE` gets exactly the certificates that
  file lists, and a named file that is missing or holds no certificate fails
  the read. The TypeScript package leaves root trust to its runtime, so this
  surface is Rust alone.

  Scenario: A library crate trusts the bundled roots when no file is named
    Given no SSL_CERT_FILE or SSL_CERT_DIR is set
    When a library-crate client reads the root store
    Then the store trusts exactly the bundled webpki root set

  Scenario Outline: The certificates an operator names are the roots a library crate trusts
    Given a PEM file holding <count> CA certificates
    When the root store is read with SSL_CERT_FILE naming that file
    Then the store trusts exactly <count> root certificates

    Examples:
      | count |
      | 1     |
      | 2     |

  @state:error
  Scenario Outline: A library crate refuses a root file it cannot read
    Given SSL_CERT_FILE names a file <problem>
    When the root store is read with SSL_CERT_FILE naming that file
    Then reading the root store fails naming that file

    Examples:
      | problem                |
      | that does not exist    |
      | holding no certificate |
