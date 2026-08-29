use std::{thread, time::Duration};

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{Value, json};

fn initialized_server() -> (Connection, thread::JoinHandle<tomlsmith_lsp::ServerResult>) {
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
    response.response_result.unwrap()[0]["newText"]
        .as_str()
        .expect("formatting should return changed text")
        .to_owned()
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

    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(22),
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
    let symbols = response.response_result.unwrap();
    assert_eq!(symbols[0]["name"], "owner");
    assert_eq!(symbols[0]["kind"], 3);
    assert_eq!(symbols[1]["name"], "owner.name");
    assert_eq!(symbols[1]["kind"], 7);
    assert_eq!(symbols[2]["name"], "owner.age");
    assert_eq!(symbols[2]["detail"], "integer");

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
