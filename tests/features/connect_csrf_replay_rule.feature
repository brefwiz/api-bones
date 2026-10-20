# SPDX-License-Identifier: MIT
@library @connect
Feature: The CSRF synchronizer replay rule

  A session BFF refuses a state-changing call that does not echo its current
  synchronizer token and hands the fresh one back. api-bones exports the one
  rule that decides whether such a refusal is replayed, in Rust and in
  TypeScript, so no caller restates it. Both implementations answer the rows
  below.

  Scenario Outline: A refusal is replayed only with a different token
    Given a refusal with status <status> carrying the token "<refusal>" for a call that sent "<sent>"
    Then the call is replayed with "<replay>"

    Examples: replayed
      | status | refusal | sent  | replay |
      | 403    | fresh   | stale | fresh  |
      | 403    | fresh   | none  | fresh  |

    Examples: not replayed
      | status | refusal | sent  | replay |
      | 403    | same    | same  | none   |
      | 403    | none    | stale | none   |
      | 401    | fresh   | stale | none   |
      | 200    | fresh   | stale | none   |
