// SPDX-License-Identifier: MIT
import type { Page } from "./page.js";

interface Chain {
  revision: string;
  pages: WeakRef<Page<unknown>>[];
  complete: boolean;
}

/** Per-client, holder-bound cache. Identity changes discard every old chain. */
export class RevisionCache {
  private identity?: string;
  private generation = 0;
  private readonly chains = new Map<string, Chain>();

  constructor(private readonly identityKey: () => string) {}

  clear(): void {
    this.chains.clear();
    this.generation++;
  }

  isEmpty(): boolean {
    return this.chains.size === 0;
  }

  key(method: string, requestHash: string): string {
    const identity = this.identityKey();
    if (this.identity !== identity) {
      this.clear();
      this.identity = identity;
    }
    return JSON.stringify([this.generation, identity, method, requestHash]);
  }

  replay<T>(key: string): Page<T>[] | undefined {
    const chain = this.chains.get(key);
    if (!chain?.complete) return undefined;
    const pages = chain.pages.map((ref) => ref.deref());
    if (pages.some((page) => page === undefined || page.revision !== chain.revision)) {
      this.chains.delete(key);
      return undefined;
    }
    return pages as Page<T>[];
  }

  begin(key: string): void {
    this.chains.set(key, { revision: "", pages: [], complete: false });
  }

  keep<T>(key: string, page: Page<T>, complete: boolean): void {
    const chain = this.chains.get(key);
    if (!chain) return;
    if (chain.pages.length === 0) chain.revision = page.revision;
    if (!page.revision || page.revision !== chain.revision) {
      this.chains.delete(key);
      return;
    }
    chain.pages.push(new WeakRef(page as Page<unknown>));
    chain.complete = complete;
  }
}
