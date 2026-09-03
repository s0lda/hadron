#[cfg(test)]
mod tests {
    use super::super::protocol::*;
    use super::super::codec::*;
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    #[test]
    fn test_jsonrpc_request_response_roundtrip() {
        let req = JsonRpcRequest::new("swarm/turn/dispatch", serde_json::json!({
            "quark": "http-ollama",
            "prompt": "Verify health"
        }));
        assert_eq!(req.jsonrpc, "2.0");
        assert!(!req.id.is_empty());

        let json_str = serde_json::to_string(&req).expect("serialize request");
        let parsed: JsonRpcRequest = serde_json::from_str(&json_str).expect("deserialize request");
        assert_eq!(parsed.method, "swarm/turn/dispatch");
        assert_eq!(parsed.id, req.id);

        let res = JsonRpcResponse::success(&req.id, serde_json::json!({"status": "dispatched"}));
        let res_str = serde_json::to_string(&res).expect("serialize response");
        let parsed_res: JsonRpcResponse = serde_json::from_str(&res_str).expect("deserialize response");
        assert_eq!(parsed_res.id, req.id);
        assert!(parsed_res.error.is_none());
        assert_eq!(parsed_res.result.unwrap()["status"], "dispatched");
    }

    #[test]
    fn test_jsonrpc_codec_framing() {
        let mut codec = JsonRpcCodec::new();
        let mut buffer = BytesMut::new();

        let notif = JsonRpcNotification::new("stream/field/event", serde_json::json!({
            "seq": 42,
            "text": "Streaming output"
        }));
        let msg = JsonRpcMessage::Notification(notif);

        codec.encode(msg.clone(), &mut buffer).expect("encode frame");
        assert!(buffer.ends_with(b"\n"));

        let decoded = codec.decode(&mut buffer).expect("decode frame").expect("must produce frame");
        match decoded {
            JsonRpcMessage::Notification(n) => {
                assert_eq!(n.method, "stream/field/event");
                assert_eq!(n.params["seq"], 42);
            }
            _ => panic!("expected notification frame"),
        }
    }
}
