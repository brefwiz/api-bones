import { describe, it, expect } from "vitest";
import {
  addEnvelopeUnwrapInterceptor,
  getEnvelopeMeta,
  getEnvelopeLinks,
  type AxiosLikeInstance,
  type EnvelopeAxiosResponse,
} from "./index";

function makeInstance() {
  let handler: ((r: EnvelopeAxiosResponse) => EnvelopeAxiosResponse) | null = null;
  const instance: AxiosLikeInstance = {
    interceptors: {
      response: {
        use(onFulfilled) {
          handler = onFulfilled as typeof handler;
          return 0;
        },
        eject() {},
      },
    },
  };
  return {
    instance,
    intercept(response: EnvelopeAxiosResponse): EnvelopeAxiosResponse {
      return handler!(response);
    },
  };
}

describe("addEnvelopeUnwrapInterceptor", () => {
  it("unwraps data and stashes meta + links", () => {
    const { instance, intercept } = makeInstance();
    addEnvelopeUnwrapInterceptor(instance);

    const meta = { request_id: "req-1", timestamp: "2024-01-01T00:00:00Z" };
    const links = { self: { rel: "self", href: "/items/1" } };
    const payload = { id: "1", name: "item" };

    const result = intercept({
      data: { data: payload, meta, links },
      status: 200,
      config: {},
    });

    expect(result.data).toEqual(payload);
    expect(getEnvelopeMeta(result.config!)).toEqual(meta);
    expect(getEnvelopeLinks(result.config!)).toEqual(links);
  });

  it("passes non-envelope responses through unchanged", () => {
    const { instance, intercept } = makeInstance();
    addEnvelopeUnwrapInterceptor(instance);

    const plain = { id: "1" };
    const result = intercept({ data: plain, status: 200 });
    expect(result.data).toEqual(plain);
  });

  it("returns null meta/links when response was not an envelope", () => {
    const { instance, intercept } = makeInstance();
    addEnvelopeUnwrapInterceptor(instance);

    const result = intercept({ data: { id: "1" }, status: 200, config: {} });
    expect(getEnvelopeMeta(result.config!)).toBeNull();
    expect(getEnvelopeLinks(result.config!)).toBeNull();
  });
});
