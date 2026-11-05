# SPDX-License-Identifier: MIT
@library @connect
Feature: Stream keepalive

  A proxy cuts an upstream read that stays idle, so a server stream with
  nothing to send is dropped mid-flight. After 15000 ms of stream silence the
  server sends a keepalive, an ordinary data message whose only content is the
  protobuf field 19999 set to the varint 1. Protobuf reserves the field
  numbers 19000 through 19999, so no real message declares that field and no
  real message can collide with a keepalive.

  The server sends a keepalive only when the request carried the header
  "bones-stream-keepalive" set to "1", and only on a protobuf codec. The
  client sends that header on every server-stream call and drops the
  keepalives it reads, so the reader receives the stream's events and nothing
  else. A message that carries any other content, field 19999 included, is an
  event and is delivered.

  Scenario: The keepalive payload is field 19999 set to the varint 1
    Given the keepalive payload
    When the payload is read as protobuf wire data
    Then the payload is the bytes "F8E10901"
    And the payload holds one field, number 19999, as the varint 1

  Scenario Outline: A payload is a keepalive only when it is exactly the keepalive payload
    Given the payload bytes "<bytes>"
    When the payload is checked for a keepalive
    Then the payload is a keepalive: <verdict>

    Examples:
      | bytes        | verdict |
      | F8E10901     | true    |
      |              | false   |
      | F8E109       | false   |
      | F8E10902     | false   |
      | F8E10900     | false   |
      | F8E1090100   | false   |
      | 0A02652D31   | false   |

  Scenario: A server stream silent past the interval delivers no extra message to the reader
    Given a server stream that delivers the event "e-1", stays silent for 15000 ms and delivers the event "e-2"
    When the reader reads the stream
    Then the server sent 1 keepalive while the stream was silent
    And the reader received the events "e-1,e-2" and nothing else

  Scenario: A longer silence sends one keepalive per interval and the reader still sees none
    Given a server stream that delivers the event "e-1", stays silent for 45000 ms and delivers the event "e-2"
    When the reader reads the stream
    Then the server sent 3 keepalives while the stream was silent
    And the reader received the events "e-1,e-2" and nothing else

  Scenario: A stream that is never silent for the interval carries no keepalive
    Given a server stream that delivers the event "e-1", stays silent for 14999 ms and delivers the event "e-2"
    When the reader reads the stream
    Then the server sent 0 keepalives while the stream was silent
    And the reader received the events "e-1,e-2" and nothing else

  Scenario: A message that carries other content next to field 19999 is delivered
    Given a server stream that delivers the event "e-1" also carrying field 19999 as the varint 1
    When the reader reads the stream
    Then the reader received the events "e-1" and nothing else

  Scenario: A failure on the stream reaches the reader after the keepalives are dropped
    Given a server stream that delivers the event "e-1", stays silent for 15000 ms and fails with "closed"
    When the reader reads the stream
    Then the reader received the events "e-1" and nothing else
    And the reader read the failure "closed"

  Scenario: A server-stream request carries the opt-in header
    When the reader prepares a server-stream request
    Then the request carries the header "bones-stream-keepalive" set to "1"
