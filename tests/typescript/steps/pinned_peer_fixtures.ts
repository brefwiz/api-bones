// SPDX-License-Identifier: MIT
/**
 * Real loopback TLS against a test CA, shared by the pinned-peer step lane and
 * the package's own tests. Nothing about the handshake is stubbed.
 */

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:https";
import type { AddressInfo } from "node:net";
import { join } from "node:path";

export interface Authority {
  cert: string;
  key: string;
}

export interface Leaf {
  cert: string;
  key: string;
}

let counter = 0;

function openssl(...args: string[]): void {
  execFileSync("openssl", args, { stdio: "ignore", timeout: 30_000 });
}

function newRequest(key: string, csr: string, commonName: string): void {
  openssl(
    "req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
    "-nodes", "-keyout", key, "-out", csr, "-subj", `/CN=${commonName}`,
  );
}

export function authority(dir: string): Authority {
  const name = `ca${counter++}`;
  const key = join(dir, `${name}.key`);
  const cert = join(dir, `${name}.crt`);
  openssl(
    "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
    "-nodes", "-keyout", key, "-out", cert, "-days", "1", "-subj", `/CN=test-ca-${name}`,
    "-addext", "basicConstraints=critical,CA:TRUE",
  );
  return { cert, key };
}

/**
 * A leaf issued by `ca` naming exactly `uris` as URI SANs (none when empty).
 * With `asCa` the leaf is itself a CA certificate: what an X509-SVID is not.
 */
export function leaf(
  dir: string,
  ca: Authority,
  uris: readonly string[],
  asCa = false,
): Leaf {
  const name = `leaf${counter++}`;
  const key = join(dir, `${name}.key`);
  const csr = join(dir, `${name}.csr`);
  const cert = join(dir, `${name}.crt`);
  const ext = join(dir, `${name}.ext`);
  newRequest(key, csr, "leaf");
  const lines = [asCa ? "basicConstraints=CA:TRUE" : "basicConstraints=CA:FALSE"];
  // A CA leaf that also carries the server-auth usages passes OpenSSL's own
  // purpose check, so any refusal is the caller's.
  if (asCa) lines.push("keyUsage=digitalSignature,keyCertSign", "extendedKeyUsage=serverAuth");
  if (uris.length > 0) lines.push(`subjectAltName=${uris.map((u) => `URI:${u}`).join(",")}`);
  writeFileSync(ext, `${lines.join("\n")}\n`);
  openssl(
    "x509", "-req", "-in", csr, "-CA", ca.cert, "-CAkey", ca.key, "-CAcreateserial",
    "-out", cert, "-days", "1", "-extfile", ext,
  );
  return { cert: readFileSync(cert, "utf8"), key: readFileSync(key, "utf8") };
}

/** A self-signed certificate carrying no basicConstraints extension at all. */
export function bareCertificate(dir: string): string {
  const name = `bare${counter++}`;
  const key = join(dir, `${name}.key`);
  const csr = join(dir, `${name}.csr`);
  const cert = join(dir, `${name}.crt`);
  const ext = join(dir, `${name}.ext`);
  newRequest(key, csr, "bare");
  writeFileSync(ext, "keyUsage=keyCertSign\n");
  openssl("x509", "-req", "-in", csr, "-signkey", key, "-out", cert, "-days", "1", "-extfile", ext);
  return readFileSync(cert, "utf8");
}

export interface Listener {
  port: number;
  /** Whether any request so far arrived with a client certificate. */
  sawClientCert(): boolean;
  /** Handshakes the server completed. */
  handshakes(): number;
  close(): void;
}

/** A TLS server that asks for, but does not require, a client certificate. */
export async function listen(
  server: Leaf,
  tlsVersion?: "TLSv1.2" | "TLSv1.3",
): Promise<Listener> {
  let saw = false;
  let completed = 0;
  const https: Server = createServer(
    {
      ...server,
      requestCert: true,
      rejectUnauthorized: false,
      ...(tlsVersion ? { minVersion: tlsVersion, maxVersion: tlsVersion } : {}),
    },
    (req, res) => {
      const presented = (
        req.socket as unknown as { getPeerCertificate(): object }
      ).getPeerCertificate();
      saw ||= Object.keys(presented).length > 0;
      // An empty message under the Connect unary protocol: the caller's RPC
      // completes only if the TLS handshake did.
      res.writeHead(200, { "content-type": "application/proto" });
      res.end();
    },
  );
  https.on("secureConnection", () => {
    completed += 1;
  });
  https.on("tlsClientError", () => {});
  await new Promise<void>((resolve) => https.listen(0, "127.0.0.1", resolve));
  return {
    port: (https.address() as AddressInfo).port,
    sawClientCert: () => saw,
    handshakes: () => completed,
    close: () => {
      https.close();
      https.closeAllConnections();
    },
  };
}
