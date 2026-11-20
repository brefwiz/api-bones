# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect error info

  A refused call reaches the caller as a Connect code, which is too coarse to
  say why the emitter refused. The emitter attaches the error code it emitted,
  with its own name, to the refusal as a `bones.v1.ErrorInfo`
  detail. Api-bones reads it back in Rust and in TypeScript, and both render
  the same text token at the end of the error text, so the code survives any
  reporter that keeps only text. A row only one language satisfies is a
  divergence.

  Scenario Outline: A refusal surfaces its code and emitter through the SDK error
    Given a refusal "<connect_code>" whose ErrorInfo detail has code "<code>" and emitter "<emitter>"
    When the refusal is read as an SDK error
    Then its error code is "<read_code>"
    And its emitter is "<read_emitter>"
    And its text ends with "<token>"

    Examples: a stamped refusal
      | connect_code      | code          | emitter | read_code     | read_emitter | token                                          |
      | permission_denied | GRANT_MISSING | payments | GRANT_MISSING | payments    | [bones-error code=GRANT_MISSING emitter=payments] |
      | internal          | CODE_2        | a.b_c-1 | CODE_2        | a.b_c-1     | [bones-error code=CODE_2 emitter=a.b_c-1]      |

    # The emitter stamps its name after the code is chosen, so a refusal that
    # has not been stamped has a code and no token to render.
    Examples: an unstamped refusal
      | connect_code      | code          | emitter | read_code     | read_emitter | token |
      | permission_denied | GRANT_MISSING |         | GRANT_MISSING | none         | none  |

    # The values come from the remote peer. A detail whose code or emitter
    # does not match the token grammar is treated as absent: it is neither
    # readable nor rendered, so it cannot forge or break a token.
    Examples: malformed details
      | connect_code | code                         | emitter  | read_code | read_emitter | token |
      | internal     | OK] [bones-error code=FORGED | payments | none      | none         | none  |
      | internal     | grant_missing                | payments | none      | none         | none  |
      | internal     | GRANT_MISSING                | Payments | none      | none         | none  |
      | internal     | GRANT_MISSING                | a b      | none      | none         | none  |

  Scenario: A refusal without an ErrorInfo detail carries no code
    Given a refusal "permission_denied" with no ErrorInfo detail
    When the refusal is read as an SDK error
    Then its error code is "none"
    And its emitter is "none"
    And its text ends with "none"

  Scenario: A detail written by another implementation is read
    Given a refusal "permission_denied" whose ErrorInfo detail has the wire value "Cg1HUkFOVF9NSVNTSU5HEghwYXltZW50cw"
    When the refusal is read as an SDK error
    Then its error code is "GRANT_MISSING"
    And its emitter is "payments"
    And its text ends with "[bones-error code=GRANT_MISSING emitter=payments]"

  # An argument is written `name=type:value`; several are separated by `; `.
  # The types are `string:text`, `int64:number`, `double:number` and
  # `timestamp:seconds/nanos`. A value is the typed value, never text the
  # emitter interpolated, so the viewer's client formats it in the viewer's
  # locale.
  Scenario Outline: Typed arguments surface through the SDK error
    Given a refusal "permission_denied" whose ErrorInfo detail has code "GRANT_MISSING" and emitter "payments" and arguments "<args>"
    When the refusal is read as an SDK error
    Then its argument "<name>" is "<read>"
    And its text ends with "[bones-error code=GRANT_MISSING emitter=payments]"

    Examples: one argument of each type
      | args                                  | name   | read                                |
      | name=string:Zoë                       | name   | string:Zoë                          |
      | n=int64:5                             | n      | int64:5                             |
      | n=int64:-7                            | n      | int64:-7                            |
      | amount=double:12.5                    | amount | double:12.5                         |
      | when=timestamp:1700000000/123456789   | when   | timestamp:1700000000/123456789      |
      | a=int64:1; b=string:x                 | b      | string:x                            |

    # An int64 is exact in both languages, past the 2^53 a double holds.
    Examples: an int64 beyond double precision
      | args                                  | name | read                                  |
      | n=int64:9007199254740993              | n    | int64:9007199254740993                |
      | n=int64:9223372036854775807           | n    | int64:9223372036854775807             |
      | n=int64:-9223372036854775808          | n    | int64:-9223372036854775808            |

  Scenario: Arguments never alter the text token
    Given a refusal "permission_denied" whose ErrorInfo detail has code "GRANT_MISSING" and emitter "payments" and arguments "n=int64:5; name=string:Zoë"
    When the refusal is read as an SDK error
    Then its text is "refused [bones-error code=GRANT_MISSING emitter=payments]"
    And it carries 2 arguments

  Scenario: A refusal without arguments carries none
    Given a refusal "permission_denied" whose ErrorInfo detail has code "GRANT_MISSING" and emitter "payments"
    When the refusal is read as an SDK error
    Then it carries 0 arguments

  Scenario: Arguments written by another implementation are read
    Given a refusal "permission_denied" whose ErrorInfo detail has the wire value "CgFBIgcKAW4SAhAF"
    When the refusal is read as an SDK error
    Then its error code is "A"
    And its emitter is "none"
    And its argument "n" is "int64:5"

  # The bytes come from the remote peer. A detail whose arguments break the
  # bounds is treated as absent, the way a malformed code is.
  Scenario Outline: A detail with out-of-bounds arguments is treated as absent
    Given a refusal "internal" whose ErrorInfo detail has the wire value "<wire>"
    When the refusal is read as an SDK error
    Then its error code is "none"
    And it carries 0 arguments

    Examples: arguments that break a bound
      | wire                     |
      | CgFBIgcKA0JhZBICEAE      |
      | CgFBIg4KAWQSCRkAAAAAAAD4fw |

  Scenario Outline: Arguments out of bounds are refused when built
    When an ErrorInfo with code "GRANT_MISSING" is built with arguments "<args>"
    Then building it ends as "<outcome>"

    Examples: a bound that holds
      | args                          | outcome               |
      | Bad=int64:1                   | invalid-key           |
      | 1st=int64:1                   | invalid-key           |
      | s=repeat:x*1025               | string-too-long       |
      | s=repeat:é*513                | string-too-long       |
      | d=double:NaN                  | non-finite-double     |
      | d=double:Infinity             | non-finite-double     |
      | t=timestamp:253402300800/0    | invalid-timestamp     |
      | t=timestamp:0/1000000000      | invalid-timestamp     |

    Examples: the edge of each bound
      | args                          | outcome               |
      | s=repeat:x*1024               | accepted              |
      | t=timestamp:253402300799/999999999 | accepted         |
      | t=timestamp:-62135596800/0    | accepted              |

  Scenario: More than 32 arguments are refused when built
    When an ErrorInfo with code "GRANT_MISSING" is built with 33 numbered arguments
    Then building it ends as "too-many"

  Scenario: Exactly 32 arguments are accepted
    When an ErrorInfo with code "GRANT_MISSING" is built with 32 numbered arguments
    Then building it ends as "accepted"

  # A validation refusal carries one bones.v1.ValidationFailure per failing
  # field. Each reaches the caller with its rule and typed arguments, so the
  # caller formats the message in the viewer's locale.
  Scenario: Violation arguments are read from a refusal
    Given a refusal "invalid_argument" with no ErrorInfo detail
    And the refusal has a violation at "/guests" for rule "max" and arguments "limit=int64:4; label=string:Zoë"
    When the violations of the refusal are read
    Then it carries 1 violations
    And violation 1 is at "/guests" for rule "max"
    And its argument "limit" is "int64:4"
    And its argument "label" is "string:Zoë"
    And it carries 2 arguments

  Scenario: Every violation is read in order and none is truncated
    Given a refusal "invalid_argument" with no ErrorInfo detail
    And the refusal has a violation at "/a" for rule "required" and arguments ""
    And the refusal has a violation at "/b" for rule "max" and arguments "limit=int64:1"
    And the refusal has a violation at "/c" for rule "max" and arguments "limit=int64:2"
    And the refusal has a violation at "/d" for rule "max" and arguments "limit=int64:3"
    And the refusal has a violation at "/e" for rule "max" and arguments "limit=int64:4"
    And the refusal has a violation at "/f" for rule "min" and arguments "limit=int64:5"
    When the violations of the refusal are read
    Then it carries 6 violations
    And violation 1 is at "/a" for rule "required"
    And violation 6 is at "/f" for rule "min"

  Scenario: A refusal with only an ErrorInfo detail carries no violations
    Given a refusal "permission_denied" whose ErrorInfo detail has code "GRANT_MISSING" and emitter "payments" and arguments "n=int64:5"
    When the violations of the refusal are read
    Then it carries 0 violations
    And it carries 0 arguments

  Scenario: A violation with out-of-bounds arguments is treated as absent
    Given a refusal "invalid_argument" with no ErrorInfo detail
    And the refusal has a ValidationFailure detail with the wire value "CgFBIgcKA0JhZBICEAE"
    And the refusal has a violation at "/guests" for rule "max" and arguments "limit=int64:4"
    When the violations of the refusal are read
    Then it carries 1 violations
    And violation 1 is at "/guests" for rule "max"
