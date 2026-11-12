//! Synchronous, bounded-timeout JSON-RPC 2.0 loopback client for `hadron-chamber`.
//!
//! Connects to the local `hadron-gluon` daemon advertised in `.hadron/gluon.rpc`
//! (`127.0.0.1:<port>`) to deliver `<1ms` wake notifications on user message submission
//! and optional incremental field polling without blocking the GPUI render thread.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hadron_lattice::wire::{
    DAEMON_SUBSCRIBE_METHOD, JsonRpcCodec, JsonRpcRequest, JsonRpcResponse, RpcEndpointMetadata,
    RpcPushEvent, read_rpc_metadata,
};

fn resolve_rpc_dir(path: &Path) -> PathBuf {
    if path.file_name().is_some_and(|n| n == "field.jsonl") || path.is_file() {
        if let Some(parent) = path.parent() {
            if parent.join("gluon.rpc").is_file() {
                return parent.to_path_buf();
            }
        }
        hadron_lattice::hadron_dir_of(path)
    } else if path.join("gluon.rpc").is_file() {
        path.to_path_buf()
    } else if path.join(".hadron").is_dir() {
        path.join(".hadron")
    } else {
        path.to_path_buf()
    }
}

/// Read and validate the active `gluon.rpc` endpoint metadata for `field_dir`.
pub fn active_endpoint(field_dir: &Path) -> Option<RpcEndpointMetadata> {
    let dir = resolve_rpc_dir(field_dir);
    let meta = read_rpc_metadata(&dir)?;
    if meta.addr.parse::<SocketAddr>().is_err() {
        return None;
    }
    Some(meta)
}

/// Execute a single newline-delimited JSON-RPC 2.0 request over loopback TCP with a strict timeout.
pub fn call_rpc(
    addr: &str,
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> anyhow::Result<JsonRpcResponse> {
    let sock_addr: SocketAddr = addr.parse()?;
    let mut stream = TcpStream::connect_timeout(&sock_addr, timeout)?;
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let req = JsonRpcRequest::new(method, params);
    let mut payload = serde_json::to_string(&req)?;
    payload.push('\n');
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        anyhow::bail!("empty JSON-RPC response from {addr}");
    }
    let resp: JsonRpcResponse = serde_json::from_str(line.trim())?;
    Ok(resp)
}

const WAKE_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_secs(2)
} else {
    Duration::from_millis(50)
};

const POLL_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_secs(2)
} else {
    Duration::from_millis(100)
};

/// Immediately wake the `hadron-gluon` daemon loop over JSON-RPC (`"daemon/wake"`) if `gluon.rpc` is present.
/// Uses a tight timeout and returns `false` on any error so callers fall back to file watching seamlessly.
pub fn notify_daemon_wake(field_dir: &Path) -> bool {
    let Some(meta) = active_endpoint(field_dir) else {
        return false;
    };
    call_rpc(
        &meta.addr,
        "daemon/wake",
        serde_json::json!({}),
        WAKE_TIMEOUT,
    )
    .is_ok_and(|resp| resp.error.is_none())
}

/// Poll incremental `field.jsonl` events from `hadron-gluon` over `"field/poll"` JSON-RPC.
pub fn poll_field_via_rpc(
    field_dir: &Path,
    offset: u64,
    count: usize,
) -> Option<(Vec<hadron_lattice::Event>, u64)> {
    let meta = active_endpoint(field_dir)?;
    let resp = call_rpc(
        &meta.addr,
        "field/poll",
        serde_json::json!({
            "offset": offset,
            "count": count,
        }),
        POLL_TIMEOUT,
    )
    .ok()?;
    if resp.error.is_some() {
        return None;
    }
    let result = resp.result?;
    let next_offset = result.get("next_offset")?.as_u64()?;
    let events_val = result.get("events")?.clone();
    let events: Vec<hadron_lattice::Event> = serde_json::from_value(events_val).ok()?;
    Some((events, next_offset))
}

/// Read slice for the subscriber socket and backoff sleeps: bounds how long
/// [`RpcSubscriptionHandle`]'s `Drop` waits for the thread to notice `stop`.
const SUBSCRIBE_SLICE: Duration = Duration::from_millis(100);
const SUBSCRIBE_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const RECONNECT_BACKOFF_MIN: Duration = Duration::from_millis(100);
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(5);
/// Three missed daemon heartbeats (15s each): treat a silent socket as half-open and reconnect.
const SUBSCRIBE_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Default)]
struct PushState {
    dirty: AtomicBool,
    connected: AtomicBool,
    stop: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

/// Cloneable view of a [`RpcSubscriptionHandle`]: a dirty flag set on every
/// non-heartbeat `daemon/event`, plus an async wake for the chamber's poll loop.
#[derive(Clone, Default)]
pub struct PushSignal(Arc<PushState>);

impl PushSignal {
    /// Whether the subscriber currently holds an acknowledged `daemon/subscribe` stream.
    pub fn is_connected(&self) -> bool {
        self.0.connected.load(Ordering::Acquire)
    }

    /// Consume the dirty flag, returning whether a push arrived since the last take.
    pub fn take_dirty(&self) -> bool {
        self.0.dirty.swap(false, Ordering::AcqRel)
    }

    /// Async-poll form of [`Self::take_dirty`]: registers `cx`'s waker before
    /// re-checking, so a push landing between the two checks is never lost.
    pub fn poll_dirty(&self, cx: &mut std::task::Context<'_>) -> Poll<()> {
        if self.take_dirty() {
            return Poll::Ready(());
        }
        *self.0.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        if self.take_dirty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn mark_dirty(&self) {
        self.0.dirty.store(true, Ordering::Release);
        let waker = self.0.waker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(w) = waker {
            w.wake();
        }
    }

    fn stopped(&self) -> bool {
        self.0.stop.load(Ordering::Acquire)
    }
}

/// Background `daemon/subscribe` listener. Owns one OS thread that resolves
/// `gluon.rpc` (re-read on every reconnect, so a restarted daemon on a new port is
/// picked up), streams `daemon/event` notifications, and raises [`PushSignal`].
/// All disk and socket I/O stays on that thread; dropping the handle stops and joins it.
pub struct RpcSubscriptionHandle {
    signal: PushSignal,
    thread: Option<JoinHandle<()>>,
}

impl RpcSubscriptionHandle {
    pub fn spawn(field_dir: PathBuf) -> Self {
        let signal = PushSignal::default();
        let worker = signal.clone();
        let thread = std::thread::Builder::new()
            .name("chamber-rpc-subscribe".into())
            .spawn(move || run_subscriber(&field_dir, &worker))
            .ok();
        Self { signal, thread }
    }

    pub fn signal(&self) -> PushSignal {
        self.signal.clone()
    }
}

impl Drop for RpcSubscriptionHandle {
    fn drop(&mut self) {
        self.signal.0.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run_subscriber(field_dir: &Path, signal: &PushSignal) {
    let mut backoff = RECONNECT_BACKOFF_MIN;
    while !signal.stopped() {
        let subscribed = active_endpoint(field_dir)
            .is_some_and(|meta| subscribe_until_disconnect(&meta.addr, signal));
        signal.0.connected.store(false, Ordering::Release);
        backoff = if subscribed {
            RECONNECT_BACKOFF_MIN
        } else {
            (backoff * 2).min(RECONNECT_BACKOFF_MAX)
        };
        let deadline = Instant::now() + backoff;
        while !signal.stopped() && Instant::now() < deadline {
            std::thread::sleep(SUBSCRIBE_SLICE.min(deadline - Instant::now()));
        }
    }
}

/// One subscription session. Returns whether the daemon acknowledged the subscribe,
/// which resets the reconnect backoff.
fn subscribe_until_disconnect(addr: &str, signal: &PushSignal) -> bool {
    let Ok(sock_addr) = addr.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&sock_addr, SUBSCRIBE_CONNECT_TIMEOUT) else {
        return false;
    };
    stream.set_nodelay(true).ok();
    if stream.set_read_timeout(Some(SUBSCRIBE_SLICE)).is_err()
        || stream.set_write_timeout(Some(SUBSCRIBE_CONNECT_TIMEOUT)).is_err()
    {
        return false;
    }
    let req = JsonRpcRequest::new(DAEMON_SUBSCRIBE_METHOD, serde_json::json!({}));
    let Ok(mut payload) = serde_json::to_string(&req) else {
        return false;
    };
    payload.push('\n');
    if stream.write_all(payload.as_bytes()).and_then(|()| stream.flush()).is_err() {
        return false;
    }

    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    let mut subscribed = false;
    let mut last_rx = Instant::now();
    while !signal.stopped() {
        // `read_until` keeps every byte read before a timeout in `buf`, so a line
        // split across read slices is reassembled rather than dropped.
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) if buf.last() != Some(&b'\n') => break,
            Ok(_) => {
                last_rx = Instant::now();
                let line = String::from_utf8_lossy(&buf).into_owned();
                buf.clear();
                match JsonRpcCodec::decode_push_event(&line) {
                    Ok(RpcPushEvent::Heartbeat { .. }) => {}
                    Ok(_) => signal.mark_dirty(),
                    Err(_) if !subscribed => {
                        let acked = serde_json::from_str::<JsonRpcResponse>(line.trim())
                            .is_ok_and(|r| r.id == req.id && r.error.is_none());
                        if !acked {
                            break;
                        }
                        subscribed = true;
                        signal.0.connected.store(true, Ordering::Release);
                        // Anything appended while disconnected was never pushed: poll once now.
                        signal.mark_dirty();
                    }
                    Err(_) => {}
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                if last_rx.elapsed() >= SUBSCRIBE_IDLE_TIMEOUT {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    subscribed
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::wire::{JsonRpcCodec, remove_rpc_metadata, write_rpc_metadata};
    use std::net::TcpListener;

    #[test]
    fn active_endpoint_returns_none_when_missing_and_some_when_valid() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(active_endpoint(tmp.path()).is_none());

        let meta = RpcEndpointMetadata {
            addr: "127.0.0.1:41234".to_string(),
            pid: 999,
            version: "0.33.27".to_string(),
            platform: "linux".to_string(),
        };
        write_rpc_metadata(tmp.path(), &meta).expect("write gluon.rpc");
        assert_eq!(active_endpoint(tmp.path()), Some(meta.clone()));
        assert_eq!(
            active_endpoint(&tmp.path().join("field.jsonl")),
            Some(meta)
        );

        remove_rpc_metadata(tmp.path()).expect("remove gluon.rpc");
        assert!(active_endpoint(tmp.path()).is_none());
    }

    #[test]
    fn notify_daemon_wake_and_poll_field_round_trip_over_loopback_tcp() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local_addr").to_string();

        let meta = RpcEndpointMetadata {
            addr: addr.clone(),
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
        };
        write_rpc_metadata(tmp.path(), &meta).expect("write gluon.rpc");

        let ready = std::sync::Arc::new(std::sync::Barrier::new(2));
        let server_ready = std::sync::Arc::clone(&ready);
        let server = std::thread::spawn(move || {
            server_ready.wait();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request");
                let req = JsonRpcCodec::decode_request(line.trim()).expect("decode request");
                let resp = match req.method.as_str() {
                    "daemon/wake" => JsonRpcResponse::success(
                        &req.id,
                        serde_json::json!({ "woken": true }),
                    ),
                    "field/poll" => {
                        let ev = hadron_lattice::Event::new(
                            hadron_lattice::Actor::Human,
                            None,
                            hadron_lattice::Kind::Message {
                                body: "hello over rpc".to_string(),
                            },
                        );
                        JsonRpcResponse::success(
                            &req.id,
                            serde_json::json!({
                                "events": [ev],
                                "next_offset": 128u64,
                            }),
                        )
                    }
                    other => JsonRpcResponse::error(
                        &req.id,
                        -32601,
                        format!("unknown: {other}"),
                        None,
                    ),
                };
                let encoded = JsonRpcCodec::encode_response(&resp).expect("encode response");
                stream.write_all(encoded.as_bytes()).expect("write response");
                stream.flush().expect("flush");
            }
        });

        ready.wait();
        assert!(notify_daemon_wake(tmp.path()));
        let (events, next_offset) =
            poll_field_via_rpc(tmp.path(), 0, 0).expect("poll_field_via_rpc");
        assert_eq!(next_offset, 128);
        assert_eq!(events.len(), 1);

        server.join().expect("server thread");
        remove_rpc_metadata(tmp.path()).expect("remove gluon.rpc");
    }

    fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Accept one subscriber, check it asked for `daemon/subscribe`, and ack it.
    fn accept_subscriber(listener: &TcpListener) -> TcpStream {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("read subscribe request");
        let req = JsonRpcCodec::decode_request(line.trim()).expect("decode request");
        assert_eq!(req.method, DAEMON_SUBSCRIBE_METHOD);
        let ack = JsonRpcResponse::success(&req.id, serde_json::json!({ "subscribed": true }));
        let encoded = JsonRpcCodec::encode_response(&ack).expect("encode ack");
        stream.write_all(encoded.as_bytes()).expect("write ack");
        stream
    }

    fn push(stream: &mut TcpStream, event: &RpcPushEvent) {
        let line = JsonRpcCodec::encode_push_event(event).expect("encode push");
        stream.write_all(line.as_bytes()).expect("write push");
        stream.flush().expect("flush push");
    }

    #[test]
    fn subscription_marks_dirty_on_push_reconnects_and_stops_on_drop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let meta = RpcEndpointMetadata {
            addr: listener.local_addr().expect("local_addr").to_string(),
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
        };
        write_rpc_metadata(tmp.path(), &meta).expect("write gluon.rpc");

        let handle = RpcSubscriptionHandle::spawn(tmp.path().join("field.jsonl"));
        let signal = handle.signal();

        let mut stream = accept_subscriber(&listener);
        wait_until("subscribed", || signal.is_connected());
        // The ack itself marks dirty so events appended while disconnected get polled.
        wait_until("post-ack dirty", || signal.take_dirty());

        push(&mut stream, &RpcPushEvent::Heartbeat { pid: 1 });
        std::thread::sleep(Duration::from_millis(200));
        assert!(!signal.take_dirty(), "a heartbeat must not raise dirty");
        push(&mut stream, &RpcPushEvent::FieldAppended { next_offset: 64, count: 1 });
        wait_until("field push dirty", || signal.take_dirty());

        // A pending poller is woken by the push rather than having to re-check.
        struct CountWake(std::sync::atomic::AtomicUsize);
        impl std::task::Wake for CountWake {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let count = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&count));
        let mut cx = std::task::Context::from_waker(&waker);
        assert!(signal.poll_dirty(&mut cx).is_pending());
        push(&mut stream, &RpcPushEvent::GateUpdated { quark: "q".into(), passed: Some(false) });
        wait_until("waker fired", || count.0.load(Ordering::SeqCst) == 1);
        assert!(signal.poll_dirty(&mut cx).is_ready());

        // Daemon goes away: the subscriber notices, then reconnects on its own.
        drop(stream);
        wait_until("disconnect noticed", || !signal.is_connected());
        let _stream = accept_subscriber(&listener);
        wait_until("resubscribed", || signal.is_connected());

        let started = Instant::now();
        drop(handle);
        assert!(started.elapsed() < Duration::from_secs(2), "drop must stop the thread promptly");
        assert!(!signal.is_connected(), "a stopped subscriber reports disconnected");
        remove_rpc_metadata(tmp.path()).expect("remove gluon.rpc");
    }
}
