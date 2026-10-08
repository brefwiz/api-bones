# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect wire code

  Connect maps several codes onto one HTTP status, so a caller that must tell
  `failed_precondition` from `invalid_argument` reads the code the peer put on
  the wire. The code is its own type, independent of the transport crate's, and
  an unrecognized code is never guessed at.

  Scenario Outline: A Connect error body names its code
    Given a Connect error body <body>
    When the code is read from the body
    Then the code is "<code>"

    Examples: recognized codes
      | body                                              | code                |
      | {"code":"failed_precondition","message":"stale"}  | failed_precondition |
      | {"code":"invalid_argument","message":"bad"}       | invalid_argument    |
      | {"code":"unauthenticated"}                        | unauthenticated     |

    Examples: no usable code
      | body                              | code |
      | {"message":"no code"}             | none |
      | {"code":"bogus","message":"x"}    | none |
      | {"code":7}                        | none |
      | not json                          | none |

  Scenario Outline: A code round-trips its wire string and the transport code
    Given the wire code "<wire>"
    When the code is converted to the transport code and back
    Then the code is "<wire>"

    Examples: every protocol code
      | wire                |
      | canceled            |
      | unknown             |
      | invalid_argument    |
      | deadline_exceeded   |
      | not_found           |
      | already_exists      |
      | permission_denied   |
      | resource_exhausted  |
      | failed_precondition |
      | aborted             |
      | out_of_range        |
      | unimplemented       |
      | internal            |
      | unavailable         |
      | data_loss           |
      | unauthenticated     |
