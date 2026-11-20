// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/features/connect_map_list_call.feature`.
 *
 * Serves pages through the package's list call and maps them, reading the
 * result back through the same public surface a caller uses.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";
import { Code, ConnectError } from "@connectrpc/connect";
import { listCall, mapListCall, PageError, Total_Kind, type ListCall, type Page } from "@brefwiz/api-bones-connect";

interface MapWorld extends World {
  served: number;
  source?: ListCall<number>;
  mappedItems?: number[];
  mappedPages?: Page<number>[];
  mappedSpans?: unknown[];
  sourceSpans?: unknown[];
  mappedFirst?: Page<number>;
  failure?: unknown;
}

const parse = (text: string): number[] => (text === "" ? [] : text.split(",").map(Number));

function serve(world: MapWorld, pages: number[][], failAt?: number): void {
  world.served = 0;
  world.source = listCall<number>(
    async (request) => {
      const cursor = String((request.page as { cursor?: string }).cursor ?? "");
      const index = cursor === "" ? 0 : Number(cursor);
      world.served++;
      if (index === failAt) throw new ConnectError("page unavailable", Code.Unavailable);
      return { items: pages[index], page: { nextCursor: index + 1 < pages.length ? String(index + 1) : "", revision: `r${index}`, total: { value: 3n, kind: Total_Kind.EXACT } } };
    },
    {},
    { method: "/contract.v1.Numbers/ListNumbers", paging: { kind: "unary", itemField: "items", pageField: "page", requestPageField: "page" } },
    { prefetch: 0, cache: "bypass" },
  );
}

Given("a source of pages {string} {string} served through a list call", function (this: MapWorld, first: string, second: string) {
  serve(this, [parse(first), parse(second)]);
});
Given("a source of pages {string} served through a list call", function (this: MapWorld, only: string) {
  serve(this, [parse(only)]);
});
Given("a source of pages {string} {string} served through a list call whose second page fails", function (this: MapWorld, first: string, second: string) {
  serve(this, [parse(first), parse(second)], 1);
});

const double = (item: number) => item * 2;

async function collect<T>(iteration: AsyncIterable<T>): Promise<{ values: T[]; error?: unknown }> {
  const values: T[] = [];
  try {
    for await (const value of iteration) values.push(value);
  } catch (error) {
    return { values, error };
  }
  return { values };
}

When("the caller maps each item to its doubled value and reads the items", async function (this: MapWorld) {
  const result = await collect(mapListCall(this.source!, double).items());
  this.mappedItems = result.values;
  this.failure = result.error;
});
/** Reads every page, noting the mapped and source spans live at each delivery. */
async function readPages(mapped: ListCall<number>, source: ListCall<number>) {
  const values: Page<number>[] = [];
  const mappedSpans: unknown[] = [];
  const sourceSpans: unknown[] = [];
  for await (const page of mapped.pages()) {
    values.push(page);
    mappedSpans.push(mapped.span);
    sourceSpans.push(source.span);
  }
  return { values, mappedSpans, sourceSpans };
}

When("the caller maps each item to its doubled value and reads the pages", async function (this: MapWorld) {
  const read = await readPages(mapListCall(this.source!, double), this.source!);
  this.mappedPages = read.values;
  this.mappedSpans = read.mappedSpans;
  this.sourceSpans = read.sourceSpans;
});
When("the caller maps each item to its doubled value and reads the first page", async function (this: MapWorld) {
  this.mappedFirst = await mapListCall(this.source!, double).firstPage();
});

Then("the mapped items are {string}", function (this: MapWorld, expected: string) {
  assert.deepEqual(this.mappedItems, parse(expected));
});
Then("the mapped pages hold {string} and {string}", function (this: MapWorld, first: string, second: string) {
  assert.deepEqual(this.mappedPages?.map((page) => [...page.items]), [parse(first), parse(second)]);
});
Then("every mapped page keeps the revision of its source page", function (this: MapWorld) {
  assert.deepEqual(this.mappedPages?.map((page) => page.revision), ["r0", "r1"]);
});
Then("every mapped page keeps the total of its source page", function (this: MapWorld) {
  assert.deepEqual(this.mappedPages?.map((page) => page.total), [{ value: 3n, kind: "exact" }, { value: 3n, kind: "exact" }]);
});
Then("no mapped page is reported as read from the cache", function (this: MapWorld) {
  assert.deepEqual(this.mappedPages?.map((page) => page.fromCache), [false, false]);
});
Then("the mapped call reports the span of its source call", function (this: MapWorld) {
  assert.ok(this.mappedSpans && this.mappedSpans.length > 0, "pages were read");
  assert.ok(this.sourceSpans?.every((span) => span !== undefined), "the source call has a span while it is read");
  assert.deepEqual(this.mappedSpans, this.sourceSpans);
});
Then("only the first mapped page reports a next page", function (this: MapWorld) {
  assert.deepEqual(this.mappedPages?.map((page) => page.hasNext), [true, false]);
});
Then("the first mapped page holds {string} and reports a next page", function (this: MapWorld, expected: string) {
  assert.deepEqual([...this.mappedFirst!.items], parse(expected));
  assert.equal(this.mappedFirst!.hasNext, true);
});
Then("the first mapped page holds no items and reports no next page", function (this: MapWorld) {
  assert.deepEqual([...this.mappedFirst!.items], []);
  assert.equal(this.mappedFirst!.hasNext, false);
});
Then("the server was asked for {int} page", function (this: MapWorld, count: number) {
  assert.equal(this.served, count);
});
Then("the read ends with a page error after {int} pages and {int} items", function (this: MapWorld, pages: number, items: number) {
  assert.ok(this.failure instanceof PageError, "the failure is a page error");
  assert.equal(this.failure.deliveredPages, pages);
  assert.equal(this.failure.deliveredItems, items);
});
