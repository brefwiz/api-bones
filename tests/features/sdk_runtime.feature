# SPDX-License-Identifier: MIT
@library @sdk-runtime @ts-sdk-only
Feature: The TypeScript SDK runtime revives errors, formats them and walks lists

  Every generated TypeScript SDK wrapper imports one runtime package. The Rust
  core reports a failed call as `bones.sdk.v1.CallError` protobuf bytes and a
  list as a handle that yields one `bones.sdk.v1.Page` at a time; the runtime
  revives the error into `BonesError` or `PageError`, formats it from a
  registered catalog, opens the core's spans live in OpenTelemetry from
  `SpanStart` and `SpanEnd` bytes and walks a list handle as async iterables.
  The runtime makes no network call and decides nothing. The list walk is
  proven in the runtime's unit tests and through the generated wrapper over
  the real core, never through a hand-written handle.

  Rust callers use `api-bones-sdk` directly; the runtime is the TypeScript face
  alone, so the surface is TypeScript only.

  The catalog scenarios run in order, in one module instance: the first
  proves the package ships no catalog of its own, and the later ones register
  catalogs. An error's arguments are the typed `bones.v1.ErrorArg` values the
  Rust core reports (string, int64, double, timestamp), so a count, a number
  and a timestamp reach the formatter as the runtime revives them.

  Scenario: The package ships no error catalog of its own
    Given an error "SERVICE_UNAVAILABLE" from emitter "bones" of platform "SERVICE_UNAVAILABLE" with the typed arguments '{}'
    When the error is formatted for locale "en"
    Then the formatted text is ""

  Scenario Outline: A registered catalog formats the message of the error's emitter
    Given the error catalog for locale "en" is registered from recorded file "catalogs/emitters"
    And an error "GRANT_MISSING" from emitter "<emitter>" of platform "FORBIDDEN" with the typed arguments '{"label":{"kind":"string","value":"Team"}}'
    When the error is formatted for locale "en"
    Then the formatted text is "<text>"

    Examples:
      | emitter | text                      |
      | example | Access to Team is missing |
      | other   | Team is locked            |

  Scenario: A locale without an entry falls back to the English text of the emitter
    Given the error catalog for locale "en" is registered from recorded file "catalogs/emitters"
    And an error "GRANT_MISSING" from emitter "example" of platform "FORBIDDEN" with the typed arguments '{"label":{"kind":"string","value":"Team"}}'
    When the error is formatted for locale "fr"
    Then the formatted text is "Access to Team is missing"

  Scenario Outline: An emitter without an entry formats the platform text of its locale
    Given the platform catalog for locale "<locale>" is registered
    And an error "GRANT_MISSING" from emitter "absent" of platform "FORBIDDEN" with the typed arguments '{"label":{"kind":"string","value":"Team"}}'
    When the error is formatted for locale "<locale>"
    Then the formatted text is the platform text of the error in the catalog of locale "<locale>"

    Examples:
      | locale |
      | ar |
      | bg |
      | cs |
      | da |
      | de |
      | el |
      | en |
      | es |
      | et |
      | fi |
      | fr |
      | he |
      | hr |
      | hu |
      | it |
      | ja |
      | ko |
      | lt |
      | lv |
      | nb |
      | nl |
      | pl |
      | pt-BR |
      | ro |
      | sk |
      | sl |
      | sv |
      | tr |
      | zh-Hans |

  Scenario Outline: A corpus message formats in every platform locale
    Given the corpus catalog for locale "<locale>" is registered
    And an error "<code>" from emitter "corpus" of platform "FORBIDDEN" with the typed arguments '<args>'
    When the error is formatted for locale "<locale>"
    Then the formatted text is "<text>"

    # Expected text derived from bare Intl on Node 24.16.0, ICU 78.3, CLDR 48.0, tz 2026b.
    Examples:
      | locale | code       | args | text |
      # ar: cardinal zero, one, two, few, many, other; ordinal other
      | ar | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | المتبقي: 0 عناصر |
      | ar | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | المتبقي: 1 عنصر |
      | ar | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | المتبقي: 2 عنصران |
      | ar | ITEMS_LEFT | {"count":{"kind":"double","value":3}} | المتبقي: 3 عناصر |
      | ar | ITEMS_LEFT | {"count":{"kind":"double","value":11}} | المتبقي: 11 عنصرًا |
      | ar | ITEMS_LEFT | {"count":{"kind":"double","value":100}} | المتبقي: 100 عنصر |
      | ar | RANK | {"place":{"kind":"double","value":1}} | المركز 1 |
      | ar | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | الحصة المستخدمة: 1,234,567.891 من 2,000,000 |
      | ar | LOCKED | {"label":{"kind":"string","value":"Team"}} | مقفل: Team |
      | ar | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | القفل قائم منذ 1\u{200F}/1\u{200F}/1970 |
      # bg: cardinal one, other; ordinal other
      | bg | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Остава 1 елемент |
      | bg | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Остават 2 елемента |
      | bg | RANK | {"place":{"kind":"double","value":1}} | №\u{A0}1 |
      | bg | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Квота 1\u{A0}234\u{A0}567,891 от 2\u{A0}000\u{A0}000 |
      | bg | LOCKED | {"label":{"kind":"string","value":"Team"}} | Заключено: Team |
      | bg | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Заключено от 1.01.1970 г. |
      # cs: cardinal one, few, many, other; ordinal other
      | cs | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Zbývá 1 položka |
      | cs | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Zbývají 2 položky |
      | cs | ITEMS_LEFT | {"count":{"kind":"double","value":1.5}} | Zbývá 1,5 položky |
      | cs | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | Zbývá 0 položek |
      | cs | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | cs | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvóta: 1\u{A0}234\u{A0}567,891 z\u{A0}2\u{A0}000\u{A0}000 |
      | cs | LOCKED | {"label":{"kind":"string","value":"Team"}} | Zamčeno: Team |
      | cs | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Zamčeno od 1. 1. 1970 |
      # da: cardinal one, other; ordinal other
      | da | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 element tilbage |
      | da | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 elementer tilbage |
      | da | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | da | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvote 1.234.567,891 af 2.000.000 |
      | da | LOCKED | {"label":{"kind":"string","value":"Team"}} | Låst: Team |
      | da | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Låst siden 1.1.1970 |
      # de: cardinal one, other; ordinal other
      | de | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 Element übrig |
      | de | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 Elemente übrig |
      | de | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | de | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kontingent: 1.234.567,891 von 2.000.000 |
      | de | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team ist gesperrt |
      | de | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Gesperrt seit 1.1.1970 |
      # el: cardinal one, other; ordinal other
      | el | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Απομένει 1 στοιχείο |
      | el | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Απομένουν 2 στοιχεία |
      | el | RANK | {"place":{"kind":"double","value":1}} | Θέση 1 |
      | el | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Ποσόστωση: 1.234.567,891 από 2.000.000 |
      | el | LOCKED | {"label":{"kind":"string","value":"Team"}} | Κλειδωμένο: Team |
      | el | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Κλειδωμένο από 1/1/1970 |
      # en: cardinal one, other; ordinal one, two, few, other
      | en | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 item left |
      | en | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 items left |
      | en | RANK | {"place":{"kind":"double","value":1}} | 1st |
      | en | RANK | {"place":{"kind":"double","value":2}} | 2nd |
      | en | RANK | {"place":{"kind":"double","value":3}} | 3rd |
      | en | RANK | {"place":{"kind":"double","value":4}} | 4th |
      | en | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Quota 1,234,567.891 of 2,000,000 |
      | en | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team is locked |
      | en | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Locked since 1/1/1970 |
      # es: cardinal one, many, other; ordinal other
      | es | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Queda 1 elemento |
      | es | ITEMS_LEFT | {"count":{"kind":"double","value":1000000}} | Quedan 1.000.000 de elementos |
      | es | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Quedan 2 elementos |
      | es | RANK | {"place":{"kind":"double","value":1}} | 1.º |
      | es | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Cuota: 1.234.567,891 de 2.000.000 |
      | es | LOCKED | {"label":{"kind":"string","value":"Team"}} | Bloqueado: Team |
      | es | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Bloqueado desde el 1/1/1970 |
      # et: cardinal one, other; ordinal other
      | et | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Alles on 1 üksus |
      | et | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Alles on 2 üksust |
      | et | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | et | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvoot: 1\u{A0}234\u{A0}567,891 / 2\u{A0}000\u{A0}000 |
      | et | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team on lukustatud |
      | et | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Lukustatud alates 1.1.1970 |
      # fi: cardinal one, other; ordinal other
      | fi | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 kohde jäljellä |
      | fi | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 kohdetta jäljellä |
      | fi | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | fi | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kiintiö 1\u{A0}234\u{A0}567,891 / 2\u{A0}000\u{A0}000 |
      | fi | LOCKED | {"label":{"kind":"string","value":"Team"}} | Lukittu: Team |
      | fi | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Lukittu 1.1.1970 alkaen |
      # fr: cardinal one, many, other; ordinal one, other
      | fr | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 élément restant |
      | fr | ITEMS_LEFT | {"count":{"kind":"double","value":1000000}} | 1\u{202F}000\u{202F}000 d’éléments restants |
      | fr | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 éléments restants |
      | fr | RANK | {"place":{"kind":"double","value":1}} | 1er |
      | fr | RANK | {"place":{"kind":"double","value":2}} | 2e |
      | fr | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Quota\u{A0}: 1\u{202F}234\u{202F}567,891 sur 2\u{202F}000\u{202F}000 |
      | fr | LOCKED | {"label":{"kind":"string","value":"Team"}} | Verrouillé\u{A0}: Team |
      | fr | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Verrouillé depuis le 01/01/1970 |
      # he: cardinal one, two, other; ordinal other
      | he | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | פריטים שנותרו: 1 |
      | he | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | פריטים שנותרו: 2 |
      | he | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | פריטים שנותרו: 0 |
      | he | RANK | {"place":{"kind":"double","value":1}} | מקום 1 |
      | he | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | מכסה 1,234,567.891 מתוך 2,000,000 |
      | he | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team: נעילה בתוקף |
      | he | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | נעילה בתוקף מאז 1.1.1970 |
      # hr: cardinal one, few, other; ordinal other
      | hr | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Preostalo stavki: 1 |
      | hr | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Preostalo stavki: 2 |
      | hr | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | Preostalo stavki: 0 |
      | hr | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | hr | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvota: 1.234.567,891 od 2.000.000 |
      | hr | LOCKED | {"label":{"kind":"string","value":"Team"}} | Zaključano: Team |
      | hr | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Zaključano od 01. 01. 1970. |
      # hu: cardinal one, other; ordinal one, other
      | hu | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 elem maradt |
      | hu | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 elem maradt |
      | hu | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | hu | RANK | {"place":{"kind":"double","value":2}} | 2. |
      | hu | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvóta: 1\u{A0}234\u{A0}567,891 / 2\u{A0}000\u{A0}000 |
      | hu | LOCKED | {"label":{"kind":"string","value":"Team"}} | Zárolva: Team |
      | hu | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | 1970. 01. 01. óta zárolva |
      # it: cardinal one, many, other; ordinal many, other
      | it | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 elemento rimasto |
      | it | ITEMS_LEFT | {"count":{"kind":"double","value":1000000}} | 1.000.000 di elementi rimasti |
      | it | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 elementi rimasti |
      | it | RANK | {"place":{"kind":"double","value":8}} | 8º |
      | it | RANK | {"place":{"kind":"double","value":1}} | 1º |
      | it | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Quota: 1.234.567,891 di 2.000.000 |
      | it | LOCKED | {"label":{"kind":"string","value":"Team"}} | Bloccato: Team |
      | it | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Bloccato dal 01/01/1970 |
      # ja: cardinal other; ordinal other
      | ja | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 残り1件 |
      | ja | RANK | {"place":{"kind":"double","value":1}} | 1位 |
      | ja | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | クォータ：1,234,567.891/2,000,000 |
      | ja | LOCKED | {"label":{"kind":"string","value":"Team"}} | Teamはロック中です |
      | ja | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | 1970/1/1からロック中です |
      # ko: cardinal other; ordinal other
      | ko | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1개 남았어요 |
      | ko | RANK | {"place":{"kind":"double","value":1}} | 1위 |
      | ko | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | 할당량 1,234,567.891 / 2,000,000 |
      | ko | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team: 잠겨 있어요 |
      | ko | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | 1970. 1. 1.부터 잠겨 있어요 |
      # lt: cardinal one, few, many, other; ordinal other
      | lt | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Liko 1 elementas |
      | lt | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Liko 2 elementai |
      | lt | ITEMS_LEFT | {"count":{"kind":"double","value":1.5}} | Liko 1,5 elemento |
      | lt | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | Liko 0 elementų |
      | lt | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | lt | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvota 1\u{A0}234\u{A0}567,891 iš 2\u{A0}000\u{A0}000 |
      | lt | LOCKED | {"label":{"kind":"string","value":"Team"}} | Užrakinta: Team |
      | lt | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Užrakinta nuo 1970-01-01 |
      # lv: cardinal zero, one, other; ordinal other
      | lv | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | Atlikuši 0 vienumu |
      | lv | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Atlicis 1 vienums |
      | lv | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Atlikuši 2 vienumi |
      | lv | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | lv | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvota: 1\u{A0}234\u{A0}567,891 no 2\u{A0}000\u{A0}000 |
      | lv | LOCKED | {"label":{"kind":"string","value":"Team"}} | Bloķēts: Team |
      | lv | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Bloķēts kopš 1.01.1970. |
      # nb: cardinal one, other; ordinal other
      | nb | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 element igjen |
      | nb | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 elementer igjen |
      | nb | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | nb | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvote: 1\u{A0}234\u{A0}567,891 av 2\u{A0}000\u{A0}000 |
      | nb | LOCKED | {"label":{"kind":"string","value":"Team"}} | Låst: Team |
      | nb | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Låst siden 1.1.1970 |
      # nl: cardinal one, other; ordinal other
      | nl | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | nog 1 item |
      | nl | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | nog 2 items |
      | nl | RANK | {"place":{"kind":"double","value":1}} | 1e |
      | nl | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Quota 1.234.567,891 van 2.000.000 |
      | nl | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team is vergrendeld |
      | nl | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Vergrendeld sinds 1-1-1970 |
      # pl: cardinal one, few, many, other; ordinal other
      | pl | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Pozostał 1 element |
      | pl | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Pozostały 2 elementy |
      | pl | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | Pozostało 0 elementów |
      | pl | ITEMS_LEFT | {"count":{"kind":"double","value":1.5}} | Pozostało 1,5 elementu |
      | pl | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | pl | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Przydział 1\u{A0}234\u{A0}567,891 z\u{A0}2\u{A0}000\u{A0}000 |
      | pl | LOCKED | {"label":{"kind":"string","value":"Team"}} | Zablokowano: Team |
      | pl | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Blokada trwa od 1.01.1970 |
      # pt-BR: cardinal one, many, other; ordinal other
      | pt-BR | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 item restante |
      | pt-BR | ITEMS_LEFT | {"count":{"kind":"double","value":1000000}} | 1.000.000 de itens restantes |
      | pt-BR | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 itens restantes |
      | pt-BR | RANK | {"place":{"kind":"double","value":1}} | 1º |
      | pt-BR | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Cota: 1.234.567,891 de 2.000.000 |
      | pt-BR | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team está com bloqueio |
      | pt-BR | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Bloqueio desde 01/01/1970 |
      # ro: cardinal one, few, other; ordinal one, other
      | ro | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 element rămas |
      | ro | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 elemente rămase |
      | ro | ITEMS_LEFT | {"count":{"kind":"double","value":21}} | 21 de elemente rămase |
      | ro | RANK | {"place":{"kind":"double","value":1}} | locul 1 |
      | ro | RANK | {"place":{"kind":"double","value":2}} | locul 2 |
      | ro | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Cotă: 1.234.567,891 din 2.000.000 |
      | ro | LOCKED | {"label":{"kind":"string","value":"Team"}} | Blocat: Team |
      | ro | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Blocat din 01.01.1970 |
      # sk: cardinal one, few, many, other; ordinal other
      | sk | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Zostáva 1 položka |
      | sk | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Zostávajú 2 položky |
      | sk | ITEMS_LEFT | {"count":{"kind":"double","value":1.5}} | Zostáva 1,5 položky |
      | sk | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | Zostáva 0 položiek |
      | sk | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | sk | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvóta: 1\u{A0}234\u{A0}567,891 z\u{A0}2\u{A0}000\u{A0}000 |
      | sk | LOCKED | {"label":{"kind":"string","value":"Team"}} | Uzamknuté: Team |
      | sk | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Uzamknuté od 1. 1. 1970 |
      # sl: cardinal one, two, few, other; ordinal other
      | sl | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | Preostal je 1 element |
      | sl | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | Preostala sta 2 elementa |
      | sl | ITEMS_LEFT | {"count":{"kind":"double","value":3}} | Preostalo elementov: 3 |
      | sl | ITEMS_LEFT | {"count":{"kind":"double","value":0}} | Preostalo je 0 elementov |
      | sl | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | sl | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvota 1.234.567,891 od 2.000.000 |
      | sl | LOCKED | {"label":{"kind":"string","value":"Team"}} | Zaklenjeno: Team |
      | sl | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Zaklenjeno od 1. 1. 1970 |
      # sv: cardinal one, other; ordinal one, other
      | sv | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 objekt kvar |
      | sv | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 objekt kvar |
      | sv | RANK | {"place":{"kind":"double","value":1}} | 1:a |
      | sv | RANK | {"place":{"kind":"double","value":3}} | 3:e |
      | sv | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kvot: 1\u{A0}234\u{A0}567,891 av 2\u{A0}000\u{A0}000 |
      | sv | LOCKED | {"label":{"kind":"string","value":"Team"}} | Låst: Team |
      | sv | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | Låst sedan 1970-01-01 |
      # tr: cardinal one, other; ordinal other
      | tr | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 1 öğe kaldı |
      | tr | ITEMS_LEFT | {"count":{"kind":"double","value":2}} | 2 öğe kaldı |
      | tr | RANK | {"place":{"kind":"double","value":1}} | 1. |
      | tr | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | Kota: 1.234.567,891 / 2.000.000 |
      | tr | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team kilitli |
      | tr | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | 01.01.1970 tarihinden beri kilitli |
      # zh-Hans: cardinal other; ordinal other
      | zh-Hans | ITEMS_LEFT | {"count":{"kind":"double","value":1}} | 还剩1项 |
      | zh-Hans | RANK | {"place":{"kind":"double","value":1}} | 第1名 |
      | zh-Hans | QUOTA | {"used":{"kind":"double","value":1234567.891},"limit":{"kind":"double","value":2000000}} | 配额已用1,234,567.891/2,000,000 |
      | zh-Hans | LOCKED | {"label":{"kind":"string","value":"Team"}} | Team已锁定 |
      | zh-Hans | SINCE | {"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}} | 自1970/1/1起锁定 |

  Scenario Outline: A corpus date formats in the requested time zone
    Given the corpus catalog for locale "<locale>" is registered
    And an error "SINCE" from emitter "corpus" of platform "FORBIDDEN" with the typed arguments '{"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}}'
    When the error is formatted for locale "<locale>" in the time zone "<timeZone>"
    Then the formatted text is "<text>"

    # Expected text derived from bare Intl on Node 24.16.0, ICU 78.3, CLDR 48.0, tz 2026b.
    Examples:
      | locale | timeZone   | text |
      | en     | UTC        | Locked since 1/1/1970 |
      | ar | Asia/Tokyo | القفل قائم منذ 2\u{200F}/1\u{200F}/1970 |
      | bg | Asia/Tokyo | Заключено от 2.01.1970 г. |
      | cs | Asia/Tokyo | Zamčeno od 2. 1. 1970 |
      | da | Asia/Tokyo | Låst siden 2.1.1970 |
      | de | Asia/Tokyo | Gesperrt seit 2.1.1970 |
      | el | Asia/Tokyo | Κλειδωμένο από 2/1/1970 |
      | en | Asia/Tokyo | Locked since 1/2/1970 |
      | es | Asia/Tokyo | Bloqueado desde el 2/1/1970 |
      | et | Asia/Tokyo | Lukustatud alates 2.1.1970 |
      | fi | Asia/Tokyo | Lukittu 2.1.1970 alkaen |
      | fr | Asia/Tokyo | Verrouillé depuis le 02/01/1970 |
      | he | Asia/Tokyo | נעילה בתוקף מאז 2.1.1970 |
      | hr | Asia/Tokyo | Zaključano od 02. 01. 1970. |
      | hu | Asia/Tokyo | 1970. 01. 02. óta zárolva |
      | it | Asia/Tokyo | Bloccato dal 02/01/1970 |
      | ja | Asia/Tokyo | 1970/1/2からロック中です |
      | ko | Asia/Tokyo | 1970. 1. 2.부터 잠겨 있어요 |
      | lt | Asia/Tokyo | Užrakinta nuo 1970-01-02 |
      | lv | Asia/Tokyo | Bloķēts kopš 2.01.1970. |
      | nb | Asia/Tokyo | Låst siden 2.1.1970 |
      | nl | Asia/Tokyo | Vergrendeld sinds 2-1-1970 |
      | pl | Asia/Tokyo | Blokada trwa od 2.01.1970 |
      | pt-BR | Asia/Tokyo | Bloqueio desde 02/01/1970 |
      | ro | Asia/Tokyo | Blocat din 02.01.1970 |
      | sk | Asia/Tokyo | Uzamknuté od 2. 1. 1970 |
      | sl | Asia/Tokyo | Zaklenjeno od 2. 1. 1970 |
      | sv | Asia/Tokyo | Låst sedan 1970-01-02 |
      | tr | Asia/Tokyo | 02.01.1970 tarihinden beri kilitli |
      | zh-Hans | Asia/Tokyo | 自1970/1/2起锁定 |

  Scenario: A time zone the runtime does not know is refused
    Given the corpus catalog for locale "en" is registered
    And an error "SINCE" from emitter "corpus" of platform "FORBIDDEN" with the typed arguments '{"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}}'
    When the error is formatted for locale "en" in the unknown time zone "Mars/Olympus"
    Then the time zone is refused as "FormatErrorOptionError" with rule "unknown_time_zone" and message "option refused: unknown_time_zone"

  Scenario: The corpus outline covers exactly the platform locales
    Given the locales of the examples of the scenario outline "A corpus message formats in every platform locale"
    And the locales of the corpus files
    And the locales of the platform catalog files
    And the corpus catalog for locale "en" is registered
    And an error "ITEMS_LEFT" from emitter "corpus" of platform "FORBIDDEN" with the typed arguments '{"count":{"kind":"double","value":2}}'
    When the error is formatted for locale "en"
    Then the formatted text is "2 items left"
    And the locales of the examples, the corpus files and the platform catalog files are the same set

  Scenario: The time zone outline covers exactly the platform locales
    Given the locales of the examples of the scenario outline "A corpus date formats in the requested time zone"
    And the locales of the corpus files
    And the locales of the platform catalog files
    And the corpus catalog for locale "en" is registered
    And an error "SINCE" from emitter "corpus" of platform "FORBIDDEN" with the typed arguments '{"at":{"kind":"timestamp","value":"1970-01-01T20:00:00Z"}}'
    When the error is formatted for locale "en" in the time zone "UTC"
    Then the formatted text is "Locked since 1/1/1970"
    And the locales of the examples, the corpus files and the platform catalog files are the same set

  Scenario: A call error without page detail becomes a BonesError
    Given a call error with code "GRANT_MISSING" from emitter "example" of platform "FORBIDDEN", connect code "permission_denied" and message "grant missing"
    When the call error is revived
    Then the result is a BonesError that is not a PageError, with code "GRANT_MISSING", emitter "example", platform "FORBIDDEN", connect code "permission_denied" and message "grant missing"

  Scenario Outline: A call error naming a retry-after carries it in milliseconds
    Given a call error with code "RATE_LIMITED" from emitter "example" of platform "RATE_LIMITED", connect code "resource_exhausted" and message "the server is throttling"
    And the call error names a wait of <millis> ms
    When the call error is revived
    Then the result is a BonesError that is not a PageError, with code "RATE_LIMITED", emitter "example", platform "RATE_LIMITED", connect code "resource_exhausted" and message "the server is throttling"
    And the error names a wait of <millis> ms

    Examples:
      | millis |
      | 6000   |
      | 1500   |
      | 250    |

  Scenario: A call error naming no retry-after carries no wait
    Given a call error with code "GRANT_MISSING" from emitter "example" of platform "FORBIDDEN", connect code "permission_denied" and message "grant missing"
    When the call error is revived
    Then the error names no wait

  Scenario: A call error with a list failure becomes a PageError
    Given a call error with code "CURSOR_EXPIRED" from emitter "example" of platform "PRECONDITION_FAILED", connect code "failed_precondition" and message "the cursor expired"
    And the call error names a list failure of kind "cursor_expired" after 2 pages and 4 items
    When the call error is revived
    Then the result is a PageError of kind "cursor_expired" after 2 pages and 4 items

  Scenario Outline: Each typed argument revives to its plain type
    Given a call error with code "GRANT_MISSING" from emitter "example" of platform "FORBIDDEN", connect code "permission_denied" and message "grant missing"
    And the call error carries the arguments:
      | name    | kind      | value                          |
      | seats   | int64     | 9007199254740993               |
      | since   | timestamp | 2026-03-01T10:20:30.123456789Z |
      | share   | double    | 0.5                            |
      | ratio   | double    | NaN                            |
      | ceiling | double    | Infinity                       |
      | floor   | double    | -Infinity                      |
      | label   | string    | Team                           |
      | mystery | unset     |                                |
    When the call error is revived
    Then argument "<name>" is revived as "<revived>"

    Examples:
      | name    | revived                       |
      | label   | string Team                   |
      | seats   | bigint 9007199254740993       |
      | since   | date 2026-03-01T10:20:30.123Z |
      | share   | number 0.5                    |
      | ratio   | number NaN                    |
      | ceiling | number Infinity               |
      | floor   | number -Infinity              |
      | mystery | dropped                       |

  Scenario Outline: A cursor refusal is told apart from a failed call
    Given a call error with code "CURSOR_REJECTED" from emitter "example" of platform "PRECONDITION_FAILED", connect code "failed_precondition" and message "the list position was refused"
    And the call error names a list failure of kind "<kind>" after 0 pages and 0 items
    When the call error is revived
    Then the cursor rejection check answers "<answer>"

    Examples:
      | kind                  | answer |
      | cursor_invalid        | true   |
      | cursor_expired        | true   |
      | cursor_query_mismatch | true   |
      | rpc                   | false  |

  Scenario: A host rejection that is not CallError bytes is given back unchanged
    Given the host rejected a call with an Error saying "the host is offline"
    When the host rejection is mapped
    Then the mapped value is the very Error the host rejected with, and no BonesError

  Scenario: Starting a span returns the traceparent of the open span
    Given spans are recorded by an OpenTelemetry tracer
    When the core starts the client span "example.List"
    Then the start answered the traceparent of a span that is still open

  Scenario: The call context carries the active trace and the caller's abort signal
    Given a remote span with trace id "4bf92f3577b34da6a3ce929d0e0e4736" and span id "00f067aa0ba902b7" is active
    And the caller holds an abort signal
    When the call context is requested
    Then the call context holds the traceparent "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    And the call context holds the caller's abort signal

  Scenario: The call context carries the trace state of the active trace
    Given a remote span with trace id "4bf92f3577b34da6a3ce929d0e0e4736", span id "00f067aa0ba902b7" and trace state "vendor=value" is active
    When the call context is requested
    Then the call context holds the traceparent "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    And the call context holds the tracestate "vendor=value"

  Scenario: The call context holds nothing when no trace is active and no signal is given
    Given no trace is active
    When the call context is requested
    Then the call context holds nothing

  Scenario: A span started without a parent is a new root, whatever trace is active
    Given spans are recorded by an OpenTelemetry tracer
    And a remote span with trace id "4bf92f3577b34da6a3ce929d0e0e4736" and span id "00f067aa0ba902b7" is active
    When the core starts the client span "example.Root"
    Then the started span is a new root outside the active trace

  Scenario: A span started with a parent joins the parent's trace
    Given spans are recorded by an OpenTelemetry tracer
    When the core starts the client span "example.Child" under the parent "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    Then the started span joins the trace "4bf92f3577b34da6a3ce929d0e0e4736" as a child of the span "00f067aa0ba902b7"

  Scenario: A span started with a parent and a trace state carries the trace state
    Given spans are recorded by an OpenTelemetry tracer
    When the core starts the client span "example.Child" under the parent "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01" with the trace state "vendor=value"
    Then the started span carries the trace state "vendor=value"

  Scenario: A span that is not recording gives no traceparent
    Given spans are sampled out
    When the core starts the client span "example.List"
    Then the start answered that the span is not recording

  Scenario Outline: Ending a span sets its status and ends it
    Given spans are recorded by an OpenTelemetry tracer
    And a client span "example.List" is open
    When the core ends the span with status "<status>"
    Then the finished span "example.List" has status "<recorded>"

    Examples:
      | status | recorded |
      | ok     | OK       |
      | error  | ERROR    |
      | unset  | UNSET    |

  Scenario: An error end records its message on the span
    Given spans are recorded by an OpenTelemetry tracer
    And a client span "example.List" is open
    When the core ends the span with status "error"
    Then the finished span "example.List" has the status message "unavailable SERVICE_UNAVAILABLE"

  Scenario: Attributes given at start and at end are recorded on the finished span
    Given spans are recorded by an OpenTelemetry tracer
    And a client span "example.List" is open with the attribute "page.index" set to 2
    When the core ends the span with status "ok" and the attribute "page.items" set to 3
    Then the finished span "example.List" has the attribute "page.index" set to 2
    And the finished span "example.List" has the attribute "page.items" set to 3

  Scenario: An end naming an unknown traceparent finishes no span
    Given spans are recorded by an OpenTelemetry tracer
    And a client span "example.List" is open
    When the core ends a span named by the unknown traceparent "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    Then no span has finished
    And the client span "example.List" is still open

  Scenario: A lazy start runs once however many callers ask at once
    Given a start that counts how often it runs
    When the lazy start is called 3 times at once
    Then the start ran once and every call received its one result

  Scenario: A failed lazy start is forgotten and the next call starts again
    Given a start that fails its first run
    When the lazy start is called twice in turn
    Then the start ran twice, the first call was rejected and the second received "started"

  Scenario: The wasm loader input is the bytes of a file under Node
    Given a wasm module file on disk
    When the loader input is requested for that file
    Then the loader input is the bytes of the file
