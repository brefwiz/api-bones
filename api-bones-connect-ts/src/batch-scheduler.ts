// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
export const MAX_WIRE_IN_FLIGHT = 4;
export const MAX_QUEUED_CHUNKS = 64;
interface Job { run(): Promise<unknown>; resolve(value: unknown): void; reject(error: unknown): void; signal?: AbortSignal; abort(): void; }
export interface BatchQueueIdentity { readonly caller: string; readonly credentialHash: string; }
/** One fair queue shared by explicit chunks and coalescing windows. */
export class BatchScheduler {
  private readonly queues = new Map<BatchQueueIdentity, Job[]>();
  private readonly ready: BatchQueueIdentity[] = [];
  private active = 0;
  submit<T>(key: BatchQueueIdentity, run: () => Promise<T>, signal?: AbortSignal): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      if (signal?.aborted) { reject(new ConnectError("batch cancelled", Code.Canceled)); return; }
      const identity = Array.from(this.queues.keys()).find(candidate => candidate.caller === key.caller && candidate.credentialHash === key.credentialHash) ?? { ...key };
      const queue = this.queues.get(identity) ?? [];
      if (queue.length >= MAX_QUEUED_CHUNKS) { reject(new ConnectError("batch queue exhausted", Code.ResourceExhausted)); return; }
      const job: Job = { run, resolve: value => resolve(value as T), reject, signal, abort: () => {
        const pending = this.queues.get(identity);
        const index = pending?.indexOf(job) ?? -1;
        if (index >= 0) { pending!.splice(index, 1); reject(new ConnectError("batch cancelled", Code.Canceled)); }
      } };
      signal?.addEventListener("abort", job.abort, { once: true });
      queue.push(job);
      if (!this.queues.has(identity)) { this.queues.set(identity, queue); this.ready.push(identity); }
      this.drain();
    });
  }
  private drain(): void {
    while (this.active < MAX_WIRE_IN_FLIGHT && this.ready.length) {
      const identity = this.ready.shift()!;
      const queue = this.queues.get(identity)!;
      const job = queue.shift();
      if (queue.length) this.ready.push(identity); else this.queues.delete(identity);
      if (!job) continue;
      job.signal?.removeEventListener("abort", job.abort);
      if (job.signal?.aborted) { job.reject(new ConnectError("batch cancelled", Code.Canceled)); continue; }
      this.active++;
      Promise.resolve().then(job.run).then(job.resolve, job.reject).finally(() => { this.active--; this.drain(); });
    }
  }
}
