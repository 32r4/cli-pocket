use cli_pocket_proto::HostId;
use cli_pocket_relay_core::registry::{HostRegistry, HostSlot};
use tokio::sync::mpsc;
use uuid::Uuid;

#[tokio::test(flavor = "current_thread")]
async fn register_lookup_drop() {
    let registry = HostRegistry::new();
    let host_id = HostId(Uuid::now_v7());
    let (tx, _rx) = mpsc::channel(8);
    let slot = HostSlot::new(host_id, tx);

    let handle = registry.register(slot).unwrap();

    assert!(registry.get(&host_id).is_some());

    drop(handle);

    assert!(registry.get(&host_id).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_host_rejected() {
    let registry = HostRegistry::new();
    let host_id = HostId(Uuid::now_v7());
    let (tx, _rx) = mpsc::channel(8);
    let _first = registry
        .register(HostSlot::new(host_id, tx.clone()))
        .unwrap();

    let result = registry.register(HostSlot::new(host_id, tx));

    let Err(cli_pocket_relay_core::RelayError::Protocol(message)) = result else {
        panic!("expected duplicate host protocol error");
    };
    assert_eq!(message, "duplicate host registration");
}

#[tokio::test(flavor = "current_thread")]
async fn stale_registration_handle_does_not_remove_replacement() {
    let registry = HostRegistry::new();
    let host_id = HostId(Uuid::now_v7());
    let (first_tx, _first_rx) = mpsc::channel(8);
    let first = registry.register(HostSlot::new(host_id, first_tx)).unwrap();

    assert!(registry.unregister(&first));

    let (second_tx, _second_rx) = mpsc::channel(8);
    let second = registry
        .register(HostSlot::new(host_id, second_tx))
        .unwrap();

    drop(first);

    assert!(registry.get(&host_id).is_some());

    drop(second);

    assert!(registry.get(&host_id).is_none());
}
