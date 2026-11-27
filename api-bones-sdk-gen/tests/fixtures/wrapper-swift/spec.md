---
# The sdk-call surface of the test service, with the Swift package its
# exports are wrapped by.
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
      swift:
        delivery: package
        packages: [ApiBonesTestService]
---
