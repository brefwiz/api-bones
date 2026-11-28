// SPDX-License-Identifier: MIT
// The `node` entry of the package: the runtime, and the exchange the wasm core sends its requests through.
import { installNodeExchange } from "./node-exchange.js";

installNodeExchange();

export * from "./index.js";
export type { Exchange, ExchangeConnect, ExchangeRequest, ExchangeResponse, NodeExchange, TlsMaterial } from "./node-exchange.js";
