# SPDX-License-Identifier: MIT
@library @sdk-call
Feature: The native binding layer speaks bytes a host can carry

  The library layer under the generated Kotlin, Swift and Python bindings
  exchanges bytes with its host: an error crosses as `CallError` bytes, the
  message for a person comes back as `FormatResult` bytes, and a call that
  delivers over time is read through a handle the host can cancel. The message
  is the catalog's text in the viewer's language, never the error's diagnostic
  message. Bytes that are no error, and a time zone that does not exist, are
  refused with a rule instead of a guess.

  The address a client is built for is checked on the address alone, before
  any credential is asked: `https` to any host, `http` to the loopback only,
  and never an address that carries userinfo. A host that cannot answer its
  credential, because it fails, throws or panics, fails the call closed with
  one fixed refusal that never repeats the host's own text. A connection the
  transport cannot open fails as unavailable without naming the address it
  dialed, so a message a host shows a person never discloses where the client
  was pointed.

  @state:happy
  Scenario Outline: An error is formatted as the catalog text of the viewer's locale
    Given a call that failed because the SDK versions do not match
    When the error is formatted for locale "<locale>"
    Then the text reads "<text>"
    And the text does not repeat the diagnostic message

    Examples: locales
      | locale | text                                     |
      | en     | SDK versions do not match                |
      | fr     | Les versions du SDK ne correspondent pas |

  @state:happy
  Scenario: A message naming an instant reads it in UTC without a time zone
    Given a call that failed with a message naming the instant 0 seconds after the epoch
    When the error is formatted for locale "en"
    Then the text reads "12:00:00"

  @state:happy
  Scenario: A known time zone moves the instant a message names
    Given a call that failed with a message naming the instant 0 seconds after the epoch
    When the error is formatted for locale "en" in the time zone "Asia/Tokyo"
    Then the text reads "9:00:00"
    And the text does not read "12:00:00"

  @state:error
  Scenario: Bytes that are no call error are refused
    Given bytes that are no call error
    When the error is formatted for locale "en"
    Then the result is refused with the rule "malformed_error"

  @state:error
  Scenario: A time zone that does not exist is refused
    Given a call that failed because the SDK versions do not match
    When the error is formatted for locale "en" in the time zone "Mars/Base"
    Then the result is refused with the rule "unknown_time_zone"

  @state:happy
  Scenario: An error crosses the boundary as bytes and reads back unchanged
    Given a call that failed because the SDK versions do not match
    When the error crosses the binding boundary
    Then the bytes read back as the same error with its three versions

  @state:happy
  Scenario: A cancelled handle ends its stream however often it is cancelled
    Given a handle over two items
    When the host reads one item and cancels the handle twice
    Then the next read ends the stream

  @state:happy
  Scenario Outline: A call carries the trace its host named and no other
    Given a host context naming the trace "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01" and the state "vendor=state"
    When the host context is read for the <call> call
    Then the call runs under the trace "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01" and the state "vendor=state"

    Examples: calls
      | call   |
      | list   |
      | stream |
      | unary  |

  @state:happy
  Scenario: A list takes the reads its host chose
    Given a host context asking to fetch 0 pages ahead and to read the first page only
    When the host context is read for the list call
    Then the call fetches 0 pages ahead and reads the first page only

  @state:happy
  Scenario Outline: A call that is no list leaves the reads to the SDK
    Given a host context asking to fetch 0 pages ahead and to read the first page only
    When the host context is read for the <call> call
    Then the call leaves the reads to the SDK

    Examples: calls
      | call   |
      | stream |
      | batch  |
      | unary  |

  @state:happy
  Scenario: A host that names nothing starts a new trace
    Given a host context that names nothing
    When the host context is read for the list call
    Then the call starts a new trace

  @state:happy
  Scenario Outline: An address the binding serves is accepted
    When the base URL "<url>" is checked
    Then the base URL is accepted

    Examples: addresses
      | url                            |
      | https://api.example.com        |
      | https://api.example.com:8443/b |
      | http://127.0.0.1:8080          |
      | http://localhost:8080          |
      | http://[::1]:8080              |

  @state:error
  Scenario Outline: An address that would send a credential in the clear or carry userinfo is refused
    When the base URL "<url>" is checked
    Then the base URL is refused as invalid_argument with "<reason>"

    Examples: addresses
      | url                              | reason                                           |
      | http://api.example.com           | base_url must be https unless it names the loopback |
      | http://10.0.0.1:8080             | base_url must be https unless it names the loopback |
      | https://user:pw@api.example.com  | base_url must not carry userinfo                 |
      | ftp://api.example.com            | base_url must be http or https                   |
      | not a uri                        | base_url is not a URI                            |
      | /relative                        | base_url names no host                           |

  @state:error
  Scenario Outline: A host that cannot answer its credential fails the call closed with a fixed refusal
    Given a host whose credential read <failure> with the text "sentinel-foreign-text"
    When a call is made through the env of that host
    Then the call is refused as "unavailable" before anything is sent
    And the refusal does not repeat "sentinel-foreign-text"

    Examples: failures
      | failure |
      | fails   |
      | panics  |

  @state:error
  Scenario: A connection that cannot be opened fails without naming the address
    Given an address on the loopback where nothing listens
    When a call is sent to that address through the binding transport
    Then the call fails as "unavailable"
    And the failure does not name the address
