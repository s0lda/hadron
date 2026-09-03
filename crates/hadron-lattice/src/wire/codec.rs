use bytes::{Buf, BytesMut};
use std::io;
use tokio_util::codec::{Decoder, Encoder};
use super::protocol::JsonRpcMessage;

#[derive(Debug, Default, Clone)]
pub struct JsonRpcCodec;

impl JsonRpcCodec {
    pub fn new() -> Self {
        Self
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
            let msg: JsonRpcMessage = serde_json::from_slice(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(msg))
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
