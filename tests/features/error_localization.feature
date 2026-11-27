# SPDX-License-Identifier: MIT

@library @connect
Feature: Errors render in the viewer's locale from the owner's catalog

  A refused call carries a stable code and typed arguments, never text a
  viewer should read. The SDK renders it in the viewer's platform locale from
  the catalog of the code's owner: the emitter's catalog for its domain codes,
  api-bones's for the platform codes. The key is tried in the viewer's locale,
  then in English, before the next key; the last key is the platform code the
  Connect code maps to, which always has an English string, so the operator
  message and the raw code are never shown. Rust and TypeScript share one
  definition of key resolution, fallback, locale negotiation and argument
  bounds; plural, number and date formatting are each language's own CLDR
  formatting from one pinned CLDR release. A row only one language satisfies
  is a divergence.

  Text is compared after every Unicode white-space character becomes one
  U+0020, so a narrow no-break space in a CLDR time compares as a space;
  every other character, every plural or select branch and every substituted
  argument compares exactly.

  Arguments are written `key=type:value` joined by `; `, with type `string`,
  `int64`, `double` or `timestamp` (RFC 3339), `timestamp_raw:<seconds>:<nanos>`
  for a raw `google.protobuf.Timestamp`, and `(none)` for no arguments.
  `repeat(<text>,<n>)` expands to `<text>` written `<n>` times, `\uXXXX` is
  that code point, and `args(<n>)` is `<n>` arguments `k1=int64:1` to
  `k<n>=int64:<n>`. Every Connect error below carries the operator message
  "seat limit reached".

  Background:
    Given the platform error catalogs are loaded
    And the error catalogs of emitter "acme" are registered from fixture "catalogs/acme"
    And the error catalogs of emitter "acme" are registered again from fixture "catalogs/acme"
    And the error catalogs of emitter "acme-dup" are registered from fixture "catalogs/acme-dup-a"
    And the error catalogs of emitter "acme-dup" are registered from fixture "catalogs/acme-dup-b"
    And the error catalogs of emitter "bones" are registered from fixture "catalogs/bones-hijack"
    And the error catalogs of emitter "acme-hijack" are registered from fixture "catalogs/acme-hijack"

  Scenario Outline: A key resolves in the viewer's locale, then English, before the next key
    Given a Connect error "<connect_code>" whose ErrorInfo has code "<code>", emitter "<emitter>" and args "(none)"
    When the error is localized for "<locale>"
    Then the localized text is "<text>"
    And the localized hint is "<hint>"
    And the localized text does not contain "seat limit reached"

    Examples: the emitter's own key
      | locale | connect_code        | code     | emitter | text                               | hint                        |
      | en     | failed_precondition | ARCHIVED | acme    | This workspace is archived.        | Ask an owner to restore it. |
      | fr     | failed_precondition | ARCHIVED | acme    | Cet espace de travail est archivé. | none                        |
      | de     | failed_precondition | ARCHIVED | acme    | This workspace is archived.        | Ask an owner to restore it. |

    Examples: a platform code the emitter names
      | locale | connect_code       | code                | emitter | text              | hint                                                                        |
      | en     | resource_exhausted | RATE_LIMITED        | acme    | Too many requests | You're going too fast. Please slow down and try again.                      |
      | en     | aborted            | PRECONDITION_FAILED | acme    | Out of date       | The resource has changed since you loaded it. Please refresh and try again. |
      | en     | permission_denied  | RATE_LIMITED        |         | Too many requests | You're going too fast. Please slow down and try again.                      |

    Examples: the SDK boundary mismatch code in every platform locale
      | locale  | connect_code        | code                  | emitter | text                                     | hint                                                                                                     |
      | ar      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | إصدارات SDK غير متطابقة                  | أجزاء من هذا التطبيق غير متزامنة. أعد تحميل الصفحة وحاول مرة أخرى.                                       |
      | bg      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Версиите на SDK не съвпадат              | Части от това приложение не са синхронизирани. Презаредете страницата и опитайте отново.                 |
      | cs      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Verze SDK si neodpovídají                | Části této aplikace nejsou ve shodě. Načtěte stránku znovu a zkuste to znovu.                            |
      | da      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK-versionerne passer ikke sammen       | Dele af denne app er ikke synkroniserede. Genindlæs siden, og prøv igen.                                 |
      | de      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK-Versionen passen nicht zusammen      | Teile dieser App sind nicht aufeinander abgestimmt. Laden Sie die Seite neu und versuchen Sie es erneut. |
      | el      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Οι εκδόσεις του SDK δεν ταιριάζουν       | Τμήματα αυτής της εφαρμογής δεν είναι συγχρονισμένα. Φορτώστε ξανά τη σελίδα και δοκιμάστε ξανά.         |
      | en      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK versions do not match                | Parts of this app are out of step. Reload the page and try again.                                        |
      | es      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Las versiones del SDK no coinciden       | Algunas partes de esta aplicación no están sincronizadas. Recarga la página e inténtalo de nuevo.        |
      | et      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK versioonid ei ühti                   | Selle rakenduse osad ei ole sünkroonis. Laadige leht uuesti ja proovige uuesti.                          |
      | fi      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK-versiot eivät täsmää                 | Tämän sovelluksen osat eivät ole synkronoituja. Lataa sivu uudelleen ja yritä uudelleen.                 |
      | fr      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Les versions du SDK ne correspondent pas | Des parties de cette application ne sont pas synchronisées. Rechargez la page et réessayez.              |
      | he      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | גרסאות ה-SDK אינן תואמות                 | חלקים מהאפליקציה הזו אינם מסונכרנים. טען את הדף מחדש ונסה שוב.                                           |
      | hr      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Verzije SDK-a se ne podudaraju           | Dijelovi ove aplikacije nisu usklađeni. Ponovno učitajte stranicu i pokušajte ponovno.                   |
      | hu      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Az SDK-verziók nem egyeznek              | Az alkalmazás egyes részei nincsenek összhangban. Töltse újra az oldalt, és próbálja újra.               |
      | it      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Le versioni dell'SDK non corrispondono   | Alcune parti di questa app non sono sincronizzate. Ricarica la pagina e riprova.                         |
      | ja      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDKのバージョンが一致しません                         | このアプリの一部が同期していません。ページを再読み込みして、もう一度お試しください。                                                               |
      | ko      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK 버전이 일치하지 않아요                         | 이 앱의 일부가 서로 맞지 않아요. 페이지를 새로고침한 뒤 다시 시도해 주세요.                                                             |
      | lt      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK versijos nesutampa                   | Kai kurios šios programos dalys nesinchronizuotos. Iš naujo įkelkite puslapį ir bandykite dar kartą.     |
      | lv      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK versijas nesakrīt                    | Dažas šīs lietotnes daļas nav sinhronizētas. Pārlādējiet lapu un mēģiniet vēlreiz.                       |
      | nb      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK-versjonene samsvarer ikke            | Deler av denne appen er ikke synkronisert. Last inn siden på nytt og prøv igjen.                         |
      | nl      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK-versies komen niet overeen           | Onderdelen van deze app lopen niet gelijk. Laad de pagina opnieuw en probeer het opnieuw.                |
      | pl      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Wersje SDK nie pasują do siebie          | Części tej aplikacji nie są zsynchronizowane. Odśwież stronę i spróbuj ponownie.                         |
      | pt-BR   | failed_precondition | SDK_BOUNDARY_MISMATCH |         | As versões do SDK não correspondem       | Partes deste app estão dessincronizadas. Recarregue a página e tente novamente.                          |
      | ro      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Versiunile SDK nu se potrivesc           | Părți ale acestei aplicații nu sunt sincronizate. Reîncărcați pagina și încercați din nou.               |
      | sk      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Verzie SDK sa nezhodujú                  | Časti tejto aplikácie nie sú zosúladené. Načítajte stránku znova a skúste to znova.                      |
      | sl      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | Različice SDK se ne ujemajo              | Deli te aplikacije niso usklajeni. Ponovno naložite stran in poskusite znova.                            |
      | sv      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK-versionerna matchar inte             | Delar av den här appen är inte synkade. Ladda om sidan och försök igen.                                  |
      | tr      | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK sürümleri uyuşmuyor                  | Bu uygulamanın bazı parçaları eşzamanlı değil. Sayfayı yeniden yükleyin ve tekrar deneyin.               |
      | zh-Hans | failed_precondition | SDK_BOUNDARY_MISMATCH |         | SDK 版本不匹配                                | 此应用的部分组件不同步。请重新加载页面后重试。                                                                                  |

    Examples: the platform code of the Connect code
      | locale | connect_code      | code         | emitter | text          | hint                                                                     |
      | en     | permission_denied | NOT_DECLARED | acme    | Access denied | You don't have permission to perform this action.                        |
      | en     | permission_denied | ARCHIVED     | billing | Access denied | You don't have permission to perform this action.                        |
      | en     | permission_denied | ARCHIVED     |         | Access denied | You don't have permission to perform this action.                        |
      | en     | aborted           | archived     | acme    | Conflict      | This change conflicts with another update. Please refresh and try again. |

  Scenario Outline: An error without ErrorInfo renders the platform code of its Connect code
    Given a Connect error "<connect_code>" with no ErrorInfo
    When the error is localized for "en"
    Then the localized text is "<text>"
    And the localized hint is "<hint>"

    Examples:
      | connect_code        | text                      | hint                                                                        |
      | canceled            | Server error              | Something went wrong on our end. Please try again later.                    |
      | unknown             | Server error              | Something went wrong on our end. Please try again later.                    |
      | invalid_argument    | Validation failed         | Please review the highlighted fields and try again.                         |
      | deadline_exceeded   | Gateway timeout           | none                                                                        |
      | not_found           | Not found                 | none                                                                        |
      | already_exists      | Already exists            | none                                                                        |
      | permission_denied   | Access denied             | You don't have permission to perform this action.                           |
      | resource_exhausted  | Too many requests         | You're going too fast. Please slow down and try again.                      |
      | failed_precondition | Out of date               | The resource has changed since you loaded it. Please refresh and try again. |
      | aborted             | Conflict                  | This change conflicts with another update. Please refresh and try again.    |
      | out_of_range        | Unable to process request | none                                                                        |
      | unimplemented       | Not implemented           | none                                                                        |
      | internal            | Server error              | Something went wrong on our end. Please try again later.                    |
      | unavailable         | Service unavailable       | The service is temporarily unavailable. Please try again later.             |
      | data_loss           | Server error              | Something went wrong on our end. Please try again later.                    |
      | unauthenticated     | Sign in required          | Your session has expired. Please sign in again.                             |

  Scenario Outline: The viewer's locale is negotiated over the platform locales
    Given a Connect error "failed_precondition" whose ErrorInfo has code "ARCHIVED", emitter "acme" and args "(none)"
    When the error is localized for "<locale>"
    Then the localized text is "<text>"

    Examples: one tag
      | locale     | text                                    |
      | fr         | Cet espace de travail est archivé.      |
      | fr-CA      | Cet espace de travail est archivé.      |
      | FR_ca      | Cet espace de travail est archivé.      |
      | en-GB      | This workspace is archived.             |
      | nb         | Dette arbeidsområdet er arkivert.       |
      | no         | Dette arbeidsområdet er arkivert.       |
      | nn         | Dette arbeidsområdet er arkivert.       |
      | he         | סביבת העבודה הזו נמצאת בארכיון.         |
      | he-IL      | סביבת העבודה הזו נמצאת בארכיון.         |
      | iw         | סביבת העבודה הזו נמצאת בארכיון.         |
      | pt         | Este espaço de trabalho está arquivado. |
      | pt-PT      | Este espaço de trabalho está arquivado. |
      | zh         | 此工作区已归档。                        |
      | zh-CN      | 此工作区已归档。                        |
      | zh-SG      | 此工作区已归档。                        |
      | zh-Hans-HK | 此工作区已归档。                        |
      | zh-TW      | This workspace is archived.             |
      | zh-Hant    | This workspace is archived.             |
      | ar         | This workspace is archived.             |
      | xx         | This workspace is archived.             |
      | *          | This workspace is archived.             |
      |            | This workspace is archived.             |

    # The first platform locale an Accept-Language list prefers wins even when
    # the emitter has no string in it: the fallback is English, never the
    # list's next entry.
    Examples: an Accept-Language list
      | locale                | text                                    |
      | zh-Hant, fr;q=0.5     | Cet espace de travail est archivé.      |
      | fr;q=0, he;q=0.2      | סביבת העבודה הזו נמצאת בארכיון.         |
      | he;q=0.5, fr;q=0.5    | סביבת העבודה הזו נמצאת בארכיון.         |
      | nb;q=0.5, pt;q=0.9    | Este espaço de trabalho está arquivado. |
      | de, fr;q=0.8          | This workspace is archived.             |
      | xx, yy                | This workspace is archived.             |

  Scenario Outline: Plural branches and numbers follow the viewer's locale
    Given a Connect error "resource_exhausted" whose ErrorInfo has code "SEATS_EXHAUSTED", emitter "acme" and args "<args>"
    When the error is localized for "en"
    Then the localized text is "<text>"
    And the localized hint is "Remove a member or upgrade the plan."

    Examples:
      | args                            | text                                          |
      | count=int64:0                   | No seats left                                 |
      | count=int64:1                   | Only 1 seat left                              |
      | count=int64:3                   | Only 3 seats left                             |
      | count=int64:1234                | Only 1,234 seats left                         |
      | count=double:0                  | No seats left                                 |
      | count=double:1                  | Only 1 seat left                              |
      | count=double:1.5                | Only 1.5 seats left                           |

  Scenario Outline: An int64 beyond 2^53 renders exactly
    Given a Connect error "resource_exhausted" whose ErrorInfo has code "SEATS_EXHAUSTED", emitter "acme" and args "<args>"
    When the error is localized for "en"
    Then the localized text is "<text>"

    Examples:
      | args                            | text                                          |
      | count=int64:9007199254740992    | Only 9,007,199,254,740,992 seats left         |
      | count=int64:9007199254740993    | Only 9,007,199,254,740,993 seats left         |
      | count=int64:-9007199254740993   | Only -9,007,199,254,740,993 seats left        |
      | count=int64:9223372036854775807 | Only 9,223,372,036,854,775,807 seats left     |

  Scenario Outline: Select branches match the string argument exactly
    Given a Connect error "permission_denied" whose ErrorInfo has code "PLAN_LOCKED", emitter "acme" and args "<args>"
    When the error is localized for "en"
    Then the localized text is "<text>"

    Examples:
      | args                   | text                            |
      | plan=string:team       | The team plan is locked.        |
      | plan=string:enterprise | The enterprise plan is locked.  |
      | plan=string:free       | This plan is locked.            |
      | plan=string:TEAM       | This plan is locked.            |
      | plan=string:           | This plan is locked.            |

  Scenario Outline: Numbers round half away from zero
    Given a Connect error "resource_exhausted" whose ErrorInfo has code "<code>", emitter "acme" and args "<args>"
    When the error is localized for "en"
    Then the localized text is "<text>"

    Examples:
      | code       | args                                                                                  | text                                                                              |
      | QUOTA_USED | used=int64:1234; limit=int64:5000; ratio=double:0.25                                  | 1,234 of 5,000 requests used (25%).                                               |
      | QUOTA_USED | used=int64:1; limit=int64:8; ratio=double:0.125                                       | 1 of 8 requests used (13%).                                                       |
      | QUOTA_USED | used=int64:-9223372036854775808; limit=int64:9223372036854775807; ratio=double:1      | -9,223,372,036,854,775,808 of 9,223,372,036,854,775,807 requests used (100%).     |
      | RATE_SET   | rate=double:2.0625                                                                    | Limited to 2.063 requests a second.                                               |
      | RATE_SET   | rate=double:1234.5                                                                    | Limited to 1,234.5 requests a second.                                             |
      | RATE_SET   | rate=int64:3                                                                          | Limited to 3 requests a second.                                                   |

  Scenario Outline: A timestamp renders in a fixed date or time style
    Given a Connect error "failed_precondition" whose ErrorInfo has code "<code>", emitter "acme" and args "at=timestamp:2026-11-20T14:05:00Z"
    When the error is localized for "en"
    Then the localized text is "<text>"

    Examples:
      | code          | text                              |
      | RENEWS_SHORT  | Renews 11/20/26.                  |
      | RENEWS_MEDIUM | Renews Nov 20, 2026.              |
      | RENEWS_LONG   | Renews November 20, 2026.         |
      | RENEWS_FULL   | Renews Friday, November 20, 2026. |
      | RENEWS_AT     | Renews at 2:05:00 PM.             |

  Scenario Outline: A timestamp renders in the requested time zone, UTC by default
    Given a Connect error "failed_precondition" whose ErrorInfo has code "TRIAL_ENDED", emitter "acme" and args "ended_at=timestamp:2026-11-20T14:05:00Z"
    When the error is localized for "en" in time zone "<zone>"
    Then the localized text is "<text>"

    Examples:
      | zone                | text                                          |
      | (none)              | Your trial ended on Nov 20, 2026 at 2:05 PM.  |
      | UTC                 | Your trial ended on Nov 20, 2026 at 2:05 PM.  |
      | Asia/Tokyo          | Your trial ended on Nov 20, 2026 at 11:05 PM. |
      | Pacific/Kiritimati  | Your trial ended on Nov 21, 2026 at 4:05 AM.  |
      | America/Los_Angeles | Your trial ended on Nov 20, 2026 at 6:05 AM.  |

  Scenario Outline: An unknown time zone is refused
    Given a Connect error "failed_precondition" whose ErrorInfo has code "ARCHIVED", emitter "acme" and args "(none)"
    When the error is localized for "en" in time zone "<zone>"
    Then the localization is refused with rule "unknown_time_zone"

    Examples:
      | zone         |
      | Mars/Olympus |
      |              |

  Scenario Outline: Arguments are values, never patterns
    Given a Connect error "already_exists" whose ErrorInfo has code "<code>", emitter "acme" and args "<args>"
    When the error is localized for "en"
    Then the localized text is "<text>"

    Examples:
      | code           | args                                   | text                                                   |
      | NAME_TAKEN     | name=string:Ada                        | Use <b>Ada</b> as the name.                            |
      | NAME_TAKEN     | name=string:{count, plural, other {#}} | Use <b>{count, plural, other {#}}</b> as the name.     |
      | NAME_TAKEN     | name=string:<script>x</script>         | Use <b><script>x</script></b> as the name.             |
      | INVITE_BLOCKED | name=string:O'Brien                    | You can't invite O'Brien.                              |
      | CONFIRM_NAME   | name=string:Ada                        | Type {name} to confirm.                                |
      | GREETING       | name=string:it''s '{x}'                | Hello it''s '{x}'.                                     |

  Scenario Outline: An entry that cannot render with the error's arguments falls through to the next key
    Given a Connect error "failed_precondition" whose ErrorInfo has code "<code>", emitter "acme" and args "<args>"
    When the error is localized for "en"
    Then the localized text is "Out of date"
    And the localized hint is "The resource has changed since you loaded it. Please refresh and try again."

    Examples: a missing or mistyped argument
      | code            | args                                |
      | GREETING        | (none)                              |
      | GREETING        | name=int64:3                        |
      | SEATS_EXHAUSTED | count=string:3                      |
      | SEATS_EXHAUSTED | count=timestamp:2026-11-20T14:05:00Z |
      | PLAN_LOCKED     | plan=int64:1                        |
      | RATE_SET        | rate=string:2                       |
      | TRIAL_ENDED     | ended_at=int64:3                    |

    Examples: a pattern outside the supported subset
      | code          | args                             |
      | BROKEN_PLURAL | count=int64:1                    |
      | ORDINAL       | count=int64:1                    |
      | CURRENCY      | amount=double:1                  |
      | DATE_SKELETON | at=timestamp:2026-11-20T14:05:00Z |
      | TIME_LONG     | at=timestamp:2026-11-20T14:05:00Z |
      | SPELLOUT      | count=int64:1                    |

  Scenario Outline: The SDK builds only arguments within bounds
    When the emitter builds ErrorInfo code "SEATS_EXHAUSTED" with args "<args>"
    Then the construction succeeds

    Examples:
      | args                                    |
      | args(32)                                |
      | a=int64:1                               |
      | repeat(a,64)=int64:1                    |
      | name=string:repeat(a,1024)              |
      | name=string:repeat(é,512)               |
      | name=string:café 東京                   |
      | count=int64:-9223372036854775808        |
      | ratio=double:-0                         |
      | at=timestamp_raw:253402300799:999999999 |
      | at=timestamp_raw:-62135596800:0         |

  # In a table cell `\\` is one backslash, so `\\u0007` is the text `\u0007`,
  # which the argument grammar above expands to U+0007.
  Scenario Outline: The SDK refuses an argument out of bounds with a typed rule
    When the emitter builds ErrorInfo code "SEATS_EXHAUSTED" with args "<args>"
    Then the construction is refused with rule "<rule>"

    Examples:
      | args                              | rule                  |
      | args(33)                          | too_many_args         |
      | Count=int64:1                     | arg_key_format        |
      | _count=int64:1                    | arg_key_format        |
      | 9count=int64:1                    | arg_key_format        |
      | count-x=int64:1                   | arg_key_format        |
      | repeat(a,65)=int64:1              | arg_key_format        |
      | name=string:repeat(a,1025)        | string_too_long       |
      | name=string:repeat(é,513)         | string_too_long       |
      | name=string:a\\u0007b              | string_forbidden_char |
      | name=string:a\\u000ab              | string_forbidden_char |
      | name=string:a\\u007fb              | string_forbidden_char |
      | name=string:a\\u0085b              | string_forbidden_char |
      | name=string:a؜b              | string_forbidden_char |
      | name=string:a‏b              | string_forbidden_char |
      | name=string:a‮b              | string_forbidden_char |
      | name=string:a⁦b              | string_forbidden_char |
      | ratio=double:NaN                  | non_finite_double     |
      | ratio=double:inf                  | non_finite_double     |
      | ratio=double:-inf                 | non_finite_double     |
      | at=timestamp_raw:253402300800:0   | timestamp_range       |
      | at=timestamp_raw:-62135596801:0   | timestamp_range       |
      | at=timestamp_raw:0:1000000000     | timestamp_range       |
      | at=timestamp_raw:0:-1             | timestamp_range       |

    Examples: the first failing rule wins, in the same order in both construction layers
      | args                                          | rule                  |
      | Count=string:a\\u0007b                        | arg_key_format        |
      | Count=double:NaN                              | arg_key_format        |
      | args(32); extra=int64:1                       | too_many_args         |
      | args(32); Extra=string:a\\u0007b               | too_many_args         |
      | name=string:a\\u0007b; ratio=double:NaN        | string_forbidden_char |
      | ratio=double:NaN; name=string:a\\u0007b        | non_finite_double     |
      | count=int64:1; Count=int64:2                  | arg_key_format        |

  Scenario Outline: A received error with arguments out of bounds renders as if it had no ErrorInfo
    Given a Connect error "permission_denied" whose unchecked ErrorInfo has code "SEATS_EXHAUSTED", emitter "acme" and args "<args>"
    When the error is localized for "en"
    Then the localized text is "<text>"

    # Each refused row carries a valid count, so only the bound check keeps it
    # from rendering "Only 3 seats left".
    Examples:
      | args                                         | text              |
      | count=int64:3                                | Only 3 seats left |
      | count=int64:3; args(32)                      | Access denied     |
      | count=int64:3; Count=int64:3                 | Access denied     |
      | count=int64:3; name=string:repeat(a,1025)    | Access denied     |
      | count=int64:3; name=string:a‮b          | Access denied     |
      | count=int64:3; ratio=double:NaN              | Access denied     |
      | count=int64:3; at=timestamp_raw:0:1000000000 | Access denied     |

  Scenario: Wire bytes written by another implementation render
    Given a Connect error "resource_exhausted" whose ErrorInfo detail is "Cg9TRUFUU19FWEhBVVNURUQSBGFjbWUiCwoFY291bnQSAhAD"
    When the error is localized for "en"
    Then the localized text is "Only 3 seats left"
    And the localized hint is "Remove a member or upgrade the plan."

  Scenario: A timestamp argument written by another implementation renders
    Given a Connect error "failed_precondition" whose ErrorInfo detail is "CgtUUklBTF9FTkRFRBIEYWNtZSIUCghlbmRlZF9hdBIIIgYIjKeB2AY"
    When the error is localized for "en"
    Then the localized text is "Your trial ended on Nov 20, 2026 at 2:05 PM."

  Scenario: A Connect error's validation failures render one by one
    Given a Connect error "invalid_argument" whose ValidationFailure detail is "CgYvc2VhdHMSEm11c3QgYmUgYXQgbW9zdCAxMBoDbWF4IgkKA21heBICEAo"
    When the violations of the error are read
    Then there is 1 violation
    And violation 1 has field "/seats" and rule "max"
    When violation 1 is localized for "en"
    Then the localized text is "Must be at most 10."
    And the localized hint is "none"

  Scenario: A bare ValidationFailure renders
    Given a ValidationFailure whose wire value is "CgYvc2VhdHMSEm11c3QgYmUgYXQgbW9zdCAxMBoDbWF4IgkKA21heBICEAo"
    When the error is localized for "en"
    Then the localized text is "Must be at most 10."

  Scenario Outline: A validation failure without its own string renders the platform validation text
    Given a ValidationFailure with field "/seats", rule "<rule>" and unchecked args "<args>"
    When the error is localized for "en"
    Then the localized text is "<text>"
    And the localized hint is "<hint>"

    Examples:
      | rule        | args               | text                | hint                                                |
      | max         | max=int64:10       | Must be at most 10. | none                                                |
      | zzz_unknown | (none)             | Validation failed   | Please review the highlighted fields and try again. |
      | max         | (none)             | Validation failed   | Please review the highlighted fields and try again. |
      | max         | max=string:10      | Validation failed   | Please review the highlighted fields and try again. |
      | max         | max=double:NaN     | Validation failed   | Please review the highlighted fields and try again. |
      |             | (none)             | Validation failed   | Please review the highlighted fields and try again. |

  Scenario Outline: A problem+json body renders from its error_info member, else its type, else its status
    Given the problem+json body from fixture "problem/<fixture>"
    When the error is localized for "en"
    Then the localized text is "<text>"
    And the localized hint is "<hint>"
    And the localized text does not contain "seat limit reached"

    Examples: the error_info member in the proto3 JSON mapping
      | fixture                 | text                                         | hint                                 |
      | seats_exhausted.json    | Only 3 seats left                            | Remove a member or upgrade the plan. |
      | seats_beyond_2_53.json  | Only 9,007,199,254,740,993 seats left        | Remove a member or upgrade the plan. |
      | trial_ended.json        | Your trial ended on Nov 20, 2026 at 2:05 PM. | none                                 |
      | trial_ended_offset.json | Your trial ended on Nov 20, 2026 at 2:05 PM. | none                                 |
      | quota_used.json         | 1,234 of 5,000 requests used (25%).          | none                                 |
      | plan_locked.json        | The team plan is locked.                     | none                                 |

    Examples: a malformed error_info member is ignored
      | fixture                 | text           | hint |
      | int64_as_number.json    | Already exists | none |
      | two_value_members.json  | Already exists | none |
      | non_finite_double.json  | Already exists | none |
      | double_as_string.json   | Already exists | none |

    Examples: no error_info member
      | fixture                  | text              | hint                                                     |
      | type_urn_only.json       | Not found         | none                                                     |
      | type_base_url_only.json  | Too many requests | You're going too fast. Please slow down and try again.   |
      | status_only.json         | Access denied     | You don't have permission to perform this action.        |
      | status_unlisted_4xx.json | Invalid request   | none                                                     |
      | status_unlisted_5xx.json | Server error      | Something went wrong on our end. Please try again later. |
      | validation_failed.json   | Validation failed | Please review the highlighted fields and try again.      |

  Scenario: A problem+json body's validation errors render one by one
    Given the problem+json body from fixture "problem/validation_failed.json"
    When the violations of the error are read
    Then there are 2 violations
    And violation 1 has field "/seats" and rule "max"
    And violation 2 has field "/name" and rule "zzz_unknown"
    When violation 1 is localized for "en"
    Then the localized text is "Must be at most 10."
    When violation 2 is localized for "en"
    Then the localized text is "Validation failed"

  Scenario Outline: A catalog registered twice with different entries is not used for that locale
    Given a Connect error "failed_precondition" whose ErrorInfo has code "ARCHIVED", emitter "acme-dup" and args "(none)"
    When the error is localized for "<locale>"
    Then the localized text is "<text>"

    Examples:
      | locale | text                               |
      | en     | Out of date                        |
      | fr     | Cet espace de travail est archivé. |

  # The platform strings come from the rendering package itself: a registered
  # catalog for emitter "bones" is ignored, and a registered catalog supplies
  # only keys under its own emitter's prefix.
  Scenario Outline: A registered catalog supplies only its own emitter's keys
    Given a Connect error "<connect_code>" whose ErrorInfo has code "<code>", emitter "<emitter>" and args "(none)"
    When the error is localized for "en"
    Then the localized text is "<text>"

    Examples:
      | connect_code        | code               | emitter     | text                        |
      | failed_precondition | HIJACK             | acme-hijack | Prefixed entry.             |
      | permission_denied   | FORBIDDEN          | acme-hijack | Access denied               |
      | failed_precondition | ARCHIVED           | acme        | This workspace is archived. |
      | not_found           | RESOURCE_NOT_FOUND | bones       | Not found                   |
      | not_found           | NOT_DECLARED       | billing     | Not found                   |

  # tests/emitter-catalogs/catalogs is the source both build helpers embed: the
  # Rust crate's `embed_and_check_error_catalogs` and the TypeScript package's
  # `embed-error-catalogs` bin. Every emitter that owns keys in the directory
  # renders from it in every locale the directory holds.
  Scenario Outline: A catalog embedded by the build helper renders under each emitter that owns keys in it
    Given the catalogs the build helper embeds are registered
    And a Connect error "<connect_code>" whose ErrorInfo has code "<code>", emitter "<emitter>" and args "<args>"
    When the error is localized for "<locale>"
    Then the localized text is "<text>"

    Examples:
      | locale | connect_code       | code   | emitter     | args             | text                |
      | en     | resource_exhausted | LIMIT  | acme-origin | limit=int64:1234 | Limit 1,234 reached |
      | fr     | resource_exhausted | LIMIT  | acme-origin | limit=int64:3    | Limite 3 atteinte   |
      | en     | permission_denied  | DENIED | acme-edge   | seat=string:A1   | No seat A1          |
      | fr     | permission_denied  | DENIED | acme-edge   | seat=string:A1   | Pas de place A1     |

  Scenario: The platform locales and aliases are core-ui's
    Given the platform locale registry from fixture "platform_locales.json"
    When each of its locales and aliases is negotiated
    Then each negotiates to its registry locale and the empty request to the default locale
    And this language's platform locales, aliases and default locale equal it

  Scenario: Formatting data is the pinned CLDR release
    Given a Connect error "failed_precondition" whose ErrorInfo has code "RENEWS_FULL", emitter "acme" and args "at=timestamp:2026-11-20T14:05:00Z"
    And the CLDR release pinned in "api-bones-i18n/cldr.json"
    When the error is localized for "en"
    Then the localized text is "Renews Friday, November 20, 2026."
    And this language's formatting data is that CLDR release
