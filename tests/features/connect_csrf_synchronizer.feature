# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: The browser transport echoes the BFF's CSRF synchronizer

  A product BFF protects its session with a synchronizer token: it hands one
  out on the answer that signs the caller in and on every answer under that
  session, and refuses an unsafe call that does not echo the current one. The
  browser transport keeps the latest token it was given and sends it back on
  every call, so a page never has to be told the token.

  Scenario: The token handed out on sign-in is sent on the next call
    Given a webapp transport for "https://app.example.com/itinerwiz"
    When it signs in and the BFF answers with the token "first"
    And it calls the session method "Book"
    Then the "Book" call carried the token "first"

  Scenario: A call refused for a stale token is retried once with the fresh one
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "stale"
    When the BFF refuses the next call and hands out the token "fresh"
    And it calls the session method "Book"
    Then the call was sent twice, with "stale" and then "fresh"
    And the call succeeded

  Scenario: A call refused with the token it sent is not retried
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "current"
    When the BFF refuses the next call and hands out the token "current"
    And it calls the session method "Book"
    Then the call was sent once
    And the call was refused

  Scenario: A second transport on the same origin reuses the token
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "shared"
    When a second transport for "https://app.example.com/itinerwiz" calls the session method "Book"
    Then the "Book" call carried the token "shared"

  Scenario: A public read carries no token
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "private"
    When it reads the public method "GetWeek"
    Then the "GetWeek" call carried no token

  Scenario: A call refused again after its one retry is sent exactly twice
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "stale"
    When the BFF refuses the next call and hands out the token "fresh"
    And the BFF refuses the next call and hands out the token "newer"
    And it calls the session method "Book"
    Then the call was sent twice, with "stale" and then "fresh"
    And the call was refused

  Scenario: A refused server stream is not retried
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "stale"
    When the BFF refuses the next call and hands out the token "fresh"
    And it opens the server stream "Watch"
    Then the call was sent once
    And the call was refused

  Scenario: A transport on another origin does not receive the token
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "private"
    When a second transport for "https://other.example.com/itinerwiz" calls the session method "Book"
    Then the "Book" call carried no token

  Scenario: A failure other than a CSRF refusal is not retried even with a new token
    Given a webapp transport for "https://app.example.com/itinerwiz"
    And it signed in and holds the token "old"
    When the BFF fails the next call as unauthenticated and hands out the token "new"
    And it calls the session method "Book"
    Then the call was sent once
    And the call failed as Unauthenticated
