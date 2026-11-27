# SPDX-License-Identifier: MIT
@library @sdk-call
Feature: A Layer 2 crate generates the Swift package that fronts its uniffi build

  The Swift wrapper of a Layer 2 crate is the one package a surface lists as
  its swift target: one crate, one package. It copies the uniffi bindings it
  is given as they are and takes no root, pin or verifier. Generation is
  refused when no bindings directory is passed, when the bindings directory
  holds no file, when the bindings lack a function the package calls, when no
  surface lists a swift package, when two packages are listed, when a surface
  that lists the package names an endpoint the crate does not serve, when an
  export belongs to a surface that does not list the package, and when a
  client's options message declares a field that carries a root, a
  certificate, a pin, a verifier, a TLS setting or a trust decision.

  @state:happy
  Scenario: The package of the test service is generated with its client
    Given the test service crate and the SPEC that lists the swift package "ApiBonesTestService"
    And the uniffi bindings of the test service
    When the Swift wrapper is generated
    Then the generation succeeded
    And the package holds the file "Package.swift"
    And the package holds the file "Sources/ApiBonesTestService/ApiBonesClient.swift"
    And the package holds the file "Sources/ApiBonesTestService/Errors.swift"
    And the package holds the file "Sources/ApiBonesTestServiceFFI/api_bones_sdk_ffi.swift"
    And the client of the package takes no root, pin or verifier

  @state:error
  Scenario: A run without bindings is refused
    Given the test service crate and the SPEC that lists the swift package "ApiBonesTestService"
    When the Swift wrapper is generated
    Then the generation is refused with "`wrapper swift` copies the uniffi output: pass `--bindings <dir>`"

  @state:error
  Scenario: An empty bindings directory is refused
    Given the test service crate and the SPEC that lists the swift package "ApiBonesTestService"
    And an empty bindings directory
    When the Swift wrapper is generated
    Then the generation is refused with "holds no file"

  @state:error
  Scenario: Bindings that lack a function the package calls are refused
    Given the test service crate and the SPEC that lists the swift package "ApiBonesTestService"
    And the uniffi bindings of the test service without the function "apiBonesEventsClientWatch"
    When the Swift wrapper is generated
    Then the generation is refused with "func apiBonesEventsClientWatch"

  @state:error
  Scenario: A SPEC that lists no swift package is refused
    Given the test service crate and the SPEC that lists no swift package
    And the uniffi bindings of the test service
    When the Swift wrapper is generated
    Then the generation is refused with "no sdk_surfaces entry lists a swift target package"

  @state:error
  Scenario: Two swift packages are refused
    Given the test service crate and the SPEC that lists the swift packages "ApiBonesTestService, Other"
    And the uniffi bindings of the test service
    When the Swift wrapper is generated
    Then the generation is refused with "one crate is one package"

  @state:error
  Scenario: A surface that lists the package and names an endpoint the crate does not serve is refused
    Given the test service crate and the SPEC that lists the swift package "ApiBonesTestService"
    And the surface names the endpoint "contract.v1.NumbersService/Unserved"
    And the uniffi bindings of the test service
    When the Swift wrapper is generated
    Then the generation is refused with "names endpoint `contract.v1.NumbersService/Unserved`, which no export of `api-bones-test-service-l2` serves"

  @state:error
  Scenario: An export of a surface that does not list the package is refused
    Given the test service crate and the SPEC that lists the swift package "ApiBonesTestService"
    And the SPEC renames the surface "sdk-call" to "other-surface"
    And the uniffi bindings of the test service
    When the Swift wrapper is generated
    Then the generation is refused with "package `ApiBonesTestService` is not a swift target of surface `sdk-call`, which the exports of `api-bones-test-service-l2` name"

  @state:error
  Scenario Outline: An options field that carries a root, pin or verifier is refused
    Given the test service crate and the SPEC that lists the swift package "ApiBonesTestService"
    And the options message of the client declares the field "<field>"
    And the uniffi bindings of the test service
    When the Swift wrapper is generated
    Then the generation is refused with "option `<json>` of"
    And the refusal says "takes no root, pin or verifier"

    Examples:
      | field        | json        |
      | ca_pem       | caPem       |
      | pinned_hash  | pinnedHash  |
      | tls_mode     | tlsMode     |
