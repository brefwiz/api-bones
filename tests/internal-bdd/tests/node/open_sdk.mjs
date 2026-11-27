// SPDX-License-Identifier: MIT
// Opens the SDK of one built copy of the test-service wrapper and prints the
// outcome as one JSON line: `{"opened":true}` or the refusal's name, code and
// message. Arguments: the copy's `dist/generated-l2/index.js`, the base URL,
// the caller's subject and organization.
import { pathToFileURL } from "node:url";

const [index, baseUrl, subject, org] = process.argv.slice(2);
const { apiBonesSdk } = await import(pathToFileURL(index).href);

const session = {
  credential: async () => ({ subject, org, headers: { authorization: "Bearer caller" } }),
  onUnauthenticated: () => {},
};

let outcome;
try {
  await apiBonesSdk(session, { baseUrl });
  outcome = { opened: true };
} catch (error) {
  outcome = { opened: false, name: error.name, code: error.code ?? null, message: error.message };
}
process.stdout.write(`${JSON.stringify(outcome)}\n`, () => process.exit(0));
