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
