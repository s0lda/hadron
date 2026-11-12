use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use hadron_lattice::wire::{
    remove_rpc_metadata, write_rpc_metadata, JsonRpcCodec, JsonRpcResponse, RpcEndpointMetadata,
    RpcPushEvent, DAEMON_SUBSCRIBE_METHOD,
};
use super::dispatcher::RpcDispatcher;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

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
                    res = shutdown.changed() => {
                        if res.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((stream, _)) => {
                                let _ = stream.set_nodelay(true);
                                let disp = dispatcher.clone();
                                let mut client_shutdown = shutdown.clone();
                                tokio::spawn(async move {
                                    let (reader, mut writer) = stream.into_split();
                                    let mut buf_reader = BufReader::new(reader);
                                    let mut line = String::new();
                                    let mut push_rx: Option<tokio::sync::broadcast::Receiver<RpcPushEvent>> = None;
                                    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
                                    // Consume the immediate first tick so heartbeats fire every 15s after subscribe.
                                    heartbeat.tick().await;

                                    loop {
                                        line.clear();
                                        tokio::select! {
                                            res = client_shutdown.changed() => {
                                                if res.is_err() || *client_shutdown.borrow() {
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
                                                            Ok(req) => {
                                                                if req.method == DAEMON_SUBSCRIBE_METHOD {
                                                                    if push_rx.is_none() {
                                                                        push_rx = Some(disp.subscribe());
                                                                    }
                                                                    JsonRpcResponse::success(
                                                                        &req.id,
                                                                        serde_json::json!({ "subscribed": true }),
                                                                    )
                                                                } else {
                                                                    disp.dispatch(req).await
                                                                }
                                                            }
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
                                                            if writer.flush().await.is_err() {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    Err(_) => break,
                                                }
                                            }
                                            recv_res = async {
                                                match push_rx.as_mut() {
                                                    Some(rx) => rx.recv().await,
                                                    None => std::future::pending().await,
                                                }
                                            } => {
                                                match recv_res {
                                                    Ok(ev) => {
                                                        if let Ok(encoded) = JsonRpcCodec::encode_push_event(&ev) {
                                                            if writer.write_all(encoded.as_bytes()).await.is_err() {
                                                                break;
                                                            }
                                                            if writer.flush().await.is_err() {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    Err(RecvError::Lagged(_)) => continue,
                                                    Err(RecvError::Closed) => break,
                                                }
                                            }
                                            _ = heartbeat.tick(), if push_rx.is_some() => {
                                                let hb = RpcPushEvent::Heartbeat { pid: std::process::id() };
                                                if let Ok(encoded) = JsonRpcCodec::encode_push_event(&hb) {
                                                    if writer.write_all(encoded.as_bytes()).await.is_err() {
                                                        break;
                                                    }
                                                    if writer.flush().await.is_err() {
                                                        break;
                                                    }
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
