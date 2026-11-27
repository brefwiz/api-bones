# SPDX-License-Identifier: MIT
@library @sdk-call @ts-sdk-only
Feature: The generated SDK records the wasm module it loads

  The TypeScript wrapper of a service loads its Layer 2 core as a wasm module.
  When the run collects evidence, the module is checked against its build stamp
  before it is instantiated; a module that differs is refused and records
  nothing. The load records the module's provenance once. A native Rust client
  compiles together with its core, so none of this can differ there.
  The functions of the package that need no client, a static export and the
  reading of a constant, run on the loaded module, so a caller that has not
  awaited the SDK's init is refused with a typed error that says to await it.

  @state:happy
  Scenario: Loading the SDK records the provenance
    Given the run collects SDK evidence in the run's evidence directory
    And the SDK caller is "ada" of organization "acme"
    When the caller opens the SDK
    Then one SDK provenance line was recorded with the schema "api-bones.sdk-provenance/v1"
    And its wasm digest is the SHA-256 of the installed wasm module
    And its wasm digest is the one the SDK reports as its build info

  @state:error
  Scenario: A static function called before the SDK is initialised is refused
    When a fresh process calls the SDK's clamp function before it awaits the SDK's init
    Then that call is refused as the SDK not initialised
    When the caller calls the SDK's clamp function after it awaits the SDK's init
    Then the clamp answers 5
