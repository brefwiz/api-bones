# SPDX-License-Identifier: MIT
@library @sdk-call
Feature: A Layer 2 crate generates the wrapper of the package its manifest names

  The TypeScript wrapper of a Layer 2 crate is the package named by the
  `package.json` tracked in its output directory: one crate, one wrapper, one
  package. A service whose crates are published as different packages gets one
  wrapper per package. Generation is refused when the tracked manifest is
  missing or untracked, when no SDK surface lists its package as a TypeScript
  target, when an export belongs to a surface that does not list it, when a
  surface that lists it names an endpoint the crate does not serve, when the
  crate links another crate's wasm exports, and when the output directory
  tracks an `api-surface.d.ts` or a `src/` file the generator did not write.
  The `l2-methods.json` it writes resolves the endpoints of a Rust method per
  surface: one name on Types of different surfaces resolves to each surface's
  own endpoints, the same name on two Types of one surface is refused, and a
  tracked snapshot of another schema, or missing a field, is refused rather
  than replaced.

  The `package.json` the generator writes derives its registry access and its
  published files from the package: a published crate whose license is
  OSI-approved publishes with `public` access, any other license publishes
  `restricted`, and the files are the directories the exports reach.

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

  @state:happy
  Scenario: One method name on Types of different surfaces resolves to each surface's endpoints
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" exports "authorize" on the Type "Device" of the surface "s-device"
    And the crate "alpha" exports "authorize" on the Type "Flow" of the surface "s-flow"
    And the surface "s-device" serves the endpoint "acme.v1.Device/Authorize" through "authorize"
    And the surface "s-flow" serves the endpoint "acme.v1.Flow/Authorize" through "authorize"
    When the wrapper is generated for the crate "alpha"
    Then every generation succeeded
    And the snapshot of the crate "alpha" has "authorize" on the Type "Device" serving only "acme.v1.Device/Authorize"
    And the snapshot of the crate "alpha" has "authorize" on the Type "Flow" serving only "acme.v1.Flow/Authorize"

  @state:error
  Scenario: One method name on two Types of one surface is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" exports "authorize" on the Type "Device" of the surface "s-device"
    And the crate "alpha" exports "authorize" on the Type "Flow" of the surface "s-device"
    And the surface "s-device" serves the endpoint "acme.v1.Device/Authorize" through "authorize"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "two client types on surface `s-device` both export `authorize`"

  @state:error
  Scenario: An endpoint a surface names is served only by an export of that surface
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" exports "authorize" on the Type "Flow" of the surface "s-beta"
    And the surface "s-beta" serves the endpoint "acme.v1.Beta/Authorize" through "authorize"
    And the surface "s-alpha" serves the endpoint "acme.v1.Alpha/Authorize" through "authorize"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "surface `s-alpha` lists package `@acme/alpha` and names endpoint `acme.v1.Alpha/Authorize`, which no export of `alpha` serves"

  @state:error
  Scenario: A tracked snapshot of another schema is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" tracks an l2-methods.json of the schema "api-bones.l2-methods/v0"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "snapshot schema is `api-bones.l2-methods/v0`, expected `api-bones.l2-methods/v1`"

  @state:error
  Scenario: A tracked snapshot missing a field is refused
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" tracks an l2-methods.json without the field "kind"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "missing field `kind`"

  @state:error
  Scenario: An output directory that tracks hand-written source is refused, each file named
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the output of the crate "alpha" tracks the hand-written file "api-surface.d.ts"
    And the output of the crate "alpha" tracks the hand-written file "src/index.ts"
    When the wrapper is generated for the crate "alpha"
    Then the generation is refused with "tracks files the generator does not own"
    And the refusal names "api-surface.d.ts"
    And the refusal names "src/index.ts"
    And the refusal does not name "package.json"

  @state:happy
  Scenario: An output directory that tracks a generated snapshot is regenerated
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the output of the crate "alpha" tracks the generated file "api-surface.d.ts"
    When the wrapper is generated for every crate
    Then every generation succeeded
    And the wrapper of the crate "alpha" is the package "@acme/alpha"

  @state:happy
  Scenario: An Apache-2.0 package publishes public and ships its dist directory
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" is licensed "Apache-2.0" and publishes to a registry
    When the wrapper is generated for the crate "alpha"
    Then every generation succeeded
    And the manifest of the crate "alpha" publishes with the access "public"
    And the manifest of the crate "alpha" ships exactly "dist"

  @state:happy
  Scenario Outline: A package under a license that is not OSI-approved publishes restricted
    Given a service with the crate "alpha" published as "@acme/alpha"
    And the crate "alpha" is licensed "<license>" and publishes to a registry
    When the wrapper is generated for the crate "alpha"
    Then every generation succeeded
    And the manifest of the crate "alpha" publishes with the access "restricted"
    And the manifest of the crate "alpha" ships exactly "dist"

    Examples:
      | license                |
      | Elastic-2.0            |
      | LicenseRef-Proprietary |
