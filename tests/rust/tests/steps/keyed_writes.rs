// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The steps of `../../../features/keyed_writes.feature`.
//!
//! Every scenario compiles one contract against the shipped `bones/v1` files,
//! reads the marker back through descriptor reflection and runs the
//! `batch_of` and `idempotency_key` rules `api-bones-automation` ships over
//! the resulting pool.

use api_bones_automation::{
    BatchOfError, IdempotencyKeyError, check_batch_of, check_idempotency_key,
};
use cucumber::{given, then, when};

use super::keyed_write_contract::keyed_write_contract;
use super::shipped_resolver::{CONTRACT, compiler};
use crate::world::KeyedWriteWorld;

#[given(
    expr = "a single declaring {string} whose request holds {string} and a batch of it declaring {string} whose request holds {string}"
)]
fn given_contract(
    world: &mut KeyedWriteWorld,
    single: String,
    single_field: String,
    batch: String,
    batch_field: String,
) {
    world.source = keyed_write_contract(&single, &single_field, &batch, &batch_field);
}

#[when("the contract is compiled and the keyed write rules are checked")]
fn when_checked(world: &mut KeyedWriteWorld) {
    let mut compiler = compiler(world.source.clone(), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    let pool = compiler.descriptor_pool();
    let set = pool.encode_to_vec();
    world.batch_outcome = Some(check_batch_of(&set));
    world.key_outcome = Some(check_idempotency_key(&set));
    world.pool = Some(pool);
}

fn keyed(world: &KeyedWriteWorld, method: &str) -> bool {
    let pool = world.pool.as_ref().expect("contract not compiled");
    let ext = pool
        .get_extension_by_name("bones.v1.idempotency_key")
        .expect("the shipped vocabulary declares bones.v1.idempotency_key");
    let method = pool
        .get_service_by_name("contract.v1.Svc")
        .expect("service")
        .methods()
        .find(|candidate| candidate.name() == method)
        .expect("method");
    method
        .options()
        .get_extension(&ext)
        .as_bool()
        .expect("idempotency_key is a bool")
}

#[then("the single and the batch read back as keyed")]
fn then_keyed(world: &mut KeyedWriteWorld) {
    assert!(keyed(world, "Do") && keyed(world, "DoMany"));
}

#[then("the single and the batch read back as not keyed")]
fn then_not_keyed(world: &mut KeyedWriteWorld) {
    assert!(!keyed(world, "Do") && !keyed(world, "DoMany"));
}

#[then("every keyed write conforms")]
fn then_conforms(world: &mut KeyedWriteWorld) {
    let batch = world
        .batch_outcome
        .as_ref()
        .expect("batch rule not checked");
    let key = world.key_outcome.as_ref().expect("key rule not checked");
    assert_eq!(batch, &Ok(()));
    assert_eq!(key, &Ok(()));
}

#[then(expr = "the batch rule refuses it as {string}")]
fn then_batch_refused(world: &mut KeyedWriteWorld, reason: String) {
    let outcome = world
        .batch_outcome
        .as_ref()
        .expect("batch rule not checked");
    let Err(BatchOfError::Refused(refusals)) = outcome else {
        panic!("the batch conformed but must be refused: {outcome:?}");
    };
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(
        refusals[0].to_string().ends_with(&reason),
        "refusal `{}` does not end with `{reason}`",
        refusals[0]
    );
    assert_eq!(refusals[0].method, "contract.v1.Svc/DoMany");
}

#[then(expr = "the key rule refuses it as {string}")]
fn then_key_refused(world: &mut KeyedWriteWorld, reason: String) {
    let outcome = world.key_outcome.as_ref().expect("key rule not checked");
    let Err(IdempotencyKeyError::Refused(refusals)) = outcome else {
        panic!("the contract conformed but must be refused: {outcome:?}");
    };
    assert!(!refusals.is_empty());
    for refusal in refusals {
        assert!(
            refusal.to_string().ends_with(&reason),
            "refusal `{refusal}` does not end with `{reason}`"
        );
    }
}
