// SPDX-License-Identifier: MIT
//! The seam on `wasm32`, run in Node by `wasm-bindgen-test-runner`.
//!
//! Native targets compile this file to nothing; the native behaviour is
//! proven by the unit tests beside the code.

#![cfg(target_arch = "wasm32")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use api_bones_rt::{
    Elapsed, Instant, fill_random, sleep, sleep_until, spawn, system_now, timeout, yield_now,
};
use wasm_bindgen_test::wasm_bindgen_test;

/// Sets its flag when dropped, so a test can see a task's future dropped.
struct Flag(Rc<Cell<bool>>);

impl Drop for Flag {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

#[wasm_bindgen_test]
async fn a_joined_task_returns_its_output() {
    let task = spawn(async { 6 * 7 });
    assert_eq!(task.join().await, Some(42));
}

#[wasm_bindgen_test]
async fn a_task_runs_non_send_state() {
    let shared = Rc::new(Cell::new(0));
    let seen = Rc::clone(&shared);
    spawn(async move {
        sleep(Duration::from_millis(5)).await;
        seen.set(9);
    })
    .join()
    .await;
    assert_eq!(shared.get(), 9);
}

#[wasm_bindgen_test]
async fn dropping_a_task_aborts_it() {
    let dropped = Rc::new(Cell::new(false));
    let guard = Flag(Rc::clone(&dropped));
    let task = spawn(async move {
        let _guard = guard;
        sleep(Duration::from_secs(3600)).await;
    });
    yield_now().await;
    assert!(!dropped.get());
    drop(task);
    sleep(Duration::from_millis(5)).await;
    assert!(dropped.get());
}

#[wasm_bindgen_test]
async fn dropping_the_join_future_aborts_the_task() {
    let dropped = Rc::new(Cell::new(false));
    let guard = Flag(Rc::clone(&dropped));
    let task = spawn(async move {
        let _guard = guard;
        sleep(Duration::from_secs(3600)).await;
    });
    let abandoned = timeout(Duration::from_millis(10), task.join()).await;
    assert_eq!(abandoned, Err(Elapsed));
    sleep(Duration::from_millis(5)).await;
    assert!(dropped.get());
}

#[wasm_bindgen_test]
async fn a_detached_task_runs_to_completion() {
    let finished = Rc::new(Cell::new(false));
    let seen = Rc::clone(&finished);
    spawn(async move {
        sleep(Duration::from_millis(5)).await;
        seen.set(true);
    })
    .detach();
    sleep(Duration::from_millis(50)).await;
    assert!(finished.get());
}

#[wasm_bindgen_test]
async fn sleep_waits_at_least_its_duration() {
    let started = Instant::now();
    sleep(Duration::from_millis(30)).await;
    assert!(started.elapsed() >= Duration::from_millis(25));
}

#[wasm_bindgen_test]
async fn sleep_until_waits_for_the_instant_and_not_past_it() {
    let started = Instant::now();
    sleep_until(started + Duration::from_millis(30)).await;
    assert!(started.elapsed() >= Duration::from_millis(25));
    let before = Instant::now();
    sleep_until(started).await;
    assert!(before.elapsed() < Duration::from_millis(25));
}

#[wasm_bindgen_test]
async fn timeout_returns_the_output_before_the_deadline() {
    let done = timeout(Duration::from_secs(5), async {
        sleep(Duration::from_millis(5)).await;
        "done"
    })
    .await;
    assert_eq!(done, Ok("done"));
}

#[wasm_bindgen_test]
async fn timeout_reports_elapsed_after_the_deadline() {
    let late = timeout(Duration::from_millis(10), sleep(Duration::from_secs(3600))).await;
    assert_eq!(late, Err(Elapsed));
}

#[wasm_bindgen_test]
async fn yield_now_lets_a_queued_task_run_first() {
    let ran = Rc::new(Cell::new(false));
    let seen = Rc::clone(&ran);
    spawn(async move { seen.set(true) }).detach();
    assert!(!ran.get());
    yield_now().await;
    assert!(ran.get());
}

#[wasm_bindgen_test]
fn instant_never_goes_backwards() {
    let mut last = Instant::now();
    for _ in 0..1000 {
        let next = Instant::now();
        assert!(next >= last);
        last = next;
    }
}

#[wasm_bindgen_test]
fn instant_arithmetic_is_consistent() {
    let a = Instant::now();
    let b = a + Duration::from_secs(2);
    assert!(b > a);
    assert_eq!(b - a, Duration::from_secs(2));
    assert_eq!(a - b, Duration::ZERO);
    assert_eq!(b.saturating_duration_since(a), Duration::from_secs(2));
    assert_eq!(b - Duration::from_secs(2), a);
    assert_eq!(a.checked_add(Duration::from_secs(2)), Some(b));
    assert_eq!(b.checked_sub(Duration::from_secs(2)), Some(a));
    assert_eq!(a.checked_add(Duration::MAX), None);
}

#[wasm_bindgen_test]
fn system_now_is_after_the_unix_epoch() {
    let since_epoch = system_now().duration_since(UNIX_EPOCH).unwrap();
    assert!(since_epoch > Duration::from_secs(1_700_000_000));
}

#[wasm_bindgen_test]
fn fill_random_fills_the_buffer_with_fresh_bytes() {
    let mut first = [0u8; 32];
    let mut second = [0u8; 32];
    fill_random(&mut first).unwrap();
    fill_random(&mut second).unwrap();
    assert_ne!(first, [0u8; 32]);
    assert_ne!(first, second);
}
