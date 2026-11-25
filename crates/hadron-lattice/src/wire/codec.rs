use bytes::{Buf, BytesMut};
use std::io;
use tokio_util::codec::{Decoder, Encoder};
use super::protocol::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, MAX_RPC_FRAME_BYTES};

#[derive(Debug, Default, Clone)]
pub struct JsonRpcCodec;

impl JsonRpcCodec {
    pub fn new() -> Self {
        Self
    }

    pub fn decode_request(line: &str) -> Result<JsonRpcRequest, serde_json::Error> {
        let trimmed = line.trim();
        if trimmed.len() > MAX_RPC_FRAME_BYTES {
            return Err(serde::de::Error::custom(format!(
                "JSON-RPC request exceeds {MAX_RPC_FRAME_BYTES} byte limit"
            )));
        }
        let req: JsonRpcRequest = serde_json::from_str(trimmed)?;
        if req.jsonrpc != "2.0" {
            return Err(serde::de::Error::custom(format!(
                "unsupported jsonrpc version: {}",
                req.jsonrpc
            )));
        }
        Ok(req)
    }

    pub fn decode_response(
        line: &str,
        expected_id: Option<&str>,
    ) -> Result<JsonRpcResponse, serde_json::Error> {
        let trimmed = line.trim();
        if trimmed.len() > MAX_RPC_FRAME_BYTES {
            return Err(serde::de::Error::custom(format!(
                "JSON-RPC response exceeds {MAX_RPC_FRAME_BYTES} byte limit"
            )));
        }
        let resp: JsonRpcResponse = serde_json::from_str(trimmed)?;
        if resp.jsonrpc != "2.0" {
            return Err(serde::de::Error::custom(format!(
                "unsupported jsonrpc version: {}",
                resp.jsonrpc
            )));
        }
        if resp.result.is_some() == resp.error.is_some() {
            return Err(serde::de::Error::custom(
                "JSON-RPC response must contain exactly one of result or error",
            ));
        }
        if let Some(req_id) = expected_id {
            if resp.error.is_none() && resp.id != req_id {
                return Err(serde::de::Error::custom(format!(
                    "JSON-RPC response id '{}' does not match request id '{req_id}'",
                    resp.id
                )));
            }
        }
        Ok(resp)
    }

    pub fn encode_response(res: &JsonRpcResponse) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string(res)?;
        s.push('\n');
        Ok(s)
    }
}

impl Decoder for JsonRpcCodec {
    type Item = JsonRpcMessage;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(i) = src.iter().position(|&b| b == b'\n') {
            let line = src.split_to(i);
            src.advance(1); // strip '\n'
            if line.is_empty() {
                return Ok(None);
            }
            if line.len() > MAX_RPC_FRAME_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("JSON-RPC frame exceeds {MAX_RPC_FRAME_BYTES} byte limit"),
                ));
            }
            let msg: JsonRpcMessage = serde_json::from_slice(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(msg))
        } else if src.len() > MAX_RPC_FRAME_BYTES {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unterminated JSON-RPC frame exceeds {MAX_RPC_FRAME_BYTES} byte limit"),
            ))
        } else {
            Ok(None)
        }
    }
}

impl Encoder<JsonRpcMessage> for JsonRpcCodec {
    type Error = io::Error;

    fn encode(&mut self, item: JsonRpcMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let json = serde_json::to_vec(&item)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        dst.extend_from_slice(&json);
        dst.extend_from_slice(b"\n");
        Ok(())
    }
}
