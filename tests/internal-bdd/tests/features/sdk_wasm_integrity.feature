# SPDX-License-Identifier: MIT
@library @sdk-call
Feature: The generated SDK refuses a wasm module or wrapper that is not the one it was built with

  The TypeScript wrapper of a service loads its Layer 2 core as a wasm module
  and stamps every client it builds with the boundary version it speaks. The
  module is checked against the digest stamped when the package was built
  before it is instantiated, when the run collects evidence: a module that
  differs is refused and leaves no evidence behind. A core built for another boundary version than the
  wrapper's refuses to build a client. Both faults change the built artifact,
  so no SDK call can produce them: the proof alters a copy of the built
  package and opens it.

  @state:error
  Scenario: A wasm module that differs from its build stamp is refused at load
    Given the SDK caller is "ada" of organization "acme"
    And the run collects SDK evidence in a fresh directory
    When the caller opens an SDK whose wasm module has one byte flipped
    Then the refusal says "not the stamped"
    And no SDK provenance was recorded

  @state:error
  Scenario: An SDK whose wrapper speaks another boundary version is refused with SDK_BOUNDARY_MISMATCH
    Given the SDK caller is "ada" of organization "acme"
    When the caller opens an SDK whose wrapper speaks boundary version 2
    Then the call is refused with the code "SDK_BOUNDARY_MISMATCH"
    And the refusal names the core, wrapper and runtime versions
