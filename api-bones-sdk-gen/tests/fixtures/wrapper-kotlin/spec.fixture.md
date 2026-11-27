---
service: api-bones
sdk_surfaces:
  sdk-call:
    l2_methods:
      contract.v1.NumbersService/ListNoSideEffects:
        rust: [list]
      contract.v1.WidgetsService/ListWidgetParts:
        rust: [list_widget_parts]
      contract.v1.WidgetsService/GetWidget:
        rust: [get_widget]
      contract.v1.WidgetsService/BatchGetWidgets:
        rust: [batch_get_widgets]
      contract.v1.EventsService/Watch:
        rust: [watch]
    targets:
      kotlin:
        delivery: package
        packages: [com.brefwiz.apibones.testservice]
---
Fixture SPEC of the Kotlin wrapper golden test.
