use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use hadron_lattice::wire::{
    remove_rpc_metadata, write_rpc_metadata, JsonRpcCodec, JsonRpcResponse, RpcEndpointMetadata,
};
use super::dispatcher::RpcDispatcher;

pub struct RpcServer;

impl RpcServer {
    pub async fn bind_and_serve(
        field_dir: &Path,
        dispatcher: Arc<RpcDispatcher>,
        mut shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<(SocketAddr, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let meta = RpcEndpointMetadata {
            addr: addr.to_string(),
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
        };
        write_rpc_metadata(field_dir, &meta)?;

        let field_dir_buf: PathBuf = field_dir.to_path_buf();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            break;
                        }
                    }
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((stream, _)) => {
                                let disp = dispatcher.clone();
                                let mut client_shutdown = shutdown.clone();
                                tokio::spawn(async move {
                                    let (reader, mut writer) = stream.into_split();
                                    let mut buf_reader = BufReader::new(reader);
                                    let mut line = String::new();
                                    loop {
                                        line.clear();
                                        tokio::select! {
                                            _ = client_shutdown.changed() => {
                                                if *client_shutdown.borrow() {
                                                    break;
                                                }
                                            }
                                            read_res = buf_reader.read_line(&mut line) => {
                                                match read_res {
                                                    Ok(0) => break, // EOF
                                                    Ok(_) => {
                                                        let trimmed = line.trim();
                                                        if trimmed.is_empty() {
                                                            continue;
                                                        }
                                                        let resp = match JsonRpcCodec::decode_request(trimmed) {
                                                            Ok(req) => disp.dispatch(req).await,
                                                            Err(e) => JsonRpcResponse::error(
                                                                "null",
                                                                -32700,
                                                                format!("Parse error: {e}"),
                                                                None,
                                                            ),
                                                        };
                                                        if let Ok(encoded) = JsonRpcCodec::encode_response(&resp) {
                                                            if writer.write_all(encoded.as_bytes()).await.is_err() {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    Err(_) => break,
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
            if let Err(e) = remove_rpc_metadata(&field_dir_buf) {
                hadron_lattice::term::warn(
                    hadron_lattice::term::Source::Gluon,
                    &format!("unable to clean up rpc metadata: {e:#}"),
                );
            }
        });

        Ok((addr, handle))
    }
}
