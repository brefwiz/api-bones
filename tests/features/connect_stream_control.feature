# SPDX-License-Identifier: MIT
@library @protos @rust-sdk-only
Feature: Stream control

  A long-lived server stream is replaced before it is dropped. The server
  signals that in-band with a control frame, a message that sets only the
  stream-control field 2047 of the stream's response message, either a drain
  notice or a checkpoint. The frame reads back from the shipped proto, in the
  proto codec and in the JSON codec, and a frame whose control cannot be read
  is refused rather than ignored. The request header a client opts in with
  joins this contract when the rotation state machine sends it.

  Scenario Outline: A control-only frame in the proto codec decodes as field 2047 of the shipped message
    Given a contract whose stream response carries the stream control
    And <control>
    When the stream control is framed in the "proto" codec
    Then the frame decodes against the shipped proto as <decoded>
    And the proto frame sets field 2047 of the shipped message

    Examples:
      | control | decoded |
      | a drain notice cutting in 30000 ms | a drain cutting in 30000 ms |
      | a checkpoint for domain "revocations" live from 41 delivered through 40 | a checkpoint for domain "revocations" live from 41 delivered through 40 |

  Scenario Outline: A control-only frame in the JSON codec carries the control under bonesControl
    Given a contract whose stream response carries the stream control
    And <control>
    When the stream control is framed in the "json" codec
    Then the JSON frame holds the control under "bonesControl" and nothing else
    And the frame decodes against the shipped proto as <decoded>

    Examples:
      | control | decoded |
      | a drain notice cutting in 30000 ms | a drain cutting in 30000 ms |
      | a checkpoint for domain "revocations" live from 41 delivered through 40 | a checkpoint for domain "revocations" live from 41 delivered through 40 |

  Scenario Outline: A framed control keeps the message's ordinary field readable
    Given a contract whose stream response carries the stream control
    And a drain notice cutting in 5000 ms
    And the response also sets its ordinary field "id" to "e-1"
    When the stream control is framed in the "<codec>" codec
    Then the frame decodes against the shipped proto as a drain cutting in 5000 ms
    And the same message reads its ordinary field "id" back as "e-1"

    Examples:
      | codec |
      | proto |
      | json  |

  Scenario Outline: A frame the shipped proto wrote is read as its control
    Given a contract whose stream response carries the stream control
    And <control>
    And the shipped proto wrote that message as a "<codec>" frame
    When the frame is read by the stream control codec
    Then the stream control read back is the one declared

    Examples:
      | control | codec |
      | a drain notice cutting in 30000 ms | proto |
      | a drain notice cutting in 30000 ms | json |
      | a checkpoint for domain "revocations" live from 41 delivered through 40 | proto |
      | a checkpoint for domain "revocations" live from 41 delivered through 40 | json |

  Scenario Outline: A control is read from a frame the shipped proto wrote with its ordinary field
    Given a contract whose stream response carries the stream control
    And a drain notice cutting in 5000 ms
    And the response also sets its ordinary field "id" to "e-1"
    And the shipped proto wrote that message as a "<codec>" frame
    When the frame is read by the stream control codec
    Then the stream control read back is the one declared

    Examples:
      | codec |
      | proto |
      | json  |

  Scenario Outline: A frame whose control cannot be read is refused
    Given a frame with "<kind>"
    When the frame is read by the stream control codec
    Then the frame is refused as a malformed control

    Examples:
      | kind                               |
      | a truncated control                |
      | a control that sets no signal      |
      | the control field as a varint      |
      | a JSON control that sets no signal |

  Scenario Outline: A message without a control reads back as no control
    Given a frame with "<kind>"
    When the frame is read by the stream control codec
    Then the codec reads no control

    Examples:
      | kind                                       |
      | an ordinary message without a control      |
      | an ordinary JSON message without a control |
      | a JSON control that is null                |
