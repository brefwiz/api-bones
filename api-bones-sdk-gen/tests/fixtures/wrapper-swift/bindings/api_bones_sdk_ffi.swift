// SPDX-License-Identifier: MIT
// The declarations of the uniffi bindgen output of the boundary crate that
// the wrapper calls; the generator checks the package against them.
import Foundation

public enum FfiError: Swift.Error {
    case call(bytes: Data)
}

public protocol FfiHost: AnyObject {
    func credential(request: Data) async throws -> Data
    func anonymous(request: Data) async throws -> Data
    func span(event: Data)
}

public protocol FfiHandleProtocol: AnyObject {
    func next() async throws -> Data?
    func cancel()
}

open class FfiHandle: FfiHandleProtocol, @unchecked Sendable {
    open func next() async throws -> Data? { nil }
    open func cancel() {}
}

public func formatError(err: Data, locale: String, timeZone: String?) -> Data {
    Data()
}
