# SPDX-License-Identifier: MIT
@library @automation
Feature: Automation manifest compilation, signature and admission

  A release compiles the automation surface a product declares into one
  manifest, signs it, and the layer admits it only when the signature
  verifies against a trusted key valid at that moment and the manifest is
  the product's own and matches the digest the product serves. A declaration
  that breaks a rule the compiler enforces is refused, naming the rule, and a
  product's build applies the same rules without the product's identity; the
  shared document check resolves bindings against each product's admitted
  manifest and requires required parameters to have values at publish.

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
      | a lasting action that does not declare its idempotency | A4 |
      | a resolver on an action that is not a read | A5 |
      | a request that carries an untyped payload | A6 |
      | a trigger field without a classification | C1 |
      | a template with no steps                 | M1   |

  Scenario: A product's build checks its declarations without its identity
    Given a valid orders contract
    When the declarations are checked without a product identity
    Then the check lists the declared trigger, action and template
    And the check gives the digest the release manifest of product "orders" carries

  Scenario Outline: A declaration that breaks a rule fails the product's build naming it
    Given an orders contract with <defect>
    When the declarations are checked without a product identity
    Then the check is refused under rule "<rule>"

    Examples: refused at build
      | defect                                   | rule |
      | a trigger without a label                | T1   |
      | an action without an effect              | A1   |
      | an action without a required grant       | A2   |
      | an action that streams its response      | A3   |
      | a template naming an undeclared trigger  | D1   |
      | a template step on a person-reserved action | D2 |
      | an action that is person-reserved and planner-pinned | A7 |
      | a lasting action that does not declare its idempotency | A4 |
      | a resolver on an action that is not a read | A5 |
      | a request that carries an untyped payload | A6 |
      | a trigger field without a classification | C1 |
      | a template with no steps                 | M1   |

  Scenario: A template that names its own product is refused at release
    Given an orders contract with a template naming its own product
    When the release "1.2.3" of product "orders" is compiled
    Then the compilation is refused under rule "M1"

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

  Scenario Outline: Admitted product schemas resolve valid document bindings
    Given a document with "<case>" against admitted product manifests
    When the shared document check runs for "admission"
    Then the document check accepts it

    Examples:
      | case                    |
      | open required parameter |
      | local reference         |
      | cross-product trigger   |
      | matching trigger        |
      | matching output         |
      | matching repeated       |
      | matching enum           |
      | matching message        |
      | matching map            |
      | matching resolver       |

  Scenario Outline: Publishing obtains required parameter values from arguments or defaults
    Given a document with "<case>" against admitted product manifests
    When the shared document check runs for "publish"
    Then the document check accepts it

    Examples:
      | case               |
      | argument value     |
      | default value      |
      | optional parameter |

  Scenario Outline: A document is refused when its bindings or values violate a rule
    Given a document with "<case>" against admitted product manifests
    When the shared document check runs for "<phase>"
    Then the document check refuses it under rule "<rule>"

    Examples:
      | case                      | phase     | rule |
      | open required parameter   | publish   | D6   |
      | wrong argument            | publish   | D6   |
      | unknown argument          | publish   | D6   |
      | duplicate argument        | publish   | D6   |
      | wrong argument            | admission | D6   |
      | unknown action            | admission | D1   |
      | unknown product           | admission | D1   |
      | unknown trigger           | admission | D1   |
      | missing manifest          | admission | D1   |
      | person-reserved action    | admission | D2   |
      | later output              | admission | D3   |
      | wrong binding type        | admission | D4   |
      | unknown target field      | admission | D4   |
      | unknown source field      | admission | D4   |
      | repeated mismatch         | admission | D4   |
      | different enum            | admission | D4   |
      | different message         | admission | D4   |
      | changed message           | admission | D4   |
      | map mismatch              | admission | D4   |
      | output mismatch           | admission | D4   |
      | literal mismatch          | admission | D4   |
      | unknown parameter         | admission | D5   |
      | resolver without resolves | admission | D5   |
