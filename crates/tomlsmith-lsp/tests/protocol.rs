use std::{thread, time::Duration};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{Value, json};

fn initialized_server() -> (Connection, thread::JoinHandle<tomlsmith_lsp::ServerResult>) {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(0),
            method: "initialize".to_owned(),
            params: json!({
                "capabilities": {
                    "textDocument": {
                        "documentSymbol": {"hierarchicalDocumentSymbolSupport": true}
                    }
                }
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("initialize must return a response")
    };
    assert!(response.response_result.is_ok());
    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();
    (client, server_thread)
}

fn initialized_server_with_options(
    initialization_options: &Value,
) -> (Connection, thread::JoinHandle<tomlsmith_lsp::ServerResult>) {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(0),
            method: "initialize".to_owned(),
            params: json!({
                "capabilities": {},
                "initializationOptions": initialization_options
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("initialize must return a response")
    };
    assert!(response.response_result.is_ok());
    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();
    (client, server_thread)
}

fn formatted_text(client: &Connection, source: &str, tab_size: u32) -> String {
    let uri = "file:///workspace/config-format.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": source
                }
            }),
        }))
        .unwrap();
    let _diagnostics = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(900),
            method: "textDocument/formatting".to_owned(),
            params: json!({
                "textDocument": {"uri": uri},
                "options": {"tabSize": tab_size, "insertSpaces": true}
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("formatting must return a response")
    };
    let edits = response.response_result.unwrap();
    let edits = edits.as_array().expect("formatting must return edits");
    assert!(!edits.is_empty(), "formatting should return changed text");
    apply_formatting_edits(source, edits)
}

/// Byte offset for a UTF-16 (line, character) position, mirroring the LSP
/// position encoding the server advertises.
fn byte_offset(text: &str, line: u64, character: u64) -> usize {
    let mut current_line = 0_u64;
    let mut offset = 0_usize;
    if line > 0 {
        for (position, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                current_line += 1;
                if current_line == line {
                    offset = position + 1;
                    break;
                }
            }
        }
        assert_eq!(current_line, line, "line {line} out of bounds");
    }
    let mut utf16 = 0_u64;
    let line_text = &text[offset..];
    for (position, ch) in line_text.char_indices() {
        if utf16 >= character {
            return offset + position;
        }
        assert!(
            ch != '\n',
            "character {character} beyond end of line {line}"
        );
        utf16 += ch.len_utf16() as u64;
    }
    offset + line_text.len()
}

/// Applies formatting edits after checking they are sorted ascending and
/// non-overlapping; the server no longer answers with one edit covering the
/// whole document, so tests reconstruct the formatted text this way.
fn apply_formatting_edits(text: &str, edits: &[Value]) -> String {
    let mut spans = Vec::new();
    for edit in edits {
        let range = &edit["range"];
        let start = byte_offset(
            text,
            range["start"]["line"].as_u64().unwrap(),
            range["start"]["character"].as_u64().unwrap(),
        );
        let end = byte_offset(
            text,
            range["end"]["line"].as_u64().unwrap(),
            range["end"]["character"].as_u64().unwrap(),
        );
        assert!(start <= end, "edit range reversed: {edit}");
        spans.push((start, end, edit["newText"].as_str().unwrap().to_owned()));
    }
    for window in spans.windows(2) {
        assert!(
            window[0].1 <= window[1].0,
            "edits overlap or are unsorted: {:?} then {:?}",
            window[0],
            window[1]
        );
    }
    let mut result = text.to_owned();
    for (start, end, replacement) in spans.iter().rev() {
        result.replace_range(start..end, replacement);
    }
    result
}

fn open_document(client: &Connection, uri: &str, text: &str) -> Notification {
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": text
                }
            }),
        }))
        .unwrap();
    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didOpen must publish diagnostics")
    };
    published
}

fn request_formatting(client: &Connection, id: i32, uri: &str) -> Value {
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(id),
            method: "textDocument/formatting".to_owned(),
            params: json!({
                "textDocument": {"uri": uri},
                "options": {"tabSize": 2, "insertSpaces": true}
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("formatting must return a response")
    };
    response.response_result.unwrap()
}

fn request_document_symbols(client: &Connection, id: i32, uri: &str) -> Value {
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(id),
            method: "textDocument/documentSymbol".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("documentSymbol must return a response")
    };
    response.response_result.unwrap()
}

fn position_not_after(left: &Value, right: &Value) -> bool {
    let key = |position: &Value| {
        (
            position["line"].as_u64().unwrap(),
            position["character"].as_u64().unwrap(),
        )
    };
    key(left) <= key(right)
}

fn range_contains(outer: &Value, inner: &Value) -> bool {
    position_not_after(&outer["start"], &inner["start"])
        && position_not_after(&inner["end"], &outer["end"])
}

/// Asserts the LSP `DocumentSymbol` invariants over a whole subtree: the
/// selection range lies within the full range, and every child range lies
/// within its parent's range.
fn assert_symbol_tree_invariants(symbol: &Value) {
    assert!(
        range_contains(&symbol["range"], &symbol["selectionRange"]),
        "selectionRange must lie within range: {symbol:?}"
    );
    let Some(children) = symbol["children"].as_array() else {
        return;
    };
    for child in children {
        assert!(
            range_contains(&symbol["range"], &child["range"]),
            "child {child:?} must lie within its parent {symbol:?}"
        );
        assert_symbol_tree_invariants(child);
    }
}

fn change_configuration(client: &Connection, settings: &Value) {
    client
        .sender
        .send(Message::Notification(Notification {
            method: "workspace/didChangeConfiguration".to_owned(),
            params: json!({"settings": settings}),
        }))
        .unwrap();
}

fn finish_server(
    client: Connection,
    server_thread: thread::JoinHandle<tomlsmith_lsp::ServerResult>,
) {
    drop(client);
    assert!(server_thread.join().unwrap().is_ok());
}

#[test]
fn initialize_advertises_the_supported_document_features() {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(1),
            method: "initialize".to_owned(),
            params: json!({
                "capabilities": {},
                "rootUri": null
            }),
        }))
        .unwrap();

    let response = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    let Message::Response(response) = response else {
        panic!("initialize must return a response")
    };
    assert_eq!(response.id, RequestId::from(1));
    let result: Value = response.response_result.unwrap();
    assert_eq!(result["capabilities"]["positionEncoding"], "utf-16");
    assert_eq!(result["capabilities"]["textDocumentSync"], 2);
    assert_eq!(result["capabilities"]["documentFormattingProvider"], true);
    assert_eq!(result["capabilities"]["documentSymbolProvider"], true);
    assert_eq!(result["capabilities"]["foldingRangeProvider"], true);
    assert_eq!(result["capabilities"]["hoverProvider"], true);
    assert_eq!(
        result["capabilities"]["semanticTokensProvider"]["full"],
        true
    );
    assert_eq!(
        result["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"],
        json!([
            "property",
            "namespace",
            "string",
            "number",
            "keyword",
            "comment",
            "operator",
            "tomlDateTime",
            "tomlInvalid"
        ])
    );
    assert_eq!(
        result["capabilities"]["semanticTokensProvider"]["legend"]["tokenModifiers"],
        json!([
            "tomlArray",
            "tomlInlineTable",
            "tomlArrayTable",
            "tomlInlineTableMember"
        ])
    );

    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();
    drop(client);
    assert!(server_thread.join().unwrap().is_ok());
}

#[test]
fn initialization_can_select_strict_toml_1_0_core_rules() {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(1),
            method: "initialize".to_owned(),
            params: json!({
                "capabilities": {},
                "initializationOptions": {"tomlVersion": "1.0"}
            }),
        }))
        .unwrap();
    let _initialize = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": "file:///workspace/v1.toml",
                    "languageId": "toml",
                    "version": 1,
                    "text": "escape = \"\\e\"\n"
                }
            }),
        }))
        .unwrap();
    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didOpen must publish strict-version diagnostics")
    };
    assert!(
        published.params["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "version.toml-1.1-syntax")
    );

    finish_server(client, server_thread);
}

#[test]
fn absent_initialization_options_default_to_toml_1_1() {
    let (client, server_thread) = initialized_server();

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": "file:///workspace/default-version.toml",
                    "languageId": "toml",
                    "version": 1,
                    "text": "escape = \"\\e\"\n"
                }
            }),
        }))
        .unwrap();
    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didOpen must publish diagnostics")
    };
    assert_eq!(published.params["diagnostics"], json!([]));

    finish_server(client, server_thread);
}

#[test]
fn unrecognized_toml_version_values_fall_back_to_1_1() {
    let (client, server_thread) = initialized_server_with_options(&json!({"tomlVersion": "2.0"}));

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": "file:///workspace/unknown-version.toml",
                    "languageId": "toml",
                    "version": 1,
                    "text": "escape = \"\\e\"\n"
                }
            }),
        }))
        .unwrap();
    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didOpen must publish diagnostics")
    };
    assert_eq!(published.params["diagnostics"], json!([]));

    finish_server(client, server_thread);
}

#[test]
fn explicit_toml_1_1_accepts_toml_1_1_syntax() {
    let (client, server_thread) = initialized_server_with_options(&json!({"tomlVersion": "1.1"}));

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": "file:///workspace/opt-in.toml",
                    "languageId": "toml",
                    "version": 1,
                    "text": "escape = \"\\e\"\n"
                }
            }),
        }))
        .unwrap();
    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didOpen must publish diagnostics")
    };
    assert_eq!(published.params["diagnostics"], json!([]));

    finish_server(client, server_thread);
}

#[test]
fn initialization_format_options_control_indent_and_line_endings() {
    let (client, server_thread) = initialized_server_with_options(&json!({
        "format": {
            "indentWidth": 4,
            "lineWidth": 120,
            "lineEnding": "crlf"
        }
    }));

    let formatted = formatted_text(&client, "values=[\n1,\n]\n", 0);

    assert_eq!(formatted, "values = [\r\n    1,\r\n]\r\n");
    finish_server(client, server_thread);
}

#[test]
fn explicit_initialized_indent_width_overrides_formatting_tab_size() {
    let (client, server_thread) = initialized_server_with_options(&json!({
        "format": {
            "indentWidth": 4,
            "lineWidth": 120,
            "lineEnding": "lf"
        }
    }));

    let formatted = formatted_text(&client, "values=[\n1,\n]\n", 3);

    assert_eq!(formatted, "values = [\n    1,\n]\n");
    finish_server(client, server_thread);
}

#[test]
fn formatting_tab_size_applies_when_initialized_indent_width_is_missing() {
    let (client, server_thread) = initialized_server_with_options(&json!({
        "format": {"lineEnding": "lf"}
    }));

    let formatted = formatted_text(&client, "values=[\n1,\n]\n", 3);

    assert_eq!(formatted, "values = [\n   1,\n]\n");
    finish_server(client, server_thread);
}

#[test]
fn formatting_tab_size_applies_when_initialized_indent_width_is_invalid() {
    let (client, server_thread) = initialized_server_with_options(&json!({
        "format": {"indentWidth": 0, "lineEnding": "lf"}
    }));

    let formatted = formatted_text(&client, "values=[\n1,\n]\n", 3);

    assert_eq!(formatted, "values = [\n   1,\n]\n");
    finish_server(client, server_thread);
}

#[test]
fn formatting_uses_core_defaults_without_initialization_options() {
    let (client, server_thread) = initialized_server();

    let formatted = formatted_text(&client, "values=[\r\n1,\r\n]\r\n", 0);

    assert_eq!(formatted, "values = [\r\n  1,\r\n]\r\n");
    finish_server(client, server_thread);
}

#[test]
fn invalid_format_configuration_and_tab_size_fall_back_safely() {
    let (client, server_thread) = initialized_server_with_options(&json!({
        "format": {
            "indentWidth": 0,
            "lineWidth": 70000,
            "lineEnding": "native"
        }
    }));

    let formatted = formatted_text(&client, "values=[\r\n1,\r\n]\r\n", 999);

    assert_eq!(formatted, "values = [\r\n  1,\r\n]\r\n");
    finish_server(client, server_thread);
}

#[test]
fn opening_and_closing_a_document_publishes_and_clears_core_diagnostics() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/example.toml";

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 7,
                    "text": "broken\n"
                }
            }),
        }))
        .unwrap();
    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didOpen must publish diagnostics")
    };
    assert_eq!(published.method, "textDocument/publishDiagnostics");
    assert_eq!(published.params["uri"], uri);
    assert_eq!(published.params["version"], 7);
    let missing_equals = published.params["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "parse.missing-equals")
        .expect("core's stable missing-equals diagnostic must be published");
    assert_eq!(
        missing_equals["range"],
        json!({
            "start": {"line": 0, "character": 6},
            "end": {"line": 0, "character": 6}
        })
    );

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didClose".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Notification(cleared) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didClose must clear diagnostics")
    };
    assert_eq!(cleared.method, "textDocument/publishDiagnostics");
    assert_eq!(cleared.params["diagnostics"], json!([]));
    assert_eq!(cleared.params.get("version"), None);

    finish_server(client, server_thread);
}

#[test]
fn incremental_changes_use_utf16_positions_and_ignore_stale_versions() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/unicode.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 5,
                    "text": "\"😀old\" = 1\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didChange".to_owned(),
            params: json!({
                "textDocument": {"uri": uri, "version": 6},
                "contentChanges": [{
                    "range": {
                        "start": {"line": 0, "character": 6},
                        "end": {"line": 0, "character": 7}
                    },
                    "text": ""
                }]
            }),
        }))
        .unwrap();
    let Message::Notification(changed) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("a fresh didChange must publish diagnostics")
    };
    assert_eq!(changed.params["version"], 6);
    assert_eq!(
        changed.params["diagnostics"][0]["code"],
        "parse.unterminated-string"
    );

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didChange".to_owned(),
            params: json!({
                "textDocument": {"uri": uri, "version": 5},
                "contentChanges": [{"text": "replacement = true\n"}]
            }),
        }))
        .unwrap();
    let Message::Notification(logged) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("a stale didChange must be reported through window/logMessage")
    };
    assert_eq!(logged.method, "window/logMessage");
    assert!(
        logged.params["message"]
            .as_str()
            .unwrap()
            .contains("stale didChange"),
        "unexpected log message: {:?}",
        logged.params
    );
    assert!(
        client
            .receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
        "a stale didChange must not replace the snapshot or republish diagnostics"
    );

    finish_server(client, server_thread);
}

#[test]
fn document_formatting_returns_core_edits_in_lsp_coordinates() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/format.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "name=\"😀x\"\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(20),
            method: "textDocument/formatting".to_owned(),
            params: json!({
                "textDocument": {"uri": uri},
                "options": {"tabSize": 2, "insertSpaces": true}
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("formatting must return a response")
    };
    assert_eq!(response.id, RequestId::from(20));
    assert_eq!(
        response.response_result.unwrap(),
        json!([{
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 1, "character": 0}
            },
            "newText": "name = \"😀x\"\n"
        }])
    );

    finish_server(client, server_thread);
}

#[test]
fn semantic_tokens_are_derived_from_core_highlights_and_encoded_as_utf16() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/highlight.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "\"😀key\"=12 # hi\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(21),
            method: "textDocument/semanticTokens/full".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("semantic tokens must return a response")
    };
    assert_eq!(
        response.response_result.unwrap(),
        json!({
            "data": [
                0, 0, 7, 0, 0,
                0, 7, 1, 6, 0,
                0, 1, 2, 3, 0,
                0, 3, 4, 5, 0
            ]
        })
    );

    finish_server(client, server_thread);
}

#[test]
fn semantic_tokens_keep_standard_types_and_distinguish_toml_structures() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/structural-highlights.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "[workspace]\nscalar=1\narray=[]\ninline={}\n[[bin]]\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(22),
            method: "textDocument/semanticTokens/full".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("semantic tokens must return a response")
    };
    assert_eq!(
        response.response_result.unwrap(),
        json!({
            "data": [
                0, 0, 1, 6, 0,
                0, 1, 9, 1, 0,
                0, 9, 1, 6, 0,
                1, 0, 6, 0, 0,
                0, 6, 1, 6, 0,
                0, 1, 1, 3, 0,
                1, 0, 5, 0, 1,
                0, 5, 1, 6, 0,
                0, 1, 1, 6, 0,
                0, 1, 1, 6, 0,
                1, 0, 6, 0, 2,
                0, 6, 1, 6, 0,
                0, 1, 1, 6, 0,
                0, 1, 1, 6, 0,
                1, 0, 1, 6, 0,
                0, 1, 1, 6, 0,
                0, 1, 3, 1, 4,
                0, 3, 1, 6, 0,
                0, 1, 1, 6, 0
            ]
        })
    );

    finish_server(client, server_thread);
}

#[test]
fn semantic_tokens_distinguish_inline_table_container_and_member_keys() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/inline-table-highlights.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "criterion = { version = \"0.8.2\" }\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(23),
            method: "textDocument/semanticTokens/full".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("semantic tokens must return a response")
    };
    assert_eq!(
        response.response_result.unwrap(),
        json!({
            "data": [
                0, 0, 9, 0, 2,
                0, 10, 1, 6, 0,
                0, 2, 1, 6, 0,
                0, 2, 7, 0, 8,
                0, 8, 1, 6, 0,
                0, 2, 7, 2, 0,
                0, 8, 1, 6, 0
            ]
        })
    );

    finish_server(client, server_thread);
}

#[test]
fn document_symbols_come_from_core_semantic_declarations() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/symbols.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "[owner]\nname = \"Tom\"\nage = 42\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    let symbols = request_document_symbols(&client, 22, uri);
    assert_eq!(symbols.as_array().unwrap().len(), 1);
    assert_eq!(symbols[0]["name"], "owner");
    assert_eq!(symbols[0]["kind"], 3);
    // The table's full range spans its body so breadcrumbs and sticky
    // scroll keep the header active inside the table; its selection range
    // stays on the key so Outline navigation excludes the brackets.
    assert_eq!(
        symbols[0]["range"],
        json!({
            "start": {"line": 0, "character": 0},
            "end": {"line": 2, "character": 8}
        })
    );
    assert_eq!(
        symbols[0]["selectionRange"],
        json!({
            "start": {"line": 0, "character": 1},
            "end": {"line": 0, "character": 6}
        })
    );
    let children = symbols[0]["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["name"], "name");
    assert_eq!(children[0]["kind"], 7);
    assert_eq!(children[1]["name"], "age");
    assert_eq!(children[1]["detail"], "integer");

    finish_server(client, server_thread);
}

#[test]
fn nested_tables_form_a_symbol_tree_with_suffix_names_and_full_extents() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/nested-symbols.toml";
    let _opened = open_document(
        &client,
        uri,
        "[a]\nx = 1\n\n[a.b]\ny = 2\n\n[a.b.c]\nz = 3\n\n[d]\nw = 4\n",
    );

    let symbols = request_document_symbols(&client, 60, uri);
    let roots = symbols.as_array().unwrap();
    assert_eq!(roots.len(), 2, "unexpected roots: {symbols:?}");

    let a = &roots[0];
    assert_eq!(a["name"], "a");
    assert_eq!(
        a["range"],
        json!({
            "start": {"line": 0, "character": 0},
            "end": {"line": 7, "character": 5}
        }),
        "the extent of [a] must cover its whole subtable chain"
    );
    let a_children = a["children"].as_array().unwrap();
    assert_eq!(a_children[0]["name"], "x");
    let b = &a_children[1];
    assert_eq!(b["name"], "b", "nested names must drop the parent prefix");
    assert_eq!(
        b["range"],
        json!({
            "start": {"line": 3, "character": 0},
            "end": {"line": 7, "character": 5}
        })
    );
    let b_children = b["children"].as_array().unwrap();
    assert_eq!(b_children[0]["name"], "y");
    let c = &b_children[1];
    assert_eq!(c["name"], "c");
    assert_eq!(c["children"].as_array().unwrap()[0]["name"], "z");

    let d = &roots[1];
    assert_eq!(d["name"], "d", "[d] must stay a sibling root, not a child");
    assert_eq!(
        d["range"],
        json!({
            "start": {"line": 9, "character": 0},
            "end": {"line": 10, "character": 5}
        })
    );
    for root in roots {
        assert_symbol_tree_invariants(root);
    }

    finish_server(client, server_thread);
}

#[test]
fn array_of_tables_instances_are_siblings_and_children_follow_the_latest() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/array-symbols.toml";
    let _opened = open_document(
        &client,
        uri,
        "[[fruit]]\nname = \"apple\"\n\n[fruit.physical]\ncolor = \"red\"\n\n[[fruit]]\nname = \"banana\"\n",
    );

    let symbols = request_document_symbols(&client, 61, uri);
    let roots = symbols.as_array().unwrap();
    assert_eq!(roots.len(), 2, "each instance needs its own symbol");
    assert_eq!(roots[0]["name"], "fruit");
    assert_eq!(roots[0]["kind"], 18);
    assert_eq!(roots[1]["name"], "fruit");

    let first_children = roots[0]["children"].as_array().unwrap();
    assert_eq!(first_children[0]["name"], "name");
    assert_eq!(first_children[1]["name"], "physical");
    assert_eq!(
        first_children[1]["children"].as_array().unwrap()[0]["name"],
        "color"
    );
    assert_eq!(
        roots[0]["range"],
        json!({
            "start": {"line": 0, "character": 0},
            "end": {"line": 4, "character": 13}
        }),
        "the first instance's extent must stop before the second instance"
    );

    let second_children = roots[1]["children"].as_array().unwrap();
    assert_eq!(second_children.len(), 1);
    assert_eq!(second_children[0]["name"], "name");
    for root in roots {
        assert_symbol_tree_invariants(root);
    }

    finish_server(client, server_thread);
}

#[test]
fn out_of_order_tables_produce_independent_roots_without_panicking() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/out-of-order-symbols.toml";
    let _opened = open_document(&client, uri, "[a.b]\nx = 1\n\n[a]\ny = 2\n");

    let symbols = request_document_symbols(&client, 62, uri);
    let roots = symbols.as_array().unwrap();
    assert_eq!(roots.len(), 2, "unexpected roots: {symbols:?}");
    assert_eq!(
        roots[0]["name"], "a.b",
        "a root without an open parent keeps its full dotted name"
    );
    assert_eq!(roots[0]["children"].as_array().unwrap()[0]["name"], "x");
    assert_eq!(roots[1]["name"], "a");
    assert_eq!(roots[1]["children"].as_array().unwrap()[0]["name"], "y");
    for root in roots {
        assert_symbol_tree_invariants(root);
    }

    finish_server(client, server_thread);
}

#[test]
fn dotted_keys_inside_a_table_stay_one_leaf_with_the_suffix_name() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/dotted-symbols.toml";
    let _opened = open_document(&client, uri, "[server]\nlimits.cpu = 4\n");

    let symbols = request_document_symbols(&client, 63, uri);
    let roots = symbols.as_array().unwrap();
    assert_eq!(roots.len(), 1);
    let children = roots[0]["children"].as_array().unwrap();
    assert_eq!(children.len(), 1, "a dotted key must stay a single leaf");
    assert_eq!(children[0]["name"], "limits.cpu");
    assert_eq!(children[0]["kind"], 7);
    assert_eq!(children[0]["detail"], "integer");

    finish_server(client, server_thread);
}

#[test]
fn symbol_tree_invariants_hold_over_a_nontrivial_document() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/invariant-symbols.toml";
    let text = "top = 1\n[a]\nx = 1\n[a.b]\ny = 2\n\n[[items]]\nid = 1\n[items.meta]\ntag = \"t\"\n\n[[items]]\nid = 2\n\n[z.later]\nq = 1\n[z]\nr.s = 2\n";
    let _opened = open_document(&client, uri, text);

    let symbols = request_document_symbols(&client, 64, uri);
    let roots = symbols.as_array().unwrap();
    let names = roots
        .iter()
        .map(|root| root["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["top", "a", "items", "items", "z.later", "z"]);
    for root in roots {
        assert_symbol_tree_invariants(root);
    }

    finish_server(client, server_thread);
}

#[test]
fn hover_resolves_core_declarations_at_utf16_positions() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/hover.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "\"😀name\" = 42\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(23),
            method: "textDocument/hover".to_owned(),
            params: json!({
                "textDocument": {"uri": uri},
                "position": {"line": 0, "character": 3}
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("hover must return a response")
    };
    assert_eq!(
        response.response_result.unwrap(),
        json!({
            "contents": {
                "kind": "plaintext",
                "value": "😀name\nTOML integer"
            },
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 13}
            }
        })
    );

    finish_server(client, server_thread);
}

#[test]
fn folding_ranges_follow_core_table_declarations() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/folding.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "[first]\na = 1\nb = 2\n\n[second]\nc = 3\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(24),
            method: "textDocument/foldingRange".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("foldingRange must return a response")
    };
    assert_eq!(
        response.response_result.unwrap(),
        json!([
            {"startLine": 0, "endLine": 3, "kind": "region"},
            {"startLine": 4, "endLine": 5, "kind": "region"}
        ])
    );

    finish_server(client, server_thread);
}

#[test]
fn folding_a_parent_table_spans_its_subtables() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/nested-folding.toml";
    let _opened = open_document(
        &client,
        uri,
        "[servers]\na = 1\n\n[servers.limits]\nb = 2\n\n[other]\nc = 3\n",
    );

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(26),
            method: "textDocument/foldingRange".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("foldingRange must return a response")
    };
    // [servers] must fold over [servers.limits] and stop before [other];
    // the subtable and the sibling keep their own ranges.
    assert_eq!(
        response.response_result.unwrap(),
        json!([
            {"startLine": 0, "endLine": 5, "kind": "region"},
            {"startLine": 3, "endLine": 5, "kind": "region"},
            {"startLine": 6, "endLine": 7, "kind": "region"}
        ])
    );

    finish_server(client, server_thread);
}

#[test]
fn folding_ranges_include_multiline_core_collection_nodes() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/collection-folding.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "values = [\n  1,\n  2,\n]\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(25),
            method: "textDocument/foldingRange".to_owned(),
            params: json!({"textDocument": {"uri": uri}}),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("foldingRange must return collection ranges")
    };
    assert_eq!(
        response.response_result.unwrap(),
        json!([{
            "startLine": 0,
            "startCharacter": 9,
            "endLine": 3,
            "endCharacter": 1,
            "kind": "region"
        }])
    );

    finish_server(client, server_thread);
}

#[test]
fn symbol_names_quote_segments_that_are_not_bare_keys() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/quoted-names.toml";
    let _opened = open_document(&client, uri, "\"a.b\" = 1\n\n[\"\"]\nx = 1\n");

    // A dotted join would render the quoted root key "a.b" like the path
    // a.b and the empty table name as an empty string, which the LSP
    // specification forbids for symbol names.
    let symbols = request_document_symbols(&client, 32, uri);
    assert_eq!(symbols[0]["name"], "\"a.b\"", "unexpected: {symbols:?}");
    assert_eq!(symbols[1]["name"], "\"\"", "unexpected: {symbols:?}");
    assert_eq!(symbols[1]["children"][0]["name"], "x");

    finish_server(client, server_thread);
}

#[test]
fn flat_symbol_names_quote_segments_that_are_not_bare_keys() {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(0),
            method: "initialize".to_owned(),
            params: json!({"capabilities": {}}),
        }))
        .unwrap();
    let _initialized = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();

    let uri = "file:///workspace/flat-quoted-names.toml";
    let _opened = open_document(&client, uri, "\"a.b\" = 1\n\n[\"\"]\nx = 1\n");

    let symbols = request_document_symbols(&client, 33, uri);
    assert_eq!(symbols[0]["name"], "\"a.b\"", "unexpected: {symbols:?}");
    assert_eq!(symbols[1]["name"], "\"\"", "unexpected: {symbols:?}");
    assert_eq!(symbols[2]["name"], "\"\".x");
    assert_eq!(symbols[2]["containerName"], "\"\"");

    finish_server(client, server_thread);
}

#[test]
fn document_symbols_fall_back_to_flat_symbol_information() {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(0),
            method: "initialize".to_owned(),
            params: json!({"capabilities": {}}),
        }))
        .unwrap();
    let _initialized = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();

    let uri = "file:///workspace/flat-symbols.toml";
    let _opened = open_document(
        &client,
        uri,
        "[owner]\nname = \"Tom\"\n\n[owner.pet]\nkind = \"cat\"\n",
    );

    let symbols = request_document_symbols(&client, 31, uri);
    assert_eq!(symbols[0]["name"], "owner");
    assert_eq!(symbols[0]["location"]["uri"], uri);
    assert!(
        symbols[0].get("selectionRange").is_none(),
        "clients without hierarchical support must receive SymbolInformation: {symbols:?}"
    );
    // Flat names stay fully dotted; the parent table is reported through
    // containerName instead of the tree shape.
    assert!(
        symbols[0].get("containerName").is_none(),
        "a root table has no container: {symbols:?}"
    );
    assert_eq!(symbols[1]["name"], "owner.name");
    assert_eq!(symbols[1]["containerName"], "owner");
    assert_eq!(symbols[2]["name"], "owner.pet");
    assert_eq!(symbols[2]["containerName"], "owner");
    assert_eq!(symbols[3]["name"], "owner.pet.kind");
    assert_eq!(symbols[3]["containerName"], "owner.pet");

    finish_server(client, server_thread);
}

#[test]
fn out_of_range_positions_are_clamped_instead_of_dropping_the_change() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/clamp.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "a = 1"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    // The end position exceeds both the line length and the line count;
    // the specification requires clamping to the end of the document.
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didChange".to_owned(),
            params: json!({
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{
                    "range": {
                        "start": {"line": 0, "character": 5},
                        "end": {"line": 9, "character": 42}
                    },
                    "text": "9"
                }]
            }),
        }))
        .unwrap();
    let Message::Notification(changed) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("a clamped didChange must still publish diagnostics")
    };
    assert_eq!(changed.method, "textDocument/publishDiagnostics");
    assert_eq!(changed.params["version"], 2);
    assert_eq!(changed.params["diagnostics"], json!([]));

    // Hover over the edited value confirms the snapshot is now "a = 19".
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(32),
            method: "textDocument/hover".to_owned(),
            params: json!({
                "textDocument": {"uri": uri},
                "position": {"line": 0, "character": 0}
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("hover must return a response")
    };
    let hover = response.response_result.unwrap();
    assert_eq!(hover["contents"]["value"], "a\nTOML integer");

    finish_server(client, server_thread);
}

#[test]
fn multiple_content_changes_apply_against_the_previously_changed_text() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/multi-change.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "a = 1\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    // The second range only lands on "1" after the first change has
    // inserted a line above it.
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didChange".to_owned(),
            params: json!({
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [
                    {
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 0}
                        },
                        "text": "b = 2\n"
                    },
                    {
                        "range": {
                            "start": {"line": 1, "character": 4},
                            "end": {"line": 1, "character": 5}
                        },
                        "text": "true"
                    }
                ]
            }),
        }))
        .unwrap();
    let Message::Notification(changed) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("didChange must publish diagnostics")
    };
    assert_eq!(changed.params["diagnostics"], json!([]));

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(33),
            method: "textDocument/hover".to_owned(),
            params: json!({
                "textDocument": {"uri": uri},
                "position": {"line": 1, "character": 0}
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("hover must return a response")
    };
    let hover = response.response_result.unwrap();
    assert_eq!(hover["contents"]["value"], "a\nTOML boolean");

    finish_server(client, server_thread);
}

#[test]
fn refused_formatting_reports_the_reason_through_log_message() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/refused.toml";
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "toml",
                    "version": 1,
                    "text": "a = \"unterminated\n"
                }
            }),
        }))
        .unwrap();
    let _opened = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(34),
            method: "textDocument/formatting".to_owned(),
            params: json!({
                "textDocument": {"uri": uri},
                "options": {"tabSize": 2, "insertSpaces": true}
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("formatting must return a response")
    };
    assert_eq!(response.response_result.unwrap(), json!([]));
    let Message::Notification(logged) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("refused formatting must send window/logMessage")
    };
    // A log entry, not window/showMessage: format-on-save of a broken file
    // must not toast the user on every save.
    assert_eq!(logged.method, "window/logMessage");
    assert!(
        logged.params["message"]
            .as_str()
            .unwrap()
            .contains("refused to format"),
        "unexpected message: {:?}",
        logged.params
    );

    finish_server(client, server_thread);
}

#[test]
fn changes_for_unopened_documents_are_reported_not_ignored() {
    let (client, server_thread) = initialized_server();
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didChange".to_owned(),
            params: json!({
                "textDocument": {"uri": "file:///workspace/ghost.toml", "version": 1},
                "contentChanges": [{"text": "a = 1\n"}]
            }),
        }))
        .unwrap();
    let Message::Notification(logged) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("a didChange for an unopened document must be logged")
    };
    assert_eq!(logged.method, "window/logMessage");
    assert!(
        logged.params["message"]
            .as_str()
            .unwrap()
            .contains("not open"),
        "unexpected log message: {:?}",
        logged.params
    );

    finish_server(client, server_thread);
}

#[test]
fn lockfile_formatting_is_skipped_by_default() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/Cargo.lock";
    let _opened = open_document(&client, uri, "a=1\n");

    assert_eq!(request_formatting(&client, 40, uri), json!([]));
    let Message::Notification(logged) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("skipped formatting must be reported through window/logMessage")
    };
    assert_eq!(logged.method, "window/logMessage");
    assert_eq!(logged.params["type"], 3, "the log entry must be INFO");
    let message = logged.params["message"].as_str().unwrap();
    assert!(
        message.contains(uri) && message.contains("tomlsmith.format.generatedFiles"),
        "the log entry must name the uri and the setting: {message:?}"
    );

    finish_server(client, server_thread);
}

#[test]
fn generated_marker_comments_skip_formatting() {
    let (client, server_thread) = initialized_server();

    let exact = "file:///workspace/exact-marker.toml";
    let _opened = open_document(
        &client,
        exact,
        "# This file is @generated by tooling.\na=1\n",
    );
    assert_eq!(request_formatting(&client, 41, exact), json!([]));
    let _logged = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    let case_insensitive = "file:///workspace/case-marker.toml";
    let _opened = open_document(
        &client,
        case_insensitive,
        "\n# Automatically Generated by cargo.\n# Do not edit.\nb=2\n",
    );
    assert_eq!(request_formatting(&client, 42, case_insensitive), json!([]));
    let _logged = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    finish_server(client, server_thread);
}

#[test]
fn bom_and_marker_spelling_variants_still_skip_formatting() {
    let (client, server_thread) = initialized_server();

    for (index, text) in [
        // a BOM must not hide the leading `#` of the marker line
        "\u{feff}# This file is @generated by tooling.\na=1\n",
        // the hyphenated spelling used by goreleaser and other generators
        "# THIS FILE IS AUTO-GENERATED BY GORELEASER. DO NOT EDIT.\nb=2\n",
        // @generated must match case-insensitively like the other markers
        "# @Generated by codegen.\nc=3\n",
    ]
    .iter()
    .enumerate()
    {
        let uri = format!("file:///workspace/marker-variant-{index}.toml");
        let _opened = open_document(&client, &uri, text);
        assert_eq!(
            request_formatting(&client, 45 + i32::try_from(index).unwrap(), &uri),
            json!([]),
            "{text:?} must be detected as generated"
        );
        let _logged = client
            .receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
    }

    finish_server(client, server_thread);
}

#[test]
fn markers_after_the_leading_comment_block_do_not_skip_formatting() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/late-marker.toml";
    let _opened = open_document(&client, uri, "a=1\n# @generated\n");

    let edits = request_formatting(&client, 43, uri);
    assert!(
        edits[0]["newText"].as_str().unwrap().contains("a = 1"),
        "a marker below the leading comment block must not stop formatting: {edits:?}"
    );

    finish_server(client, server_thread);
}

#[test]
fn generated_files_format_mode_formats_lockfiles() {
    let (client, server_thread) = initialized_server_with_options(&json!({
        "format": {"generatedFiles": "format", "lineEnding": "lf"}
    }));
    let uri = "file:///workspace/Cargo.lock";
    let _opened = open_document(&client, uri, "a=1\n");

    let edits = request_formatting(&client, 44, uri);
    assert_eq!(edits[0]["newText"], "a = 1\n");

    finish_server(client, server_thread);
}

#[test]
fn invalid_didchange_ranges_close_the_document_and_notify_the_user() {
    let (client, server_thread) = initialized_server();
    let uri = "file:///workspace/desync.toml";
    let _opened = open_document(&client, uri, "a = 1\n");

    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didChange".to_owned(),
            params: json!({
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{
                    "range": {
                        "start": {"line": 0, "character": 3},
                        "end": {"line": 0, "character": 1}
                    },
                    "text": "oops"
                }]
            }),
        }))
        .unwrap();
    let Message::Notification(logged) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("an invalid didChange range must be logged")
    };
    assert_eq!(logged.method, "window/logMessage");
    assert!(
        logged.params["message"]
            .as_str()
            .unwrap()
            .contains("end before start"),
        "unexpected log message: {:?}",
        logged.params
    );
    let Message::Notification(shown) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("closing a desynchronized document must send window/showMessage")
    };
    assert_eq!(shown.method, "window/showMessage");
    assert_eq!(shown.params["type"], 1, "the message must be ERROR");
    let message = shown.params["message"].as_str().unwrap();
    assert!(
        message.contains(uri) && message.contains("reopen"),
        "the user must be told to reopen the closed file: {message:?}"
    );

    finish_server(client, server_thread);
}

#[test]
fn duplicate_key_diagnostics_link_the_first_declaration_when_supported() {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(0),
            method: "initialize".to_owned(),
            params: json!({
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": {"relatedInformation": true}
                    }
                }
            }),
        }))
        .unwrap();
    let _initialized = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();

    let uri = "file:///workspace/duplicates.toml";
    let published = open_document(&client, uri, "a = 1\na = 2\na = 3\n");

    let duplicates = published.params["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["code"] == "semantic.duplicate-key")
        .collect::<Vec<_>>();
    assert_eq!(duplicates.len(), 2, "unexpected: {:?}", published.params);
    // Both redeclarations must point at the earliest declaration, not at
    // whichever duplicate happens to precede them.
    for duplicate in duplicates {
        assert_eq!(
            duplicate["relatedInformation"],
            json!([{
                "location": {
                    "uri": uri,
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 5}
                    }
                },
                "message": "first declared here"
            }]),
            "unexpected related information: {duplicate:?}"
        );
    }

    finish_server(client, server_thread);
}

#[test]
fn changing_the_toml_version_reparses_and_republishes_every_open_document() {
    let (client, server_thread) = initialized_server_with_options(&json!({"tomlVersion": "1.0"}));
    let first = "file:///workspace/reload-one.toml";
    let second = "file:///workspace/reload-two.toml";
    for uri in [first, second] {
        let published = open_document(&client, uri, "t = { a = 1, }\n");
        assert!(
            published.params["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "version.toml-1.1-syntax"),
            "the trailing comma must be rejected while the session is TOML 1.0: {:?}",
            published.params
        );
    }

    change_configuration(&client, &json!({"tomlVersion": "1.1"}));

    let mut republished = std::collections::HashMap::new();
    for _ in 0..2 {
        let Message::Notification(published) = client
            .receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
        else {
            panic!("a version change must republish diagnostics")
        };
        assert_eq!(published.method, "textDocument/publishDiagnostics");
        republished.insert(
            published.params["uri"].as_str().unwrap().to_owned(),
            published.params.clone(),
        );
    }
    for uri in [first, second] {
        let params = &republished[uri];
        assert_eq!(
            params["version"], 1,
            "reparsing must preserve the client version counter: {params:?}"
        );
        assert_eq!(
            params["diagnostics"],
            json!([]),
            "the trailing comma must be accepted after the switch to TOML 1.1: {params:?}"
        );
    }
    assert!(
        client
            .receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
        "a client without refreshSupport must not receive a refresh request"
    );

    finish_server(client, server_thread);
}

#[test]
fn did_change_configuration_prefers_the_tomlsmith_section_object() {
    let (client, server_thread) = initialized_server_with_options(&json!({"tomlVersion": "1.0"}));
    let uri = "file:///workspace/nested-settings.toml";
    let _opened = open_document(&client, uri, "t = { a = 1, }\n");

    change_configuration(&client, &json!({"tomlsmith": {"tomlVersion": "1.1"}}));

    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("settings nested under \"tomlsmith\" must take effect")
    };
    assert_eq!(published.method, "textDocument/publishDiagnostics");
    assert_eq!(published.params["uri"], uri);
    assert_eq!(
        published.params["diagnostics"],
        json!([]),
        "unexpected: {:?}",
        published.params
    );

    finish_server(client, server_thread);
}

#[test]
fn version_changes_request_a_semantic_token_refresh_when_supported() {
    let (client, server) = Connection::memory();
    let server_thread = thread::spawn(move || tomlsmith_lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(0),
            method: "initialize".to_owned(),
            params: json!({
                "initializationOptions": {"tomlVersion": "1.0"},
                "capabilities": {
                    "workspace": {"semanticTokens": {"refreshSupport": true}}
                }
            }),
        }))
        .unwrap();
    let _initialized = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }))
        .unwrap();
    let uri = "file:///workspace/refresh.toml";
    let _opened = open_document(&client, uri, "a = 1\n");

    change_configuration(&client, &json!({"tomlVersion": "1.1"}));

    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("the version change must republish diagnostics")
    };
    assert_eq!(published.method, "textDocument/publishDiagnostics");
    let Message::Request(refresh) = client
        .receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
    else {
        panic!("a version change must request workspace/semanticTokens/refresh")
    };
    assert_eq!(refresh.method, "workspace/semanticTokens/refresh");
    assert_eq!(refresh.params, Value::Null);

    // The client's answer to the server-initiated request must be ignored
    // without disturbing the session.
    client
        .sender
        .send(Message::Response(Response::new_ok(refresh.id, Value::Null)))
        .unwrap();

    // A restatement that only adjusts format options keeps the version, so
    // it must trigger neither a reparse nor another refresh request.
    change_configuration(
        &client,
        &json!({"tomlVersion": "1.1", "format": {"lineWidth": 100}}),
    );
    assert!(
        client
            .receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
        "format-only configuration changes must not republish or refresh"
    );

    // The session must still answer requests after the ignored response.
    assert_eq!(request_formatting(&client, 50, uri), json!([]));

    finish_server(client, server_thread);
}

#[test]
fn format_option_changes_apply_to_subsequent_formatting_requests() {
    let (client, server_thread) = initialized_server();

    assert_eq!(
        formatted_text(&client, "values=[\n1,\n]\n", 2),
        "values = [\n  1,\n]\n"
    );

    change_configuration(&client, &json!({"format": {"indentWidth": 4}}));

    // tomlVersion stayed at the 1.1 default, so no republish precedes the
    // next exchange; the new indent width must now beat the client tabSize.
    assert_eq!(
        formatted_text(&client, "values=[\n1,\n]\n", 2),
        "values = [\n    1,\n]\n"
    );

    finish_server(client, server_thread);
}

#[test]
fn duplicate_key_diagnostics_omit_related_information_without_client_support() {
    let (client, server_thread) = initialized_server();
    let published = open_document(
        &client,
        "file:///workspace/unsupported-duplicates.toml",
        "a = 1\na = 2\n",
    );

    let duplicate = published.params["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "semantic.duplicate-key")
        .expect("the duplicate-key diagnostic must be published");
    assert!(
        duplicate.get("relatedInformation").is_none(),
        "clients without the capability must not receive related information: {duplicate:?}"
    );

    finish_server(client, server_thread);
}
