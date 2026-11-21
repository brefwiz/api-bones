# SPDX-License-Identifier: MIT
@library @protos @rust-sdk-only
Feature: Error catalog strings match the arguments their code declares

  An error code is a value of the emitter's error enum, and the value declares
  the typed arguments its message takes with `bones.v1.error_args`: a name and
  a kind of string, int64, double or timestamp. The `error-catalog` check reads
  those declarations from the compiled descriptor and refuses a catalog string
  of the code that uses a placeholder naming no declared argument, a
  placeholder whose declared kind it does not accept, or anything outside the
  MessageFormat subset. A string may leave declared arguments unused. A
  placeholder accepts string for `{x}` and `select`, int64 or double for
  `number` and `plural`, and timestamp for `date` and `time`.
  The surface is Rust alone: the declarations are read at build time by
  api-bones-protos and the check ships in api-bones-i18n.

  Background:
    Given the emitter "acme" has an error enum value "SEAT_ERROR_ARCHIVED" declaring no error arguments
    And the emitter "acme" has an error enum value "SEAT_ERROR_LIMIT" declaring the error arguments "limit:int64, plan:string, ratio:double, ends_at:timestamp"

  @state:happy
  Scenario Outline: A string using only declared arguments of an accepted kind passes
    Given the catalog string "acme.errors.SEAT_ERROR_LIMIT.title" is "<text>"
    When the error-catalog check runs over the catalog
    Then the error-catalog check passes

    Examples:
      | text                                                                |
      | {limit, plural, one {# seat left} other {# seats left}}             |
      | {limit, number} of {ratio, number, percent} used on the {plan} plan |
      | {plan, select, team {The team plan} other {This plan}}              |
      | Ends {ends_at, date, long} at {ends_at, time, short}                |
      | {ratio, plural, other {# left}}                                     |
      | No placeholders at all                                              |

  @state:happy
  Scenario: A code declaring no arguments passes with a string that has no placeholder
    Given the catalog string "acme.errors.SEAT_ERROR_ARCHIVED.title" is "This workspace is archived."
    And the catalog string "acme.errors.SEAT_ERROR_ARCHIVED.description" is "Ask an owner to restore it."
    And the catalog string "bones.errors.BAD_REQUEST.title" is "{anything}"
    When the error-catalog check runs over the catalog
    Then the error-catalog check passes

  @state:error
  Scenario Outline: A placeholder that mismatches the declaration by name or kind fails the check
    Given the catalog string "acme.errors.SEAT_ERROR_LIMIT.description" is "<text>"
    When the error-catalog check runs over the catalog
    Then the error-catalog check fails at "acme.errors.SEAT_ERROR_LIMIT.description" with "<problem>"

    Examples: the name is not declared
      | text                  | problem                                         |
      | {lmit, number} left   | placeholder `lmit` is not a declared argument   |

    Examples: the kind is not accepted
      | text                                | problem                                                                        |
      | {limit, date, long}                 | placeholder `limit` needs a Timestamp argument but the code declares it Int64  |
      | {plan, number}                      | placeholder `plan` needs a Number argument but the code declares it String     |
      | {ends_at}                           | placeholder `ends_at` needs a String argument but the code declares it Timestamp |
      | {ratio, select, a {x} other {y}}    | placeholder `ratio` needs a String argument but the code declares it Double    |
      | {plan, plural, one {x} other {y}}   | placeholder `plan` needs a Number argument but the code declares it String     |
      | {limit}                             | placeholder `limit` needs a String argument but the code declares it Int64     |

    Examples: the string is outside the subset
      | text                                          | problem                       |
      | {limit, selectordinal, one {#st} other {#th}} | message pattern not supported |
      | {limit, number, ::currency/EUR}               | message pattern not supported |

  @state:error
  Scenario: A placeholder in the string of a code declaring no arguments fails the check
    Given the catalog string "acme.errors.SEAT_ERROR_ARCHIVED.title" is "{who} archived it"
    When the error-catalog check runs over the catalog
    Then the error-catalog check fails at "acme.errors.SEAT_ERROR_ARCHIVED.title" with "placeholder `who` is not a declared argument"

  @state:error
  Scenario: A string of a code the emitter does not declare fails the check
    Given the catalog string "acme.errors.SEAT_ERROR_LIMT.title" is "Limit reached."
    When the error-catalog check runs over the catalog
    Then the error-catalog check fails at "acme.errors.SEAT_ERROR_LIMT.title" with "the code is not a declared error code"

  @state:error
  Scenario Outline: A declared argument name outside the argument name format or repeated fails the check
    Given the emitter "acme" has an error enum value "SEAT_ERROR_BAD" declaring the error arguments "<arguments>"
    When the error-catalog check runs over the catalog
    Then the error-catalog check fails at "acme.errors.SEAT_ERROR_BAD" with "<problem>"

    Examples:
      | arguments                | problem                                                  |
      | Seat:string              | declared argument name `Seat` is not `^[a-z][a-z0-9_]{0,63}$` |
      | seat:string, seat:int64  | argument `seat` is declared twice                        |
