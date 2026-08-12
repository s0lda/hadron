use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};

fn main() {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
    let mut cmd = CommandBuilder::new("cmd.exe");
    cmd.arg("/c").arg("echo hello pty");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut buf = [0; 1024];
    let n = reader.read(&mut buf).unwrap();
    println!("Read: {}", String::from_utf8_lossy(&buf[..n]));
    child.wait().unwrap();
}
