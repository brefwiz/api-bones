# SPDX-License-Identifier: MIT
@library @automation
Feature: Automation manifest compilation, signature and admission

  A release compiles the automation surface a product declares into one
  manifest, signs it, and the layer admits it only when the signature
  verifies against a trusted key valid at that moment and the manifest is
  the product's own and matches the digest the product serves. A declaration
  that breaks a rule the compiler enforces is refused, naming the rule; the
  rules A4, A5, A6 and C1 and the type-check of a binding against another
  product's action are not enforced yet.

  Scenario: A valid contract compiles into a manifest for its release
    Given a valid orders contract
    When the release "1.2.3" of product "orders" is compiled
    Then the manifest names product "orders", version "1.2.3" and the digest of the contract source
    And compiling again gives the same bytes

  Scenario Outline: A declaration that breaks a rule is refused naming it
    Given an orders contract with <defect>
    When the release "1.2.3" of product "orders" is compiled
    Then the compilation is refused under rule "<rule>"

    Examples: refused declarations
      | defect                                   | rule |
      | a trigger without a label                | T1   |
      | an action without an effect              | A1   |
      | an action without a required grant       | A2   |
      | an action that streams its response      | A3   |
      | a template naming an undeclared trigger  | D1   |
      | a template step on a person-reserved action | D2 |
      | an action that is person-reserved and planner-pinned | A7 |
      | a template with no steps                 | M1   |

  Scenario: A signed manifest is admitted for the product that serves it
    Given a signed manifest of product "orders" under key "k1" valid from 100 to 200
    When the layer admits it at 150 for product "orders" serving its own digest
    Then the manifest is admitted

  Scenario Outline: A signed manifest is refused when it cannot be trusted
    Given a signed manifest of product "orders" under key "k1" valid from 100 to 200
    When the layer admits it at <at> for product "<product>" serving <digest>
    Then the admission is refused as "<reason>"

    Examples: refusals
      | at  | product | digest           | reason           |
      | 99  | orders  | its own digest   | key not valid    |
      | 200 | orders  | its own digest   | key not valid    |
      | 150 | billing | its own digest   | product mismatch |
      | 150 | orders  | another digest   | digest mismatch  |

  Scenario: A manifest altered after signing is refused
    Given a signed manifest of product "orders" under key "k1" valid from 100 to 200
    And the manifest is altered after signing
    When the layer admits it at 150 for product "orders" serving its own digest
    Then the admission is refused as "bad signature"

  Scenario: A manifest signed by a key the layer does not trust is refused
    Given a signed manifest of product "orders" under key "k1" valid from 100 to 200
    And the layer trusts only key "k2"
    When the layer admits it at 150 for product "orders" serving its own digest
    Then the admission is refused as "unknown key"
