# SPDX-License-Identifier: MIT
@library @protos @rust-sdk-only
Feature: Delegation templates a provider ships

  A provider declares the delegation types it ships as templates in its own
  contract, with `bones.v1.delegation_templates`, the way it declares default
  roles. Each template has a lower_snake_case name unique within the service, a
  label, and grants in the one grammar a delegation type composes
  per provider: a scope or capability group the provider declares, selectors
  over the dimensions it declares in `bones.v1.attributes`, an approval mode
  and a budget. A selector's match kind is its dimension's, so a value is a
  literal, never a wildcard, glob or free text. Validating the provider's
  descriptor refuses a template naming anything the provider does not declare.
  The surface is Rust alone: the check ships in api-bones-protos.

  Background:
    Given a provider with an RPC requiring the scope "departure:confirm"
    And a provider with an RPC requiring the scope "departure:read"
    And the provider provides the capability "planning"
    And the provider declares the group "planning" conferring the scope "departure:read"
    And the provider declares the dimension "departure_group" matched by segment prefix on "/"
    And the provider declares the dimension "channel" matched exactly
    And the provider declares the dimension "departure_id" matched exactly over UUIDs

  @state:happy
  Scenario: A declared template reads back from the compiled contract
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "departure_group" values: "summer/alps" values: "summer/dolomites" } approval_mode: APPROVAL_MODE_ASK budget: { count: 20 window_seconds: 86400 }'
    When the provider contract is validated
    Then the service declares 1 delegation template
    And the template "departure_assistant" reads back labelled "Departure assistant"
    And its grant 0 reads back the scope "departure:confirm"
    And its grant 0 reads back the selector "departure_group" with the values "summer/alps, summer/dolomites"
    And its grant 0 reads back the approval mode "APPROVAL_MODE_ASK"
    And its grant 0 reads back the budget 20 per 86400 seconds
    And the delegation templates are accepted

  @state:happy
  Scenario: A grant on a capability group the provider declares is accepted
    Given the provider ships the template "planner" labelled "Planner" with the grant 'group: "planning" approval_mode: APPROVAL_MODE_PRE_APPROVED'
    When the provider contract is validated
    Then its grant 0 reads back the group "planning"
    And the delegation templates are accepted

  @state:error
  Scenario: A grant on a scope the provider does not declare is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:delete" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the scope `departure:delete` is not declared by the service"

  @state:error
  Scenario: A scope required only by a public method is not the service's to grant
    Given a provider with a public RPC listing the scope "departure:browse"
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:browse" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the scope `departure:browse` is not declared by the service"

  @state:error
  Scenario: A grant on a capability group the provider does not declare is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'group: "billing" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the group `billing` is not declared by the service"

  @state:error
  Scenario: A selector on a dimension the provider does not declare is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "region" values: "eu" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the dimension `region` is not declared by the service"

  @state:error
  Scenario Outline: A selector value that is a glob is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "<dimension>" values: "<value>" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the value `<value>` on `<dimension>` carries a wildcard character"

    Examples:
      | dimension       | value         |
      | departure_group | summer/*      |
      | departure_group | summer/>      |
      | channel         | ops-?         |
      | channel         | *             |

  @state:error
  Scenario Outline: A selector value that is not a literal of its dimension is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "<dimension>" values: "<value>" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the value `<value>` is not valid on `<dimension>`"

    Examples:
      | dimension       | value         |
      | departure_group | summer//alps  |
      | departure_group | /summer       |
      | departure_id    | not-a-uuid    |

  @state:error
  Scenario: A selector value with no text is refused on an exact dimension
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "channel" values: "" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the value `` is not valid on `channel`"

  @state:error
  Scenario: A selector with no value is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "channel" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the selector on `channel` carries no value"

  @state:error
  Scenario: Two selectors on one dimension are refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "channel" values: "ops" } selectors: { dimension: "channel" values: "dev" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "two selectors name the dimension `channel`"

  @state:error
  Scenario: A grant with no approval mode is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm"'
    When the provider contract is validated
    Then the delegation templates are refused with "the approval mode is unspecified"

  @state:error
  Scenario: A grant naming nothing is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the grant names no scope and no group"

  @state:error
  Scenario Outline: A budget with a zero count or window is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK <budget>'
    When the provider contract is validated
    Then the delegation templates are refused with "the budget has a zero count or window"

    Examples:
      | budget                                    |
      | budget: { count: 0 window_seconds: 60 }   |
      | budget: { window_seconds: 60 }            |
      | budget: { count: 5 window_seconds: 0 }    |
      | budget: { count: 5 }                      |

  @state:error
  Scenario Outline: A template name that is not lower_snake_case is refused
    Given the provider ships the template "<name>" labelled "Departure assistant" with the grant 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the name is not lower_snake_case"

    # `<empty>` stands for no text; the name must start with a lowercase letter.
    Examples:
      | name               |
      | DepartureAssistant |
      | <empty>            |
      | _assistant         |
      | 1assistant         |

  @state:error
  Scenario: Two templates of one service with the same name are refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK'
    And the provider ships the template "departure_assistant" labelled "Another" with the grant 'scope: "departure:read" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "another template of the service has this name"

  @state:error
  Scenario: A grant on a capability group the provider does not provide is refused
    Given the provider declares the group "foreign" conferring the scope "departure:read"
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'group: "foreign" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the group `foreign` is not declared by the service"

  @state:error
  Scenario: A grant on a capability group that confers a scope the provider does not declare is refused
    Given the provider provides the capability "ghostly"
    And the provider declares the group "ghostly" conferring the scope "departure:ghost"
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'group: "ghostly" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the group `ghostly` is not declared by the service"

  @state:error
  Scenario: A grant on a scope whose name carries whitespace is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure confirm" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "a scope, group or dimension name carries whitespace, a control or format character, punctuation or a wildcard"

  @state:error
  Scenario Outline: A scope or group name carrying a format character, a wildcard or punctuation is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant '<kind>: "<name>" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "a scope, group or dimension name carries whitespace, a control or format character, punctuation or a wildcard"
    And the refusal shows no control or format character

    # `<soft-hyphen>` stands for U+00AD and `<escape>` for U+001B, which the step
    # writes into the name.
    Examples:
      | kind  | name                       |
      | scope | departure<soft-hyphen>confirm |
      | group | departure<soft-hyphen>confirm |
      | scope | departure:*                |
      | group | plan(ning)                 |
      | scope | departure<escape>confirm   |
      | group | departure<escape>confirm   |

  @state:error
  Scenario Outline: A selector on a dimension whose name carries an unsafe character is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "<name>" values: "ops" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "a scope, group or dimension name carries whitespace, a control or format character, punctuation or a wildcard"
    And the refusal shows no control or format character

    # `<soft-hyphen>` stands for U+00AD and `<escape>` for U+001B, which the step
    # writes into the name.
    Examples:
      | name                     |
      | chan<soft-hyphen>nel     |
      | chan<escape>nel          |
      | chan*                    |

  @state:error
  Scenario Outline: A template name carrying a control or format character is refused and shown escaped
    Given the provider ships the template "<name>" labelled "Departure assistant" with the grant 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "template `departure\u{<code>}assistant`: the name is not lower_snake_case"
    And the refusal shows no control or format character

    # `<newline>` stands for U+000A, `<escape>` for U+001B and `<bidi-override>`
    # for U+202E, which the step writes into the name; the refusal shows each as
    # the escape `\u{<code>}` in lowercase hexadecimal.
    Examples:
      | name                            | code |
      | departure<newline>assistant     | a    |
      | departure<escape>assistant      | 1b   |
      | departure<bidi-override>assistant | 202e |

  @state:error
  Scenario: A group is not the provider's to confer when one entry naming it confers an undeclared scope
    Given the provider declares the group "planning" conferring the scope "departure:ghost"
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'group: "planning" approval_mode: APPROVAL_MODE_PRE_APPROVED'
    When the provider contract is validated
    Then the delegation templates are refused with "the group `planning` is not declared by the service"

  @state:error
  Scenario Outline: A selector value with whitespace, a control or format character or punctuation is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "<dimension>" values: "<value>" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "a value on `<dimension>` carries whitespace, a control or format character, or punctuation"
    And the refusal shows no control or format character

    # A table cell cannot carry a newline, a quote or an invisible character:
    # `<newline>`, `<quote>`, `<escape>` (U+001B) and `<soft-hyphen>` (U+00AD)
    # stand for them, and the step writes the character into the value.
    Examples:
      | dimension       | value                |
      | channel         | ops dev              |
      | channel         | ops<newline>dev      |
      | channel         | ops<quote>dev        |
      | channel         | ops<soft-hyphen>dev  |
      | channel         | ops<escape>dev       |
      | channel         | *<soft-hyphen>       |
      | departure_group | summer/(alps)        |

  @state:error
  Scenario: A grant with an approval mode the grammar does not define is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" approval_mode: 7'
    When the provider contract is validated
    Then the delegation templates are refused with "the approval mode 7 is not one the grammar defines"

  @state:error
  Scenario Outline: A template whose label has no text is refused
    Given the provider ships the template "departure_assistant" labelled "<label>" with the grant 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the label has no text"

    # `<empty>` stands for no text and `<space>` for a space, which a table
    # cell cannot carry; the step writes them into the label.
    Examples:
      | label                |
      | <empty>              |
      | <space>              |
      | <space><space><space> |

  @state:error
  Scenario: A template with no grant is refused
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with no grant
    When the provider contract is validated
    Then the delegation templates are refused with "the template has no grant"

  @state:happy
  Scenario: A scope the provider declares as a provided broker scope is the provider's to grant
    Given the provider provides the broker scope "departure:events" of the capability "planning"
    And the provider ships the template "departure_watcher" labelled "Departure watcher" with the grant 'scope: "departure:events" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then its grant 0 reads back the scope "departure:events"
    And the delegation templates are accepted

  @state:happy
  Scenario: A group whose capability the provider declares through the singular capability option is accepted
    Given the provider provides the capability "legacy" through the singular option
    And the provider declares the group "legacy" conferring the scope "departure:read"
    And the provider ships the template "legacy_reader" labelled "Legacy reader" with the grant 'group: "legacy" approval_mode: APPROVAL_MODE_PRE_APPROVED'
    When the provider contract is validated
    Then its grant 0 reads back the group "legacy"
    And the delegation templates are accepted

  @state:error
  Scenario: A group entry listing no scope does not make a group
    Given the provider provides the capability "scopeless"
    And the provider declares the group "scopeless" conferring no scope
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'group: "scopeless" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the group `scopeless` is not declared by the service"

  @state:happy
  Scenario: Template names are unique per service, not across the contract
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK'
    And a second service with an RPC requiring the scope "departure:confirm"
    And the second service ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are accepted

  @state:happy
  Scenario Outline: A literal of an exact or UUID dimension is accepted
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "<dimension>" values: "<value>" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then its grant 0 reads back the selector "<dimension>" with the values "<value>"
    And the delegation templates are accepted

    # An exact dimension has no segment rule, so `ops//dev` is a plain literal.
    Examples:
      | dimension    | value                                |
      | channel      | ops                                  |
      | channel      | ops//dev                             |
      | departure_id | 0190f2a4-7b1c-7d3e-8a52-3f6c1d9e4b70 |

  @state:error
  Scenario: An empty segment is judged by the separator of the dimension
    Given the provider declares the dimension "route" matched by segment prefix on "."
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "route" values: "summer..alps" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the value `summer..alps` is not valid on `route`"

  @state:happy
  Scenario: A value is a literal when it carries another dimension's separator
    Given the provider declares the dimension "route" matched by segment prefix on "."
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "route" values: "summer/alps" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are accepted

  @state:error
  Scenario: A group whose entry lists a declared and an undeclared scope is not a group
    Given the provider provides the capability "mixed"
    And the provider declares the group "mixed" conferring the scopes "departure:read, departure:ghost"
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'group: "mixed" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the group `mixed` is not declared by the service"

  @state:error
  Scenario: A provided capability with no group entry is not a group
    Given the provider provides the capability "billing"
    And the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'group: "billing" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the group `billing` is not declared by the service"

  @state:error
  Scenario: A template is checked against the scopes of its own service
    Given a second service with an RPC requiring the scope "departure:confirm"
    And the second service ships the template "other_reader" labelled "Other reader" with the grant 'scope: "departure:read" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "contract.v1.OtherService template `other_reader` grant 0: the scope `departure:read` is not declared by the service"
    And the delegation templates are refused 1 time

  @state:error
  Scenario: A refusal names the service, the template and the position of the grant
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grants 'scope: "departure:confirm" approval_mode: APPROVAL_MODE_ASK' and 'scope: "departure:delete" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "contract.v1.ProviderService template `departure_assistant` grant 1: the scope `departure:delete` is not declared by the service"
    And the delegation templates are refused 1 time

  @state:error
  Scenario: Every refusal is reported in service, template and grant order
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grants 'scope: "departure:delete" approval_mode: APPROVAL_MODE_ASK' and 'group: "billing" approval_mode: APPROVAL_MODE_ASK'
    And the provider ships the template "second_assistant" labelled "Second assistant" with no grant
    And a second service with an RPC requiring the scope "departure:confirm"
    And the second service ships the template "other_reader" labelled "Other reader" with the grant 'scope: "departure:read" approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused 4 times
    And the delegation templates are refused with "contract.v1.ProviderService template `departure_assistant` grant 0: the scope `departure:delete` is not declared by the service"
    And the delegation templates are refused with "contract.v1.ProviderService template `departure_assistant` grant 1: the group `billing` is not declared by the service"
    And the delegation templates are refused with "contract.v1.ProviderService template `second_assistant`: the template has no grant"
    And the refusal "grant 0: the scope `departure:delete`" is reported before "grant 1: the group `billing`"
    And the refusal "grant 1: the group `billing`" is reported before "template `second_assistant`: the template has no grant"
    And the delegation templates are refused with "contract.v1.OtherService template `other_reader` grant 0: the scope `departure:read` is not declared by the service"
    And the refusal "template `second_assistant`: the template has no grant" is reported before "contract.v1.OtherService template `other_reader`"

  @state:happy
  Scenario: A contract that imports no delegation templates passes
    Given the provider contract imports no delegation templates
    When the provider contract is validated
    Then the delegation templates are accepted

  @state:error
  Scenario: Every value of a selector is checked
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grant 'scope: "departure:confirm" selectors: { dimension: "channel" values: "ops" values: "ops-?" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then the delegation templates are refused with "the value `ops-?` on `channel` carries a wildcard character"
    And the delegation templates are refused 1 time

  @state:happy
  Scenario: Two grants of one template may select on the same dimension
    Given the provider ships the template "departure_assistant" labelled "Departure assistant" with the grants 'scope: "departure:confirm" selectors: { dimension: "channel" values: "ops" } approval_mode: APPROVAL_MODE_ASK' and 'scope: "departure:read" selectors: { dimension: "channel" values: "dev" } approval_mode: APPROVAL_MODE_ASK'
    When the provider contract is validated
    Then its grant 1 reads back the selector "channel" with the values "dev"
    And the delegation templates are accepted

  @state:happy
  Scenario: The shipped grant grammar carries no wildcard, regular-expression or free-text condition
    When the provider contract is validated
    Then the grant grammar message "bones.v1.ProviderGrant" declares exactly the fields "scope, group, selectors, approval_mode, budget"
    And the grant grammar message "bones.v1.AttributeSelector" declares exactly the fields "dimension, values"
    And the grant grammar message "bones.v1.Budget" declares exactly the fields "count, window_seconds"
    And the service option "bones.v1.delegation_templates" is declared with the number 5102374
    And the delegation templates are accepted
