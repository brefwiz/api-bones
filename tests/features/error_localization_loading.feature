# SPDX-License-Identifier: MIT
@library @connect @ts-sdk-only
Feature: TypeScript error catalogs load per locale

  A browser page that renders errors in English must not carry the other 28
  platform catalogs. The TypeScript package ships the English catalog in its
  base module and every other platform locale as a chunk fetched by
  `loadErrorCatalog`. Until a locale's chunk has loaded, rendering in that
  locale is refused with the rule `locale_not_loaded` rather than shown in
  English by mistake. A tag no platform locale matches negotiates to English,
  which is always ready, so it renders without loading anything.

  Rust embeds every platform catalog at compile time, so it has nothing to load
  and no row of this contract: the surface is TypeScript alone.

  One `loadErrorCatalog` call is all a page makes per locale: it fetches the
  api-bones catalog and the catalog of every service that registered a loader
  for that locale, so a service's errors never render half-loaded.

  The scenarios run in order, before any other contract loads a catalog, so the
  module starts with English alone; only the first scenario depends on that,
  and each later one may load locales.

  Background:
    Given a Connect error "resource_exhausted" whose ErrorInfo has code "RATE_LIMITED", emitter "" and args "(none)"

  Scenario Outline: Before a catalog loads, only English and an unmatched tag render
    Given no platform catalog but English is loaded
    When the error is localized for "<locale>" from the loaded catalogs
    Then the localization outcome is "<outcome>"

    Examples:
      | locale   | outcome                    |
      | fr       | refused: locale_not_loaded |
      | de-CH,en | refused: locale_not_loaded |
      | xx       | Too many requests          |
      | en       | Too many requests          |

  Scenario: A locale renders after its catalog is loaded
    When the catalog for "fr-CA" is loaded
    And the error is localized for "fr" from the loaded catalogs
    Then the localized text is "Trop de requêtes"
    And the localized hint is "Vous allez trop vite. Veuillez ralentir et réessayer."

  Scenario: Loading one locale loads every registered service's catalog
    Given the error catalogs of emitter "lazy-one" are registered for loading from fixture "catalogs/lazy-one"
    And the error catalogs of emitter "lazy-two" are registered for loading from fixture "catalogs/lazy-two"
    When the catalog for "fr" is loaded
    Then code "PAUSED" of emitter "lazy-one" renders "Ce projet est en pause." for "fr"
    And code "PAUSED" of emitter "lazy-two" renders "Cette facture est en attente." for "fr"
