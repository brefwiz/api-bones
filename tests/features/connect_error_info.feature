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
