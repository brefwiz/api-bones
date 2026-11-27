// SPDX-License-Identifier: MIT
// The uniffi bindgen output surface of the test service's
// Layer 2 crate: one object per client and one function per export.
import Foundation

open class NumbersClientFfi: @unchecked Sendable {
    public init(baseUrl: String, host: FfiHost, options: Data) throws {}
}

open class WidgetsClientFfi: @unchecked Sendable {
    public init(baseUrl: String, host: FfiHost, options: Data) throws {}
}

open class EventsClientFfi: @unchecked Sendable {
    public init(baseUrl: String, host: FfiHost, options: Data) throws {}
}

public func apiBonesNumbersClientList(client: NumbersClientFfi, req: Data) throws -> FfiHandle {
    FfiHandle()
}

public func apiBonesNumbersClientMaxPageSize() throws -> Data {
    Data()
}

public func apiBonesNumbersClientClamp(req: Data) throws -> Data {
    Data()
}

public func apiBonesWidgetsClientGetWidget(client: WidgetsClientFfi, req: Data) async throws -> Data {
    Data()
}

public func apiBonesWidgetsClientListWidgetParts(client: WidgetsClientFfi, req: Data) throws -> FfiHandle {
    FfiHandle()
}

public func apiBonesWidgetsClientBatchGetWidgets(client: WidgetsClientFfi, req: Data) throws -> FfiHandle {
    FfiHandle()
}

public func apiBonesEventsClientWatch(client: EventsClientFfi, req: Data) async throws -> FfiHandle {
    FfiHandle()
}
