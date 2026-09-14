# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect retry eligibility

  api-bones ships this vocabulary twice, once in Rust and once in
  TypeScript. Both implementations answer the scenarios below, so a row
  only one language satisfies is a divergence.

  Scenario Outline: A failure is classified the same way in every language
    Given a Connect failure with code "<code>" and message "<message>"
    Then it is a connection write failure: <write_failure>
    And it is retryable without server instruction: <unprompted>
    And its shape permits a replay: <replayable>
    And it is reported to the caller as "<reported>"

    # Every signature gets a row, so the two hand-transcribed lists cannot
    # drift apart without a build going red.
    #
    # The last two carry what the Node transport records about the connection
    # itself: whether a TLS session had ever been established, and what the
    # peer's verification came back as. A reader needs those to tell a handshake
    # that never finished from a peer that hung up on a finished one -- the two
    # have different owners -- and a classifier that stopped recognising the
    # line because it grew would send that reader nowhere.
    Examples: connection write failures
      | code     | message                                  | write_failure | unprompted | replayable | reported    |
      | internal | write EPIPE (socket=16, freshly connected) | true          | false      | true       | unavailable |
      | internal | write EPIPE (socket=15, freshly connected, TLS handshake had not completed) | true | false | true | unavailable |
      | internal | write EPIPE (socket=15, alpn=http/1.1, peer unverified: self-signed certificate) | true | false | true | unavailable |
      | internal | Broken pipe                              | true          | false      | true       | unavailable |
      | internal | read ECONNRESET                          | true          | false      | true       | unavailable |
      | internal | connection reset by peer                 | true          | false      | true       | unavailable |
      | internal | socket hang up                           | true          | false      | true       | unavailable |
      | internal | http2 stream closed                      | true          | false      | true       | unavailable |
      | internal | received GOAWAY                          | true          | false      | true       | unavailable |

    # A genuine server fault stays Internal: re-coding it would tell the
    # caller to retry a bug. A refusal the server actually sent stays
    # untouched, or a caller cannot tell a refusal from a dead hop.
    Examples: failures that are not the transport
      | code               | message                                       | write_failure | unprompted | replayable | reported           |
      | internal           | nil pointer dereference in handler            | false         | false      | false      | internal           |
      | unauthenticated    | no verified peer                              | false         | false      | false      | unauthenticated    |
      | aborted            | transaction conflict                          | false         | false      | false      | aborted            |
      | resource_exhausted | quota exceeded                                | false         | false      | false      | resource_exhausted |
      | permission_denied  | policy denied: upstream reported write EPIPE  | false         | false      | false      | permission_denied  |

    # Unavailable is the one code that reliably means the connection did
    # not carry the call.
    Examples: the connection did not carry the call
      | code        | message            | write_failure | unprompted | replayable | reported    |
      | unavailable | connection refused | false         | true       | true       | unavailable |
