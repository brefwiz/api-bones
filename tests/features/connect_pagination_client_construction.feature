# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: Platform paginated client construction

  A TypeScript SDK refuses a generated List method whose discovery document
  omits its paging contract. Platform composition exposes batching and page
  iteration together, keeping remembered pages private to each client.

  Scenario Outline: Missing paging discovery refuses construction before sending
    Given the generated pagination method is "<method>"
    When the SDK constructs a paginated client without that method's paging entry
    Then pagination construction fails with a typed error naming "<method>"
    And the server received no request

    Examples:
      | method      |
      | ListEntries |
      | StreamEntries |

  Scenario: Platform composition exposes batching and paging with one cache per client
    Given the organization "acme" has the widget "w1" with parts "p1,p2,p3"
    And the organization "acme" has the widget "w2" with parts "p4"
    And the caller is "ada" of organization "acme"
    When the caller constructs two composed platform clients from discovery
    Then the composed client batch-gets widgets "w1,w2"
    And the composed client replays its retained list pages
    And the second composed client fetches its own list pages
