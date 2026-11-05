# api-bones-automation

The shared implementation every producer and consumer of an
automation manifest shares (`bones/v1/automation_manifest.proto`):

- `compile`: builds the `AutomationManifest` from a product's serialized
  descriptor set and its repository sources, byte-identically for equal input.
- `check_surface`: applies the same rules to a product's declarations without
  its identity, which only the serving workload's SVID carries, and returns the
  declared trigger, action and template names and the `declaration_digest` the
  release manifest will carry, so a product's build refuses what its release
  would and serves the digest the release signs.
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

A template references its own product's triggers and actions with `product`
empty; a non-empty `product` names another product, whose references the layer
resolves against that product's admitted manifest. `compile` refuses a template
that names the declaring product (M1). That is the one rule that needs the
identity, so `check_surface` leaves such a reference to the layer like any other
product's, and everything else it refuses exactly as `compile` does.

## Document admission and publication

`check_document` checks serialized `AutomationDocument` bytes against the
admitted manifests supplied in `DocumentInput`, including the declaring
product's manifest. `DocumentPhase::Admission` resolves a template's references
with open parameter slots; `DocumentPhase::Publish` additionally requires each
required parameter to have an argument or default. Both phases validate
argument names and types, references and bindings, including another product's
trigger, action, resolver or earlier step response. Refusals name the rule,
document site and failing predicate. Unusable documents and manifests carry
separate input errors.

Each product's schema stays in its own descriptor pool. Named message and enum
types must have matching names and definitions; scalar types and collection
shapes must match. Only the manifest's declared trigger/action inventory is
callable. The caller supplies one already-admitted manifest per product and
owns signature and identity verification; the check performs no I/O.

`compile` checks references to the declaring product and leaves cross-product
resolution for `check_document`. Templates carry no arguments, so compilation
permits required parameter slots and never applies the publish-only condition.
