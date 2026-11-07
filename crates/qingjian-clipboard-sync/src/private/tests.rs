//! 单次消费、过期和拒绝控制字符的隐私边界。

use super::{PrivateVault, Secret};
use std::time::Duration;

#[test]
fn one_time_expiry_replacement_and_redacted_debug() {
    let vault = PrivateVault::default();
    let secret = Secret::new("synthetic-password-甲".into()).unwrap();
    assert!(!format!("{secret:?}").contains("synthetic"));
    vault.store(secret, Duration::from_secs(120));
    assert!(vault.pending());
    assert_eq!(vault.take().unwrap().as_str(), "synthetic-password-甲");
    assert!(vault.take().is_none());
    vault.store(Secret::new("expired".into()).unwrap(), Duration::ZERO);
    assert!(!vault.pending());
    assert!(vault.take().is_none());
    vault.store(
        Secret::new("lock-clear".into()).unwrap(),
        Duration::from_secs(120),
    );
    vault.clear();
    assert!(vault.take().is_none());
}

#[test]
fn input_cannot_inject_enter_tab_or_oversized_data() {
    for text in ["", "a\nb", "a\tb", "a\0b"] {
        assert!(Secret::new(text.into()).is_none());
    }
    assert!(Secret::new("x".repeat(super::MAX_SECRET_BYTES + 1)).is_none());
}

#[test]
fn repeated_input_and_automatic_fill_share_one_permit() {
    use super::input::InputGate;
    use std::sync::{Arc, Barrier};

    let gate = InputGate::new();
    let vault = PrivateVault::default();
    vault.store(
        Secret::new("single-input".into()).unwrap(),
        Duration::from_secs(120),
    );
    let held = gate.acquire().unwrap();
    let barrier = Arc::new(Barrier::new(9));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let barrier = barrier.clone();
            let gate = &gate;
            let vault = &vault;
            scope.spawn(move || {
                barrier.wait();
                assert!(gate.acquire().is_none());
                assert!(vault.pending());
            });
        }
        barrier.wait();
    });
    assert_eq!(vault.take().unwrap().as_str(), "single-input");
    drop(held);
    assert!(gate.acquire().is_some());
    assert!(!gate.active());
}

#[test]
fn failed_input_permit_does_not_erase_a_later_delivery() {
    let gate = super::input::InputGate::new();
    let vault = PrivateVault::default();
    vault.store(
        Secret::new("first".into()).unwrap(),
        Duration::from_secs(120),
    );
    let permit = gate.acquire().unwrap();
    let first = vault.take().unwrap();
    vault.store(
        Secret::new("later".into()).unwrap(),
        Duration::from_secs(120),
    );
    drop(first);
    drop(permit);
    assert_eq!(vault.take().unwrap().as_str(), "later");
}
