# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: Connect workload identity

  Workload identity reads the local Workload API socket and validates X.509
  certificates with Node's crypto, so it ships from the Node entry beside the
  Node transport, never from the runtime-agnostic entry browser code imports.

  Scenario Outline: A mesh workload's trust domain comes from its SPIFFE ID
    Given a workload whose SVID names "<spiffe_id>"
    Then its trust domain is "<trust_domain>"

    Examples:
      | spiffe_id                          | trust_domain  |
      | spiffe://example.org/ns/web/sa/api | example.org   |
      | spiffe://mesh.internal/workload    | mesh.internal |
      | https://example.org/ns/web         | none          |

  # No trust domain means no peer allow-list to derive. The transport refuses
  # rather than falling back to an anonymous identity.
  Scenario: An SVID with no usable trust domain is refused
    Given a workload whose SVID names "https://example.org/ns/web"
    Then deriving its TLS identity fails as "unusable_trust_domain"

  # A named workload is reached at a derived address and pinned to a derived
  # identity: the caller's own, with only the trailing service swapped. The
  # organization segment is carried over, so a peer is never resolved across
  # organizations.
  Scenario Outline: A named workload is reached in the caller's own organization
    Given a workload whose SVID names "<own_id>"
    When it reaches the workload "<service>"
    Then the peer address is "<url>"
    And the peer identity is "<peer_id>"

    Examples:
      | own_id                              | service | url                  | peer_id                                 |
      | spiffe://prod.example/o/acme/svc/bff | ledger  | https://ledger:8080  | spiffe://prod.example/o/acme/svc/ledger |
      | spiffe://prod.example/svc/bff        | ledger  | https://ledger:8080  | spiffe://prod.example/svc/ledger        |

  # The name is interpolated into both an address and an identity, so anything
  # that is not a single DNS label is refused outright.
  Scenario Outline: A workload name that is not a DNS label is refused
    Given a workload whose SVID names "spiffe://prod.example/o/acme/svc/bff"
    When it tries to reach the workload "<service>"
    Then reaching the peer fails as "invalid_workload_name"

    Examples:
      | service    |
      | Ledger     |
      | led/ger    |
      | ledger:80  |
      | -ledger    |

  Scenario: An identity with no service to replace yields no peer
    Given a workload whose SVID names "spiffe://prod.example/o/acme"
    When it tries to reach the workload "ledger"
    Then reaching the peer fails as "unusable_peer_identity"
