//! Adversarial protocol regressions: configuration restatement shapes,
//! refresh-request bookkeeping, related-information targeting, symbol
//! invariants, formatting edit hygiene and minimality, and folding
//! hierarchy.

use std::{fmt::Write as _, thread, time::Duration};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{Value, json};

fn init(
    capabilities: &Value,
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
                "capabilities": capabilities,
                "initializationOptions": initialization_options
            }),
        }))
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(5))
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

fn open(client: &Connection, uri: &str, text: &str) -> Notification {
    client
        .sender
        .send(Message::Notification(Notification {
            method: "textDocument/didOpen".to_owned(),
            params: json!({
                "textDocument": {
                    "uri": uri, "languageId": "toml", "version": 1, "text": text
                }
            }),
        }))
        .unwrap();
    let Message::Notification(published) = client
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("didOpen must publish diagnostics")
    };
    assert_eq!(published.method, "textDocument/publishDiagnostics");
    published
}

fn finish(client: Connection, server_thread: thread::JoinHandle<tomlsmith_lsp::ServerResult>) {
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(999),
            method: "shutdown".to_owned(),
            params: Value::Null,
        }))
        .unwrap();
    loop {
        match client.receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(Message::Response(response)) if response.id == RequestId::from(999) => break,
            Ok(_) => {}
            Err(error) => panic!("no shutdown response: {error}"),
        }
    }
    client
        .sender
        .send(Message::Notification(Notification {
            method: "exit".to_owned(),
            params: Value::Null,
        }))
        .unwrap();
    drop(client);
    server_thread.join().unwrap().unwrap();
}

fn request(client: &Connection, id: i32, method: &str, params: Value) -> Response {
    client
        .sender
        .send(Message::Request(Request {
            id: RequestId::from(id),
            method: method.to_owned(),
            params,
        }))
        .unwrap();
    loop {
        match client
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        {
            Message::Response(response) if response.id == RequestId::from(id) => return response,
            Message::Notification(_) => {}
            other => panic!("unexpected message while waiting for {method}: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// didChangeConfiguration payload shapes
// ---------------------------------------------------------------------------

/// A client that uses the pull model (or eglot with no local config) sends
/// `settings: null` or an empty object as a mere change signal; that must
/// not wipe the options the client passed through initializationOptions.
#[test]
fn null_or_empty_settings_restatements_keep_initialization_options() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.1"}));
    let uri = "file:///workspace/wipe-null.toml";
    let published = open(&client, uri, "escape = \"\\e\"\n");
    assert_eq!(
        published.params["diagnostics"],
        json!([]),
        "1.1 opt-in from initializationOptions must hold"
    );

    for settings in [json!(null), json!({}), json!({"tomlsmith": {}})] {
        client
            .sender
            .send(Message::Notification(Notification {
                method: "workspace/didChangeConfiguration".to_owned(),
                params: json!({"settings": settings}),
            }))
            .unwrap();

        // A wipe to a different version would reparse and republish with a
        // version.toml-1.1-syntax error.
        match client.receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(Message::Notification(republished)) => {
                assert_eq!(
                    republished.params["diagnostics"],
                    json!([]),
                    "an empty restatement must not wipe the 1.1 opt-in"
                );
            }
            Ok(other) => panic!("unexpected message: {other:?}"),
            Err(_) => {}
        }
    }
    finish(client, server_thread);
}

#[test]
fn array_settings_restatement_keeps_initialization_options() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.1"}));
    let uri = "file:///workspace/wipe-array.toml";
    let published = open(&client, uri, "escape = \"\\e\"\n");
    assert_eq!(published.params["diagnostics"], json!([]));

    client
        .sender
        .send(Message::Notification(Notification {
            method: "workspace/didChangeConfiguration".to_owned(),
            params: json!({"settings": [1, 2, 3]}),
        }))
        .unwrap();
    match client.receiver.recv_timeout(Duration::from_millis(500)) {
        Ok(Message::Notification(republished)) => {
            assert_eq!(
                republished.params["diagnostics"],
                json!([]),
                "a non-object restatement must not wipe the 1.1 opt-in"
            );
        }
        Ok(other) => panic!("unexpected message: {other:?}"),
        Err(_) => {}
    }
    finish(client, server_thread);
}

/// `params` missing the `settings` member entirely must be dropped with a
/// log instead of resetting the session options.
#[test]
fn missing_settings_member_is_dropped_with_a_log() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.1"}));
    let uri = "file:///workspace/missing-settings.toml";
    let published = open(&client, uri, "escape = \"\\e\"\n");
    assert_eq!(published.params["diagnostics"], json!([]));

    client
        .sender
        .send(Message::Notification(Notification {
            method: "workspace/didChangeConfiguration".to_owned(),
            params: json!({}),
        }))
        .unwrap();
    match client.receiver.recv_timeout(Duration::from_millis(500)) {
        Ok(Message::Notification(notification)) => {
            assert_eq!(notification.method, "window/logMessage");
        }
        Ok(other) => panic!("unexpected message: {other:?}"),
        Err(_) => {}
    }
    finish(client, server_thread);
}

// ---------------------------------------------------------------------------
// semanticTokens refresh ids
// ---------------------------------------------------------------------------

#[test]
fn refresh_request_ids_are_unique_and_responses_are_tolerated() {
    let capabilities = json!({"workspace": {"semanticTokens": {"refreshSupport": true}}});
    let (client, server_thread) = init(&capabilities, &json!({"tomlVersion": "1.0"}));
    let uri = "file:///workspace/refresh.toml";
    let _published = open(&client, uri, "a = 1\n");

    let mut refresh_ids = Vec::new();
    for version in ["1.1", "1.0"] {
        client
            .sender
            .send(Message::Notification(Notification {
                method: "workspace/didChangeConfiguration".to_owned(),
                params: json!({"settings": {"tomlsmith": {"tomlVersion": version}}}),
            }))
            .unwrap();
        // one publishDiagnostics per open document, then the refresh request
        loop {
            match client
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                Message::Notification(notification) => {
                    assert_eq!(notification.method, "textDocument/publishDiagnostics");
                }
                Message::Request(request) => {
                    assert_eq!(request.method, "workspace/semanticTokens/refresh");
                    refresh_ids.push(request.id.clone());
                    break;
                }
                other @ Message::Response(_) => panic!("unexpected message: {other:?}"),
            }
        }
    }
    assert_eq!(refresh_ids.len(), 2);
    assert_ne!(refresh_ids[0], refresh_ids[1], "refresh ids must be unique");

    // Respond to one refresh, leave the other unanswered; then respond with
    // an id the server never issued. The server must stay functional.
    client
        .sender
        .send(Message::Response(Response::new_ok(
            refresh_ids[0].clone(),
            Value::Null,
        )))
        .unwrap();
    client
        .sender
        .send(Message::Response(Response::new_ok(
            RequestId::from(424_242),
            Value::Null,
        )))
        .unwrap();
    let response = request(
        &client,
        50,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}}),
    );
    assert!(
        response.response_result.is_ok(),
        "server must remain functional"
    );
    finish(client, server_thread);
}

/// No refresh may be sent when the client never declared refreshSupport.
#[test]
fn no_refresh_without_declared_support() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.0"}));
    let uri = "file:///workspace/no-refresh.toml";
    let _published = open(&client, uri, "a = 1\n");
    client
        .sender
        .send(Message::Notification(Notification {
            method: "workspace/didChangeConfiguration".to_owned(),
            params: json!({"settings": {"tomlsmith": {"tomlVersion": "1.1"}}}),
        }))
        .unwrap();
    // Expect exactly one publishDiagnostics and no Request.
    match client
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    {
        Message::Notification(notification) => {
            assert_eq!(notification.method, "textDocument/publishDiagnostics");
        }
        other => panic!("unexpected message: {other:?}"),
    }
    match client.receiver.recv_timeout(Duration::from_millis(300)) {
        Err(_) => {}
        Ok(Message::Request(request)) => {
            panic!("refresh sent without refreshSupport: {}", request.method)
        }
        Ok(other) => panic!("unexpected message: {other:?}"),
    }
    finish(client, server_thread);
}

// ---------------------------------------------------------------------------
// relatedInformation targeting
// ---------------------------------------------------------------------------

#[test]
fn related_info_for_duplicates_inside_a_second_aot_instance() {
    let capabilities =
        json!({"textDocument": {"publishDiagnostics": {"relatedInformation": true}}});
    let (client, server_thread) = init(&capabilities, &json!({}));
    let uri = "file:///workspace/aot-dup.toml";
    // The duplicate is name=banana vs name=cherry inside the SECOND
    // [[fruit]] instance; name=apple in the first instance is unrelated.
    let text = "[[fruit]]\nname = \"apple\"\n\n[[fruit]]\nname = \"banana\"\nname = \"cherry\"\n";
    let published = open(&client, uri, text);
    let diagnostics = published.params["diagnostics"].as_array().unwrap();
    let duplicate = diagnostics
        .iter()
        .find(|d| d["code"] == "semantic.duplicate-key")
        .expect("duplicate diagnostic expected");
    let related = duplicate["relatedInformation"]
        .as_array()
        .expect("relatedInformation expected");
    let target_line = related[0]["location"]["range"]["start"]["line"]
        .as_u64()
        .unwrap();
    assert_eq!(
        target_line, 4,
        "the first conflicting declaration is name=\"banana\" on line 4, \
         not name=\"apple\" in the earlier array-of-tables instance"
    );
    finish(client, server_thread);
}

#[test]
fn related_info_does_not_cross_quoted_key_spelling() {
    let capabilities =
        json!({"textDocument": {"publishDiagnostics": {"relatedInformation": true}}});
    let (client, server_thread) = init(&capabilities, &json!({}));
    let uri = "file:///workspace/quoted-dup.toml";
    // "a.b" at root is a single key whose NAME contains a dot; it is not
    // the same entity as key b in table [a]. The duplicate is b=1 vs b=2.
    let text = "\"a.b\" = 1\n[a]\nb = 1\nb = 2\n";
    let published = open(&client, uri, text);
    let diagnostics = published.params["diagnostics"].as_array().unwrap();
    let duplicate = diagnostics
        .iter()
        .find(|d| d["code"] == "semantic.duplicate-key")
        .expect("duplicate diagnostic expected");
    let related = duplicate["relatedInformation"]
        .as_array()
        .expect("relatedInformation expected");
    let target_line = related[0]["location"]["range"]["start"]["line"]
        .as_u64()
        .unwrap();
    assert_eq!(
        target_line, 2,
        "first declaration of a.b (table key) is on line 2; line 0 is the quoted root key"
    );
    finish(client, server_thread);
}

// ---------------------------------------------------------------------------
// DocumentSymbol invariants under adversarial shapes
// ---------------------------------------------------------------------------

fn pos_le(left: &Value, right: &Value) -> bool {
    let (ll, lc) = (
        left["line"].as_u64().unwrap(),
        left["character"].as_u64().unwrap(),
    );
    let (rl, rc) = (
        right["line"].as_u64().unwrap(),
        right["character"].as_u64().unwrap(),
    );
    ll < rl || (ll == rl && lc <= rc)
}

fn range_contains(outer: &Value, inner: &Value) -> bool {
    pos_le(&outer["start"], &inner["start"]) && pos_le(&inner["end"], &outer["end"])
}

fn assert_invariants(symbol: &Value, path: &str) {
    let name = symbol["name"].as_str().unwrap_or("?");
    assert!(
        !name.is_empty(),
        "symbol at {path} has an empty name: {symbol}"
    );
    assert!(
        range_contains(&symbol["range"], &symbol["selectionRange"]),
        "selectionRange must be inside range at {path}/{name}: {symbol}"
    );
    if let Some(children) = symbol["children"].as_array() {
        for child in children {
            assert!(
                range_contains(&symbol["range"], &child["range"]),
                "child {} not contained in parent {path}/{name}: parent {:?} child {:?}",
                child["name"],
                symbol["range"],
                child["range"]
            );
            assert_invariants(child, &format!("{path}/{name}"));
        }
    }
}

#[test]
fn symbol_invariants_survive_adversarial_documents() {
    let capabilities = json!({
        "textDocument": {"documentSymbol": {"hierarchicalDocumentSymbolSupport": true}}
    });
    let (client, server_thread) = init(&capabilities, &json!({"tomlVersion": "1.1"}));
    let documents = [
        // out-of-order parent after child, keys on both
        "[a.b]\nx = 1\n[a]\ny = 2\nz = \"\u{1F600}\u{1F600}\"\n",
        // duplicate table reopening after an unrelated sibling
        "[a]\nx = 1\n[b]\nq = 1\n[a]\ny = 2\n",
        // key/table conflicts retained: key a.b then table [a.b]
        "[a]\nb = 1\n[a.b]\nc = 2\n",
        // table [a.b] then later key a.b via dotted key in [a]
        "[a.b]\nx = 2\n[a]\nb.y = 1\n",
        // AoT with nested AoT and reopening
        "[[f]]\n[[f.v]]\nn = 1\n[[f]]\n[[f.v]]\nn = 2\n",
        // dotted keys implying parents at root before an explicit table
        "root.sub.key = 1\n[root]\nother = 2\n[root.sub]\nk = 3\n",
        // multibyte in keys and header names
        "[\"\u{00E9}\u{1F600}\"]\n\"\u{00E9}\" = \"\u{1F600}\"\n",
        // empty-name quoted table and key
        "[\"\"]\nx = 1\n",
        // CRLF with no trailing newline
        "[a]\r\nx = 1\r\n[a.b]\r\ny = 2",
    ];
    for (index, text) in documents.iter().enumerate() {
        let uri = format!("file:///workspace/symbols-{index}.toml");
        let _published = open(&client, &uri, text);
        let response = request(
            &client,
            100 + i32::try_from(index).unwrap(),
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        );
        let symbols = response.response_result.expect("documentSymbol result");
        for symbol in symbols.as_array().unwrap_or(&Vec::new()) {
            assert_invariants(symbol, &format!("doc{index}"));
        }
    }
    finish(client, server_thread);
}

// ---------------------------------------------------------------------------
// Formatting TextEdit hygiene: sorted, non-overlapping, in-bounds, applicable
// ---------------------------------------------------------------------------

/// Byte offset for a UTF-16 (line, character) position, mirroring the
/// LSP position encoding the server advertises.
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
    assert!(
        utf16 >= character,
        "character {character} beyond line {line}"
    );
    offset + line_text.len()
}

fn apply_edits(text: &str, edits: &[Value]) -> String {
    // Verify sorted and non-overlapping, then apply back-to-front.
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
        assert!(end <= text.len(), "edit beyond document end: {edit}");
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

#[test]
fn formatting_edits_apply_cleanly_and_are_idempotent() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.0"}));
    let documents = [
        // CRLF, messy spacing, no trailing newline
        "[  a  ]\r\nx=1\r\ny   = [1,\r\n 2,3]\r\nemoji=\"\u{1F600} caf\u{00E9}\"",
        // LF with multibyte BEFORE regions needing edits on the same line;
        // the non-ASCII key must be quoted to stay valid TOML 1.0
        "\"k\u{00E9}y\" = {  a=1,b = 2 }\n\"\u{1F600}\" =    3\n",
        // no trailing newline, single line
        "a   =1",
        // blank lines and comments around tables
        "# top\n\n\n[t]\n\n  x = 1\n\n[t.u]\n y=2\n",
    ];
    for (index, text) in documents.iter().enumerate() {
        let uri = format!("file:///workspace/format-{index}.toml");
        let _published = open(&client, &uri, text);
        let response = request(
            &client,
            200 + i32::try_from(index).unwrap(),
            "textDocument/formatting",
            json!({
                "textDocument": {"uri": uri},
                "options": {"tabSize": 2, "insertSpaces": true}
            }),
        );
        let edits = response.response_result.expect("formatting result");
        let edits = edits.as_array().expect("formatting must return edits");
        // Every fixture is deliberately messy, so a refused or empty format
        // is a failure here rather than a silently skipped iteration.
        assert!(
            !edits.is_empty(),
            "doc{index} must produce formatting edits"
        );
        let formatted = apply_edits(text, edits);

        // Idempotence: reformatting the applied result must be a no-op.
        let uri2 = format!("file:///workspace/format-{index}-second.toml");
        let _published = open(&client, &uri2, &formatted);
        let response = request(
            &client,
            300 + i32::try_from(index).unwrap(),
            "textDocument/formatting",
            json!({
                "textDocument": {"uri": uri2},
                "options": {"tabSize": 2, "insertSpaces": true}
            }),
        );
        let second = response.response_result.expect("second formatting result");
        assert!(
            second.as_array().is_none_or(Vec::is_empty),
            "formatting is not idempotent for doc{index}: {second}"
        );
    }
    finish(client, server_thread);
}

// ---------------------------------------------------------------------------
// Formatting edit minimality: unchanged lines are never resent
// ---------------------------------------------------------------------------

fn formatting_edits(client: &Connection, id: i32, uri: &str) -> Vec<Value> {
    let response = request(
        client,
        id,
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri},
            "options": {"tabSize": 2, "insertSpaces": true}
        }),
    );
    response
        .response_result
        .expect("formatting result")
        .as_array()
        .expect("formatting must return edits")
        .clone()
}

#[test]
fn a_single_misformatted_middle_line_yields_one_line_scoped_edit() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.0"}));
    let uri = "file:///workspace/minimal-middle.toml";
    let text = "[table]\nfirst = 1\nsecond=2\nthird = 3\n";
    let _published = open(&client, uri, text);

    let edits = formatting_edits(&client, 500, uri);
    assert_eq!(edits.len(), 1, "only the middle line changed: {edits:?}");
    assert_eq!(
        edits[0]["range"],
        json!({
            "start": {"line": 2, "character": 0},
            "end": {"line": 3, "character": 0}
        }),
        "the edit must not touch the unchanged first and last lines"
    );
    assert_eq!(edits[0]["newText"], "second = 2\n");
    assert_eq!(
        apply_edits(text, &edits),
        "[table]\nfirst = 1\nsecond = 2\nthird = 3\n"
    );
    finish(client, server_thread);
}

#[test]
fn interleaved_indent_fixes_produce_scattered_edits_not_one_replacement() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.0"}));
    let uri = "file:///workspace/minimal-stanzas.toml";
    // The measured Cargo.lock defect shape: every stanza carries one
    // under-indented dependency line; everything else is already formatted.
    let mut text = String::new();
    let mut expected = String::new();
    for stanza in 0..80 {
        let head = format!(
            "[[package]]\nname = \"package-{stanza}\"\nversion = \"1.0.{stanza}\"\ndependencies = [\n"
        );
        text.push_str(&head);
        expected.push_str(&head);
        write!(text, " \"dep-{stanza}\",\n]\n\n").unwrap();
        write!(expected, "  \"dep-{stanza}\",\n]\n\n").unwrap();
    }
    let _published = open(&client, uri, &text);

    let edits = formatting_edits(&client, 501, uri);
    assert!(
        edits.len() >= 60,
        "expected roughly one edit per changed stanza, got {}",
        edits.len()
    );
    let replaced: usize = edits
        .iter()
        .map(|edit| edit["newText"].as_str().unwrap().len())
        .sum();
    assert!(
        replaced * 4 < text.len(),
        "{replaced} replacement bytes for a {}-byte document",
        text.len()
    );
    assert_eq!(apply_edits(&text, &edits), expected);
    finish(client, server_thread);
}

#[test]
fn crlf_documents_receive_line_scoped_crlf_edits() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.0"}));
    let uri = "file:///workspace/minimal-crlf.toml";
    let text = "alpha = 1\r\nbeta=2\r\ngamma = 3\r\n";
    let _published = open(&client, uri, text);

    let edits = formatting_edits(&client, 502, uri);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(
        edits[0]["range"],
        json!({
            "start": {"line": 1, "character": 0},
            "end": {"line": 2, "character": 0}
        })
    );
    assert_eq!(edits[0]["newText"], "beta = 2\r\n");
    assert_eq!(
        apply_edits(text, &edits),
        "alpha = 1\r\nbeta = 2\r\ngamma = 3\r\n"
    );
    finish(client, server_thread);
}

#[test]
fn a_changed_final_line_without_trailing_newline_stays_line_scoped() {
    let (client, server_thread) = init(&json!({}), &json!({"tomlVersion": "1.0"}));
    let uri = "file:///workspace/minimal-untrailed.toml";
    let text = "alpha = 1\nbeta=2";
    let _published = open(&client, uri, text);

    let edits = formatting_edits(&client, 503, uri);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(
        edits[0]["range"]["start"],
        json!({"line": 1, "character": 0}),
        "the unchanged first line must stay untouched: {edits:?}"
    );
    assert_eq!(apply_edits(text, &edits), "alpha = 1\nbeta = 2");
    finish(client, server_thread);
}

// ---------------------------------------------------------------------------
// Folding hierarchy regression probes
// ---------------------------------------------------------------------------

#[test]
fn parent_fold_stops_at_reopened_duplicate_and_out_of_order_tables() {
    let (client, server_thread) = init(&json!({}), &json!({}));
    let uri = "file:///workspace/folds.toml";
    // [a] ... [a.b] ... [a] (duplicate reopen) — the first [a]'s fold must
    // not swallow the second [a]; the out-of-order [z.y] then [z] pair must
    // not swallow each other either.
    let text = "[a]\nx = 1\n[a.b]\ny = 2\n[a]\nz = 3\n[z.y]\nq = 1\n[z]\nr = 2\n";
    let _published = open(&client, uri, text);
    let response = request(
        &client,
        400,
        "textDocument/foldingRange",
        json!({"textDocument": {"uri": uri}}),
    );
    let folds = response.response_result.unwrap();
    let folds = folds.as_array().unwrap();
    // first [a] header on line 0 must end before line 4 (second [a]).
    let first_a = folds
        .iter()
        .find(|fold| fold["startLine"] == 0)
        .expect("fold for the first [a]");
    assert_eq!(
        first_a["endLine"], 3,
        "first [a] fold must stop before the reopened [a]"
    );
    let z_y = folds
        .iter()
        .find(|fold| fold["startLine"] == 6)
        .expect("fold for [z.y]");
    assert_eq!(
        z_y["endLine"], 7,
        "[z.y] fold must stop before the out-of-order parent [z]"
    );
    finish(client, server_thread);
}
