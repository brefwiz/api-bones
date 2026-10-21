# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect pinned-peer TLS

  A caller with no SVID of its own, such as a post-deploy job that holds no
  client certificate, still has to know whom it is talking to. It pins one exact
  SPIFFE ID against a committed trust bundle, and the SPIFFE client's pinned-peer
  verifier decides: the server's chain must end at a CA of the bundle, its leaf
  must be an X509-SVID (not a CA, exactly one URI SAN) naming the pinned ID, and
  the check compares no hostname. The Connect transports compose that verifier
  and add no check of their own, so Rust and TypeScript answer the same rows
  over real loopback TLS, and a refusal says why.

  Scenario: A pinned caller reaches the peer it pinned and sends no client certificate
    Given a trust bundle holding the test CA
    And a server presenting an SVID for "spiffe://prod.example/svc/signer" issued by the test CA
    When a caller pinning "spiffe://prod.example/svc/signer" connects
    Then the connection is established
    And the server completed its handshake having seen no client certificate

  Scenario Outline: A server that is not the pinned peer is refused, and the refusal names why
    Given a trust bundle holding the test CA
    And a server presenting <leaf>
    When a caller pinning "spiffe://prod.example/svc/signer" connects
    Then the connection is refused for <reason>

    Examples: another identity
      | leaf                                                                  | reason           |
      | an SVID for "spiffe://prod.example/svc/other" issued by the test CA   | a wrong identity |
      | an SVID for "spiffe://other.example/svc/signer" issued by the test CA | a wrong identity |

    Examples: a chain outside the bundle
      | leaf                                                                       | reason             |
      | an SVID for "spiffe://prod.example/svc/signer" issued by a stranger CA     | an untrusted chain |
      | a CA certificate for "spiffe://prod.example/svc/signer" issued by a stranger CA | an untrusted chain |
      | a certificate with no URI SAN issued by a stranger CA                      | an untrusted chain |

    Examples: a leaf that is not an X509-SVID
      | leaf                                                                                  | reason          |
      | a CA certificate for "spiffe://prod.example/svc/signer" issued by the test CA         | a non-SVID leaf |
      | a certificate with no URI SAN issued by the test CA                                   | a non-SVID leaf |
      | a certificate with the URI SANs "spiffe://prod.example/svc/signer,spiffe://prod.example/svc/other" issued by the test CA | a non-SVID leaf |
      | a certificate with the URI SANs "spiffe://prod.example/svc/other,spiffe://prod.example/svc/signer" issued by the test CA | a non-SVID leaf |

  Scenario Outline: A bundle that is not a set of CAs is refused before any connection
    Given a trust bundle holding <content>
    When a caller pinning "spiffe://prod.example/svc/signer" is built from that bundle
    Then it is refused as an invalid trust bundle

    Examples:
      | content                                |
      | nothing                                |
      | only a leaf certificate                |
      | the test CA and a leaf certificate     |
      | a certificate with no basicConstraints |

  Scenario: A pinned caller asked to call a cleartext address is refused before it connects
    Given a trust bundle holding the test CA
    When a caller pinning "spiffe://prod.example/svc/signer" is asked to call the cleartext address "http://signer.example:9/pkg.v1.Svc/Method"
    Then it is refused as a cleartext address

  Scenario Outline: A peer that is not an exact SPIFFE ID is refused before any connection
    Given a trust bundle holding the test CA
    When a caller pinning "<peer>" is built from that bundle
    Then it is refused as an invalid peer id

    Examples:
      | peer                            |
      | spiffe://prod.example/svc/*     |
      | spiffe://prod.example/*/signer  |
      | spiffe://prod.example/svc/sig*  |
      | spiffe://*.example/svc/signer   |
      | https://prod.example/svc/signer |
      | spiffe:///svc/signer            |
