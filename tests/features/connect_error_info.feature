# SPDX-License-Identifier: MIT
@library @connect
Feature: Connect error info

  A refused call reaches the caller as a Connect code, which is too coarse to
  say why the emitter refused. The emitter attaches the error code it emitted,
  with its own name and version, to the refusal as a `bones.v1.ErrorInfo`
  detail. Api-bones reads it back in Rust and in TypeScript, and both render
  the same text token at the end of the error text, so the code survives any
  reporter that keeps only text. A row only one language satisfies is a
  divergence.

  Scenario Outline: A refusal surfaces its code and emitter through the SDK error
    Given a refusal "<connect_code>" whose ErrorInfo detail has code "<code>", emitter "<emitter>" and version "<version>"
    When the refusal is read as an SDK error
    Then its error code is "<read_code>"
    And its emitter is "<read_emitter>"
    And its text ends with "<token>"

    Examples: a stamped refusal
      | connect_code      | code          | emitter  | version | read_code     | read_emitter | token                                                   |
      | permission_denied | GRANT_MISSING | payments | 1.4.2   | GRANT_MISSING | payments     | [bones-error code=GRANT_MISSING emitter=payments@1.4.2] |
      | internal          | CODE_2        | a.b_c-1  | 1.0.0-rc.1+build | CODE_2 | a.b_c-1   | [bones-error code=CODE_2 emitter=a.b_c-1@1.0.0-rc.1+build] |

    # The emitter stamps its name and version after the code is chosen, so a
    # refusal that has not been stamped has a code and no token to render.
    Examples: an unstamped refusal
      | connect_code      | code          | emitter  | version | read_code     | read_emitter | token |
      | permission_denied | GRANT_MISSING |          |         | GRANT_MISSING | none         | none  |

    # The values come from the remote peer. One that does not match the token
    # grammar is still readable, but never reaches the error text, where it
    # could forge or break a token.
    Examples: values outside the token grammar
      | connect_code      | code                         | emitter  | version | read_code                    | read_emitter | token |
      | internal          | OK] [bones-error code=FORGED | payments | 1.4.2   | OK] [bones-error code=FORGED | payments     | none  |
      | internal          | grant_missing                | payments | 1.4.2   | grant_missing                | payments     | none  |
      | internal          | GRANT_MISSING                | Payments | 1.4.2   | GRANT_MISSING                | Payments     | none  |
      | internal          | GRANT_MISSING                | payments | 1 2     | GRANT_MISSING                | payments     | none  |

  Scenario: A refusal without an ErrorInfo detail carries no code
    Given a refusal "permission_denied" with no ErrorInfo detail
    When the refusal is read as an SDK error
    Then its error code is "none"
    And its emitter is "none"
    And its text ends with "none"

  Scenario: A detail written by another implementation is read
    Given a refusal "permission_denied" whose ErrorInfo detail has the wire value "Cg1HUkFOVF9NSVNTSU5HEghwYXltZW50cxoFMS40LjI"
    When the refusal is read as an SDK error
    Then its error code is "GRANT_MISSING"
    And its emitter is "payments"
    And its text ends with "[bones-error code=GRANT_MISSING emitter=payments@1.4.2]"
