# api-bones-automation

The one implementation of two things every producer and consumer of an
automation manifest shares (`bones/v1/automation_manifest.proto`):

- `compile`: builds the `AutomationManifest` from a product's serialized
  descriptor set and its repository sources, byte-identically for equal input.
- `signature`: signs and verifies the DSSE `{keyid, sig}` Ed25519 signature
  over the manifest (payload type
  `application/vnd.brefwiz.automation-manifest.v1+protobuf`), and admits a
  signed manifest only for the product that serves it. The crate holds no key
  and no pin: the caller supplies the key set, each key with its validity
  epochs.

## Rules enforced by `compile`

D1-D6 (document rules), T1 (trigger label), A1-A7 (action
declarations), C1 (field classification), U1 (unset closed choice) and M1 (template shape). A refusal
names the rule and the site.

## Rules NOT enforced yet

- D6 refuses a default of the wrong type, but not "a required parameter has
  neither an argument nor a default": templates carry no arguments, so that
  half has nothing to check yet.
- A binding to another product's action is not type-checked against that
  action; the layer resolves such references against the admitted manifest of
  that product.
