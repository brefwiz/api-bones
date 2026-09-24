# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: Connect public reads reach the public lane

  A method whose generated policy declares publicRead is served by the
  product BFF on its public lane, beside the session mount, to callers with
  no session at all. The browser transport's webapp profile sends it there
  without being told to, carrying nothing that identifies the caller.

  Scenario Outline: The public lane sits beside its session mount
    Given a product served at "<base>"
    Then its public lane is "<lane>"

    Examples:
      | base                              | lane                                     |
      | https://app.example.com/itinerwiz  | https://app.example.com/public/itinerwiz |
      | https://app.example.com/itinerwiz/ | https://app.example.com/public/itinerwiz |
      | /itinerwiz                         | /public/itinerwiz                        |

  Scenario: A public read goes to the lane as an anonymous GET
    Given a webapp transport for "https://app.example.com/itinerwiz" holding a session and a bearer
    When it calls the public read "GetWeek"
    Then the request is a GET to "https://app.example.com/public/itinerwiz/pkg.v1.PublicService/GetWeek"
    And the request sends no credentials
    And the request carries no "authorization", "x-csrf-token" or product header

  Scenario: Any other method keeps the session path
    Given a webapp transport for "https://app.example.com/itinerwiz" holding a session and a bearer
    When it calls the ordinary method "Book"
    Then the request is a POST to "https://app.example.com/itinerwiz/pkg.v1.PublicService/Book"
    And the request sends the session

  Scenario: A public read the policy cannot trust never reaches the lane
    Given a webapp transport for "https://app.example.com/itinerwiz" whose public read declares no tenant
    When it calls the public read "GetWeek"
    Then the request is a POST to "https://app.example.com/itinerwiz/pkg.v1.PublicService/GetWeek"
