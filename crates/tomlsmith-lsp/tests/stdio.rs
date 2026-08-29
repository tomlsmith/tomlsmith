use std::{
    io::{BufReader, Cursor, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use lsp_server::{Message, RequestId};
use serde_json::json;

#[test]
fn configurable_binary_path_runs_a_complete_stdio_session() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tomlsmith-lsp"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for message in [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"capabilities": {}}
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown"
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
    ] {
        let body = serde_json::to_string(&message).unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    }
    drop(stdin);

    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        let _ = child.wait();
        panic!("tomlsmith-lsp did not terminate after shutdown/exit");
    }

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");

    let mut messages = BufReader::new(Cursor::new(output.stdout));
    let Message::Response(initialize) = Message::read(&mut messages).unwrap().unwrap() else {
        panic!("first stdio message must be the initialize response")
    };
    assert_eq!(initialize.id, RequestId::from(1));
    assert!(initialize.response_result.is_ok());
    let Message::Response(shutdown) = Message::read(&mut messages).unwrap().unwrap() else {
        panic!("second stdio message must be the shutdown response")
    };
    assert_eq!(shutdown.id, RequestId::from(2));
    assert!(shutdown.response_result.is_ok());
    assert!(Message::read(&mut messages).unwrap().is_none());
}
