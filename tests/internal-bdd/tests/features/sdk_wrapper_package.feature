# SPDX-License-Identifier: MIT
@library @sdk-call
Feature: A Layer 2 crate generates the wrapper of the package its manifest names

  The TypeScript wrapper of a Layer 2 crate is the package named by the
  `package.json` tracked in its output directory: one crate, one wrapper, one
  package. A service whose crates are published as different packages gets one
  wrapper per package. Generation is refused when the tracked manifest is
  missing or untracked, when no SDK surface lists its package as a TypeScript
  target, when an export belongs to a surface that does not list it, when a
  surface that lists it names an endpoint the crate does not serve, and when
  the crate links another crate's wasm exports.

  @state:happy
  Scenario: Two crates published as two packages generate one wrapper each
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "beta" published as "@acme/beta"
    When the wrapper is generated for every crate
    Then every generation succeeded
    And the wrapper of the crate "alpha" is the package "@acme/alpha"
    And the wrapper of the crate "beta" is the package "@acme/beta"

  @state:error
  Scenario: A crate whose package no surface lists is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "beta" published as "@acme/beta"
    And the SPEC lists only the package "@acme/alpha"
    When the wrapper is generated for the crate "beta"
    Then the generation is refused with "no sdk_surfaces entry lists `@acme/beta` as a typescript target package"

  @state:error
  Scenario: An output directory without a tracked package.json is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the package.json of the crate "alpha" is not tracked by git
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "is not tracked by git: the wrapper package is the name in the tracked package.json"

  @state:error
  Scenario: An output directory without a package.json is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" has no package.json
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "the wrapper package is the name in the tracked package.json"

  @state:error
  Scenario: An export of a surface that does not list the package is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the SPEC moves the surface "s-alpha" to the package "@acme/elsewhere"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "package `@acme/alpha` is not a typescript target of surface `s-alpha`, which the exports of `alpha` name"

  @state:error
  Scenario: A surface that lists the package and names an endpoint the crate does not serve is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the surface "s-alpha" names the endpoint "acme.v1.Alpha/Unserved"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "surface `s-alpha` lists package `@acme/alpha` and names endpoint `acme.v1.Alpha/Unserved`, which no export of `alpha` serves"

  @state:error
  Scenario: A crate that enables another crate's wasm exports is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" enables the wasm feature of the crate "beta"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "feature `wasm` of `alpha` enables `beta/wasm`: a wrapper links no other crate's wasm exports"
