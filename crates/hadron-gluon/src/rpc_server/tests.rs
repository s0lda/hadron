use super::dispatcher::*;
use crate::actor::bus::ActorBus;
use hadron_lattice::wire::*;

#[tokio::test]
async fn test_rpc_dispatcher_engine_status_and_roster() {
    let bus = ActorBus::new(16);
    bus.register_quark("orchestrator").await.unwrap();
    bus.register_quark("agy").await.unwrap();

    let dispatcher = RpcDispatcher::new(bus);

    // Call engine/status
    let req = JsonRpcRequest::new("engine/status", serde_json::json!({}));
    let res = dispatcher.dispatch(req).await;
    assert!(res.error.is_none());
    let result = res.result.expect("status result");
    assert_eq!(result["status"], "running");
    assert_eq!(result["active_quarks"].as_array().unwrap().len(), 2);

    // Call unknown method
    let bad_req = JsonRpcRequest::new("invalid/method", serde_json::json!({}));
    let bad_res = dispatcher.dispatch(bad_req).await;
    assert!(bad_res.error.is_some());
    assert_eq!(bad_res.error.unwrap().code, -32601); // Method not found
}
