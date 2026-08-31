#![forbid(unsafe_code)]

use std::{collections::HashMap, error::Error, panic::AssertUnwindSafe};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    DiagnosticRelatedInformation, DiagnosticSeverity, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    FoldingRange, FoldingRangeKind, FoldingRangeParams, FoldingRangeProviderCapability, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeResult, Location,
    LogMessageParams, MarkupContent, MarkupKind, MessageType, NumberOrString, OneOf, Position,
    PositionEncodingKind, PublishDiagnosticsParams, Range, SemanticToken, SemanticTokenType,
    SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensParams, ServerCapabilities, ServerInfo, ShowMessageParams, SymbolInformation,
    SymbolKind, TextDocumentSyncCapability, TextDocumentSyncKind, Uri, WorkDoneProgressOptions,
};
use tomlsmith::{
    Declaration, DeclarationKind, DiagnosticCode, Document, FormatOptions, FormatOutcome,
    HighlightKind, LineEnding, SemanticValue, Severity, SyntaxKind, SyntaxNode, TextRange,
    TomlVersion,
};

pub type ServerResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

/// Runs one Language Server Protocol session over the supplied transport.
///
/// # Errors
///
/// Returns an error when the initialize/shutdown handshake is invalid, when
/// protocol values cannot be serialized, or when the client sends `exit`
/// without a preceding `shutdown` request.
pub fn serve(connection: &Connection) -> ServerResult {
    let (initialize_id, initialize_params) = connection.initialize_start()?;
    let initialize_result = InitializeResult {
        capabilities: capabilities(),
        server_info: Some(ServerInfo {
            name: "TomlSmith".to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
    };
    connection.initialize_finish(initialize_id, serde_json::to_value(initialize_result)?)?;

    let options = resolve_options(
        initialize_params
            .pointer("/initializationOptions")
            .unwrap_or(&serde_json::Value::Null),
    );
    let mut session = Session {
        documents: HashMap::new(),
        toml_version: options.toml_version,
        format_options: options.format_options,
        generated_files: options.generated_files,
        configured_indent_width: options.configured_indent_width,
        hierarchical_symbols: initialize_hierarchical_symbols(&initialize_params),
        related_information: initialize_related_information(&initialize_params),
        semantic_tokens_refresh: initialize_semantic_tokens_refresh(&initialize_params),
        next_request_id: 1,
    };
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                let request_id = request.id.clone();
                // A panicking handler must fail its own request, not the
                // whole session; every open document would lose language
                // support otherwise.
                let (response, notifications) =
                    std::panic::catch_unwind(AssertUnwindSafe(|| session.handle_request(request)))
                        .unwrap_or_else(|panic| {
                            (
                                Response::new_err(
                                    request_id,
                                    ErrorCode::InternalError as i32,
                                    format!("request handler panicked: {}", panic_text(&*panic)),
                                ),
                                Vec::new(),
                            )
                        });
                if connection.sender.send(response.into()).is_err() {
                    return Ok(());
                }
                for notification in notifications {
                    if connection.sender.send(notification.into()).is_err() {
                        return Ok(());
                    }
                }
            }
            Message::Notification(notification) if notification.method == "exit" => {
                // The specification requires a non-zero exit when `exit`
                // arrives without a `shutdown` handshake.
                return Err("the client sent exit before shutdown".into());
            }
            Message::Notification(notification) => {
                let outbound = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    session.handle_notification(notification)
                }))
                .unwrap_or_else(|panic| {
                    vec![
                        log_message(
                            MessageType::ERROR,
                            format!("notification handler panicked: {}", panic_text(&*panic)),
                        )
                        .into(),
                    ]
                });
                for message in outbound {
                    if connection.sender.send(message).is_err() {
                        return Ok(());
                    }
                }
            }
            // Responses answer server-initiated requests (currently only
            // workspace/semanticTokens/refresh), and none of those carry a
            // payload the server acts on.
            Message::Response(_) => {}
        }
    }

    Ok(())
}

struct Session {
    documents: HashMap<Uri, OpenDocument>,
    toml_version: TomlVersion,
    format_options: FormatOptions,
    generated_files: GeneratedFiles,
    configured_indent_width: Option<u8>,
    hierarchical_symbols: bool,
    related_information: bool,
    semantic_tokens_refresh: bool,
    /// Id source for server-initiated requests; a separate JSON-RPC id
    /// namespace from the ids the client picks for its own requests.
    next_request_id: i32,
}

/// Resolved `format.generatedFiles` setting: whether formatting requests
/// for generated files are honored or declined.
#[derive(Clone, Copy, Eq, PartialEq)]
enum GeneratedFiles {
    Skip,
    Format,
}

struct OpenDocument {
    version: i32,
    document: Document,
    line_index: LineIndex,
}

impl OpenDocument {
    fn new(version: i32, document: Document) -> Self {
        let line_index = LineIndex::new(document.text());
        Self {
            version,
            document,
            line_index,
        }
    }
}

impl Session {
    fn handle_request(&self, request: Request) -> (Response, Vec<Notification>) {
        let mut notifications = Vec::new();
        let response = match request.method.as_str() {
            "textDocument/formatting" => {
                self.handle_formatting(request.id, request.params, &mut notifications)
            }
            "textDocument/semanticTokens/full" => {
                let id = request.id;
                let params: SemanticTokensParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return (invalid_params(id, &error), notifications),
                };
                let Some(open) = self.documents.get(&params.text_document.uri) else {
                    return (
                        Response::new_ok(id, Option::<SemanticTokens>::None),
                        notifications,
                    );
                };
                Response::new_ok(id, semantic_tokens(&open.document, &open.line_index))
            }
            "textDocument/documentSymbol" => {
                let id = request.id;
                let params: DocumentSymbolParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return (invalid_params(id, &error), notifications),
                };
                let uri = params.text_document.uri;
                let Some(open) = self.documents.get(&uri) else {
                    return (
                        Response::new_ok(id, Option::<DocumentSymbolResponse>::None),
                        notifications,
                    );
                };
                Response::new_ok(
                    id,
                    document_symbols(
                        &open.document,
                        &open.line_index,
                        &uri,
                        self.hierarchical_symbols,
                    ),
                )
            }
            "textDocument/hover" => {
                let id = request.id;
                let params: HoverParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return (invalid_params(id, &error), notifications),
                };
                let text_document_position = params.text_document_position_params;
                let Some(open) = self
                    .documents
                    .get(&text_document_position.text_document.uri)
                else {
                    return (Response::new_ok(id, Option::<Hover>::None), notifications);
                };
                Response::new_ok(
                    id,
                    hover(
                        &open.document,
                        &open.line_index,
                        text_document_position.position,
                    ),
                )
            }
            "textDocument/foldingRange" => {
                let id = request.id;
                let params: FoldingRangeParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return (invalid_params(id, &error), notifications),
                };
                let Some(open) = self.documents.get(&params.text_document.uri) else {
                    return (
                        Response::new_ok(id, Option::<Vec<FoldingRange>>::None),
                        notifications,
                    );
                };
                Response::new_ok(id, folding_ranges(&open.document, &open.line_index))
            }
            _ => Response::new_err(
                request.id,
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request `{}`", request.method),
            ),
        };
        (response, notifications)
    }

    fn handle_formatting(
        &self,
        id: lsp_server::RequestId,
        params: serde_json::Value,
        notifications: &mut Vec<Notification>,
    ) -> Response {
        let params: DocumentFormattingParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => return invalid_params(id, &error),
        };
        let Some(open) = self.documents.get(&params.text_document.uri) else {
            return Response::new_ok(id, Option::<Vec<lsp_types::TextEdit>>::None);
        };
        Response::new_ok(
            id,
            self.formatting_edits(
                &params.text_document.uri,
                open,
                params.options.tab_size,
                notifications,
            ),
        )
    }

    fn formatting_edits(
        &self,
        uri: &Uri,
        open: &OpenDocument,
        tab_size: u32,
        notifications: &mut Vec<Notification>,
    ) -> Vec<lsp_types::TextEdit> {
        // Lockfiles and other generated TOML files have their own canonical
        // style; reformatting them only creates VCS churn (a real Cargo.lock
        // produced a 262-line diff).
        if self.generated_files == GeneratedFiles::Skip
            && is_generated_document(uri, open.document.text())
        {
            notifications.push(log_message(
                MessageType::INFO,
                format!(
                    "skipped formatting {} because it is a generated file \
                     (tomlsmith.format.generatedFiles = \"skip\")",
                    uri.as_str()
                ),
            ));
            return Vec::new();
        }
        let mut format_options = self.format_options.clone();
        format_options.target_version = open.document.version();
        let requested_indent = self
            .configured_indent_width
            .is_none()
            .then(|| u8::try_from(tab_size).ok())
            .flatten()
            .filter(|tab_size| *tab_size > 0);
        if let Some(tab_size) = requested_indent {
            format_options.indent_width = tab_size;
        }
        match open.document.format_with(&format_options) {
            FormatOutcome::Unchanged => Vec::new(),
            FormatOutcome::Refused { diagnostics } => {
                let detail = diagnostics
                    .first()
                    .map(|diagnostic| format!(": {}", diagnostic.message()))
                    .unwrap_or_default();
                // A log entry instead of a popup: format-on-save of a broken
                // file would otherwise toast the user on every save.
                notifications.push(log_message(
                    MessageType::WARNING,
                    format!(
                        "TomlSmith refused to format the document because it has errors{detail}"
                    ),
                ));
                Vec::new()
            }
            // The core reports one whole-document edit; narrowing it to the
            // changed line runs keeps cursors stable in clients that do not
            // diff on their own and stops resending unchanged content.
            FormatOutcome::Changed { text, .. } => {
                let original = open.document.text();
                minimal_line_edits(original, &text)
                    .into_iter()
                    .map(|edit| lsp_types::TextEdit {
                        range: to_lsp_range(
                            original,
                            &open.line_index,
                            TextRange::new(
                                saturating_u32(edit.old_start),
                                saturating_u32(edit.old_end),
                            ),
                        ),
                        new_text: text[edit.new_start..edit.new_end].to_owned(),
                    })
                    .collect()
            }
        }
    }

    fn handle_notification(&mut self, notification: Notification) -> Vec<Message> {
        match notification.method.as_str() {
            "textDocument/didOpen" => {
                let params: DidOpenTextDocumentParams =
                    match serde_json::from_value(notification.params) {
                        Ok(params) => params,
                        Err(error) => return vec![dropped_notification("didOpen", &error).into()],
                    };
                let uri = params.text_document.uri;
                let version = params.text_document.version;
                let document = Document::parse_as(params.text_document.text, self.toml_version);
                let open = OpenDocument::new(version, document);
                let published = publish_diagnostics(
                    uri.clone(),
                    Some(version),
                    &open.document,
                    &open.line_index,
                    self.related_information,
                );
                self.documents.insert(uri, open);
                vec![published.into()]
            }
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams =
                    match serde_json::from_value(notification.params) {
                        Ok(params) => params,
                        Err(error) => return vec![dropped_notification("didClose", &error).into()],
                    };
                self.documents.remove(&params.text_document.uri);
                vec![
                    Notification::new(
                        "textDocument/publishDiagnostics".to_owned(),
                        PublishDiagnosticsParams::new(params.text_document.uri, Vec::new(), None),
                    )
                    .into(),
                ]
            }
            "textDocument/didChange" => {
                let params: DidChangeTextDocumentParams =
                    match serde_json::from_value(notification.params) {
                        Ok(params) => params,
                        Err(error) => {
                            return vec![dropped_notification("didChange", &error).into()];
                        }
                    };
                self.handle_did_change(params)
            }
            "workspace/didChangeConfiguration" => {
                let params: DidChangeConfigurationParams =
                    match serde_json::from_value(notification.params) {
                        Ok(params) => params,
                        Err(error) => {
                            return vec![
                                dropped_notification("didChangeConfiguration", &error).into(),
                            ];
                        }
                    };
                self.handle_did_change_configuration(&params.settings)
            }
            _ => Vec::new(),
        }
    }

    fn handle_did_change_configuration(&mut self, settings: &serde_json::Value) -> Vec<Message> {
        // Clients differ in whether they send the section object directly
        // or wrap it under its configuration key; accept both shapes.
        let settings = match settings.get("tomlsmith") {
            Some(section) if section.is_object() => section,
            _ => settings,
        };
        // Pull-model clients (VS Code without a push section, eglot without
        // local configuration) send `settings: null` or `{}` as a mere
        // change signal; re-resolving from that would silently wipe the
        // options the client passed through initializationOptions.
        if settings.get("tomlVersion").is_none() && settings.get("format").is_none() {
            return Vec::new();
        }
        // A payload carrying recognized options is a complete restatement,
        // so every option is re-resolved from scratch with the same
        // absent-value defaults as initialize.
        let previous_version = self.toml_version;
        let options = resolve_options(settings);
        self.toml_version = options.toml_version;
        self.format_options = options.format_options;
        self.generated_files = options.generated_files;
        self.configured_indent_width = options.configured_indent_width;
        if self.toml_version == previous_version {
            // Format options need no reparse; they apply to the next
            // formatting request through the stored session state.
            return Vec::new();
        }

        let mut messages: Vec<Message> = Vec::new();
        for (uri, open) in &mut self.documents {
            // The client's document version counter tracks edits, not
            // configuration; reparsing must not advance it.
            let text = open.document.text().to_owned();
            *open = OpenDocument::new(open.version, Document::parse_as(text, self.toml_version));
            messages.push(
                publish_diagnostics(
                    uri.clone(),
                    Some(open.version),
                    &open.document,
                    &open.line_index,
                    self.related_information,
                )
                .into(),
            );
        }
        if self.semantic_tokens_refresh {
            let id = self.next_request_id;
            self.next_request_id += 1;
            messages.push(
                Request::new(
                    lsp_server::RequestId::from(id),
                    "workspace/semanticTokens/refresh".to_owned(),
                    serde_json::Value::Null,
                )
                .into(),
            );
        }
        messages
    }

    fn handle_did_change(&mut self, params: DidChangeTextDocumentParams) -> Vec<Message> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let Some(open) = self.documents.get(&uri) else {
            return vec![
                log_message(
                    MessageType::WARNING,
                    format!(
                        "dropped didChange for a document that is not open: {}",
                        uri.as_str()
                    ),
                )
                .into(),
            ];
        };
        if version <= open.version {
            return vec![
                log_message(
                    MessageType::WARNING,
                    format!(
                        "dropped stale didChange version {version} for {} (server has {})",
                        uri.as_str(),
                        open.version
                    ),
                )
                .into(),
            ];
        }

        // Apply every content change to plain text first and reparse
        // once at the end; intermediate states never need syntax,
        // semantics, or diagnostics.
        let toml_version = open.document.version();
        let mut text = open.document.text().to_owned();
        let mut index = open.line_index.clone();
        for change in params.content_changes {
            match change.range {
                Some(range) => {
                    let start = position_to_byte(&text, &index, range.start) as usize;
                    let end = position_to_byte(&text, &index, range.end) as usize;
                    if start > end {
                        self.documents.remove(&uri);
                        // The user must hear about the closed document, not
                        // only the log: without a reopen the file silently
                        // loses diagnostics and highlighting.
                        return vec![
                            log_message(
                                MessageType::ERROR,
                                format!(
                                    "closed {} after a didChange range with end before start",
                                    uri.as_str()
                                ),
                            )
                            .into(),
                            show_message(
                                MessageType::ERROR,
                                format!(
                                    "TomlSmith closed {} after an invalid edit; reopen the file to restore diagnostics and highlighting",
                                    uri.as_str()
                                ),
                            )
                            .into(),
                        ];
                    }
                    text.replace_range(start..end, &change.text);
                }
                None => text = change.text,
            }
            index = LineIndex::new(&text);
        }

        let open = OpenDocument::new(version, Document::parse_as(text, toml_version));
        let published = publish_diagnostics(
            uri.clone(),
            Some(version),
            &open.document,
            &open.line_index,
            self.related_information,
        );
        self.documents.insert(uri, open);
        vec![published.into()]
    }
}

/// Byte-offset ↔ line lookup table for one document snapshot.
///
/// Built once per snapshot in O(n) so position conversions stay O(log n)
/// per lookup plus the width of the addressed line, instead of rescanning
/// the whole document prefix on every call.
#[derive(Clone)]
struct LineIndex {
    /// Byte offset of the first character of every line.
    line_starts: Vec<u32>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0_u32];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(offset, _)| saturating_u32(offset + 1)),
        );
        Self { line_starts }
    }

    fn line_of(&self, offset: u32) -> usize {
        self.line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1)
    }

    fn line_start(&self, line: usize) -> Option<u32> {
        self.line_starts.get(line).copied()
    }
}

/// Session options resolved from one complete settings object, either the
/// `initializationOptions` value or a `didChangeConfiguration` restatement.
struct ResolvedOptions {
    toml_version: TomlVersion,
    format_options: FormatOptions,
    generated_files: GeneratedFiles,
    /// `format.indentWidth` when explicitly configured; `None` lets each
    /// formatting request fall back to the client's `tabSize`.
    configured_indent_width: Option<u8>,
}

fn resolve_options(settings: &serde_json::Value) -> ResolvedOptions {
    let toml_version = resolve_toml_version(settings);
    // Every field is spelled out so a new option cannot silently inherit a
    // struct default (such as the core's TOML 1.1 `TomlVersion::default()`)
    // instead of the documented absent-option fallback.
    ResolvedOptions {
        toml_version,
        format_options: resolve_format_options(settings, toml_version),
        generated_files: resolve_generated_files(settings),
        configured_indent_width: resolve_indent_width(settings),
    }
}

fn resolve_toml_version(settings: &serde_json::Value) -> TomlVersion {
    // Both 1.0 and 1.1 are published specifications. The editor-facing default
    // remains 1.0 because many ecosystem consumers still accept only 1.0;
    // clients can opt in to 1.1 explicitly.
    match settings
        .get("tomlVersion")
        .and_then(serde_json::Value::as_str)
    {
        Some("1.1") => TomlVersion::V1_1,
        _ => TomlVersion::V1_0,
    }
}

fn resolve_format_options(
    settings: &serde_json::Value,
    toml_version: TomlVersion,
) -> FormatOptions {
    let mut options = FormatOptions {
        target_version: toml_version,
        ..FormatOptions::default()
    };
    let Some(format) = settings.get("format") else {
        return options;
    };

    if let Some(indent_width) = resolve_indent_width(settings) {
        options.indent_width = indent_width;
    }
    if let Some(line_width) = format
        .get("lineWidth")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
    {
        options.line_width = line_width;
    }
    options.line_ending = match format.get("lineEnding").and_then(serde_json::Value::as_str) {
        Some("lf") => LineEnding::Lf,
        Some("crlf") => LineEnding::CrLf,
        _ => LineEnding::Preserve,
    };
    options
}

fn resolve_generated_files(settings: &serde_json::Value) -> GeneratedFiles {
    match settings
        .pointer("/format/generatedFiles")
        .and_then(serde_json::Value::as_str)
    {
        Some("format") => GeneratedFiles::Format,
        _ => GeneratedFiles::Skip,
    }
}

fn resolve_indent_width(settings: &serde_json::Value) -> Option<u8> {
    settings
        .pointer("/format/indentWidth")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn initialize_hierarchical_symbols(params: &serde_json::Value) -> bool {
    params
        .pointer("/capabilities/textDocument/documentSymbol/hierarchicalDocumentSymbolSupport")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn initialize_related_information(params: &serde_json::Value) -> bool {
    params
        .pointer("/capabilities/textDocument/publishDiagnostics/relatedInformation")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn initialize_semantic_tokens_refresh(params: &serde_json::Value) -> bool {
    params
        .pointer("/capabilities/workspace/semanticTokens/refreshSupport")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// Lockfile basenames whose contents are always tool-generated TOML.
const GENERATED_LOCKFILE_NAMES: [&str; 4] = ["Cargo.lock", "uv.lock", "poetry.lock", "pdm.lock"];

fn is_generated_document(uri: &Uri, text: &str) -> bool {
    let basename = uri.path().as_str().rsplit('/').next().unwrap_or_default();
    GENERATED_LOCKFILE_NAMES.contains(&basename) || leading_comments_mark_generated(text)
}

/// Reports whether the leading comment block declares the file as generated.
/// At most the first 16 lines are scanned, and the scan stops at the first
/// line that is neither blank nor a `#` comment. Markers are matched as
/// case-insensitive substrings, so a negated phrase such as "not
/// automatically generated" also skips formatting; that costs one declined
/// request instead of churning a genuinely generated file.
fn leading_comments_mark_generated(text: &str) -> bool {
    // A leading BOM is not whitespace, so it would otherwise hide the `#`
    // of the first comment line.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for line in text.lines().take(16) {
        let line = line.trim_start();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with('#') {
            return false;
        }
        let lowercase = line.to_ascii_lowercase();
        if [
            "@generated",
            "autogenerated",
            "auto-generated",
            "automatically generated",
        ]
        .iter()
        .any(|marker| lowercase.contains(marker))
        {
            return true;
        }
    }
    false
}

/// Interior regions with more lines than this fall back to one replacement:
/// hashing every line of a pathological multi-megabyte diff would cost more
/// latency than the minimal edits save in bandwidth.
const LINE_DIFF_FALLBACK_LINES: usize = 20_000;

/// Bound on the anchor-subdivision recursion; regions nested deeper degrade
/// to one replacement instead of risking the stack.
const LINE_DIFF_MAX_DEPTH: usize = 16;

/// One replacement produced by the line diff: `original[old_start..old_end]`
/// becomes `formatted[new_start..new_end]`. Every offset is a line start or
/// the text end, so all of them fall on UTF-8 character boundaries.
#[derive(Clone, Copy, Debug)]
struct LineDiffEdit {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
}

/// Computes line-granularity edits that turn `original` into `formatted`,
/// sorted ascending and non-overlapping. Common prefix and suffix line runs
/// are trimmed first; the interior is then subdivided patience-style on
/// anchor lines that are unique in both interiors. A region becomes a single
/// replacement when it has no anchors, when the recursion passes
/// `LINE_DIFF_MAX_DEPTH`, or when either side has more than
/// `LINE_DIFF_FALLBACK_LINES` lines.
fn minimal_line_edits(original: &str, formatted: &str) -> Vec<LineDiffEdit> {
    // split_inclusive keeps every line terminator, so differing line
    // endings (LF vs CRLF) and a missing final newline stay visible to the
    // line comparison instead of silently matching.
    let old_lines: Vec<&str> = original.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = formatted.split_inclusive('\n').collect();
    let mut differ = LineDiffer {
        old_starts: line_start_offsets(&old_lines),
        new_starts: line_start_offsets(&new_lines),
        old_lines,
        new_lines,
        edits: Vec::new(),
    };
    let old_len = differ.old_lines.len();
    let new_len = differ.new_lines.len();
    differ.diff_region(0, old_len, 0, new_len, 0);
    merge_touching_edits(differ.edits)
}

/// Byte offset of every line start, plus the total length as a sentinel so
/// a line index range maps directly onto a byte range.
fn line_start_offsets(lines: &[&str]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(lines.len() + 1);
    starts.push(0_usize);
    let mut offset = 0_usize;
    for line in lines {
        offset += line.len();
        starts.push(offset);
    }
    starts
}

struct LineDiffer<'a> {
    old_lines: Vec<&'a str>,
    /// Line-start byte offsets for `old_lines`; see `line_start_offsets`.
    old_starts: Vec<usize>,
    new_lines: Vec<&'a str>,
    new_starts: Vec<usize>,
    /// Replacements accumulated in ascending order by the region walk.
    edits: Vec<LineDiffEdit>,
}

impl LineDiffer<'_> {
    fn diff_region(
        &mut self,
        mut old_start: usize,
        mut old_end: usize,
        mut new_start: usize,
        mut new_end: usize,
        depth: usize,
    ) {
        while old_start < old_end
            && new_start < new_end
            && self.old_lines[old_start] == self.new_lines[new_start]
        {
            old_start += 1;
            new_start += 1;
        }
        // Comparing against the already-trimmed starts stops the suffix run
        // from overlapping lines the prefix run consumed.
        while old_end > old_start
            && new_end > new_start
            && self.old_lines[old_end - 1] == self.new_lines[new_end - 1]
        {
            old_end -= 1;
            new_end -= 1;
        }
        if old_start == old_end && new_start == new_end {
            return;
        }
        if old_start == old_end
            || new_start == new_end
            || depth > LINE_DIFF_MAX_DEPTH
            || old_end - old_start > LINE_DIFF_FALLBACK_LINES
            || new_end - new_start > LINE_DIFF_FALLBACK_LINES
        {
            self.push_replacement(old_start, old_end, new_start, new_end);
            return;
        }
        let anchors = self.unique_anchors(old_start, old_end, new_start, new_end);
        if anchors.is_empty() {
            self.push_replacement(old_start, old_end, new_start, new_end);
            return;
        }
        let mut previous = (old_start, new_start);
        for (anchor_old, anchor_new) in anchors {
            self.diff_region(previous.0, anchor_old, previous.1, anchor_new, depth + 1);
            previous = (anchor_old + 1, anchor_new + 1);
        }
        self.diff_region(previous.0, old_end, previous.1, new_end, depth + 1);
    }

    /// Pairs every line that occurs exactly once in both interiors, then
    /// keeps the longest chain of pairs that ascends on both sides.
    fn unique_anchors(
        &self,
        old_start: usize,
        old_end: usize,
        new_start: usize,
        new_end: usize,
    ) -> Vec<(usize, usize)> {
        #[derive(Default)]
        struct Occurrence {
            old_count: usize,
            new_count: usize,
            new_index: usize,
        }
        let mut occurrences: HashMap<&str, Occurrence> = HashMap::new();
        for index in old_start..old_end {
            occurrences
                .entry(self.old_lines[index])
                .or_default()
                .old_count += 1;
        }
        for index in new_start..new_end {
            let entry = occurrences.entry(self.new_lines[index]).or_default();
            entry.new_count += 1;
            entry.new_index = index;
        }
        let pairs = (old_start..old_end)
            .filter_map(|index| {
                let occurrence = &occurrences[self.old_lines[index]];
                (occurrence.old_count == 1 && occurrence.new_count == 1)
                    .then_some((index, occurrence.new_index))
            })
            .collect::<Vec<_>>();
        longest_ascending_chain(&pairs)
    }

    fn push_replacement(
        &mut self,
        old_start: usize,
        old_end: usize,
        new_start: usize,
        new_end: usize,
    ) {
        self.edits.push(LineDiffEdit {
            old_start: self.old_starts[old_start],
            old_end: self.old_starts[old_end],
            new_start: self.new_starts[new_start],
            new_end: self.new_starts[new_end],
        });
    }
}

/// Longest subsequence of `pairs` whose new-side indices strictly ascend
/// (the old side already ascends by construction), via patience sorting.
/// Anchors outside this chain would cross each other and corrupt the diff.
fn longest_ascending_chain(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut predecessors = vec![usize::MAX; pairs.len()];
    for (index, &(_, new_index)) in pairs.iter().enumerate() {
        let position = tails.partition_point(|&tail| pairs[tail].1 < new_index);
        if position > 0 {
            predecessors[index] = tails[position - 1];
        }
        if position == tails.len() {
            tails.push(index);
        } else {
            tails[position] = index;
        }
    }
    let mut chain = Vec::new();
    let mut current = tails.last().copied().unwrap_or(usize::MAX);
    while current != usize::MAX {
        chain.push(pairs[current]);
        current = predecessors[current];
    }
    chain.reverse();
    chain
}

/// Collapses runs of edits whose replaced ranges touch. The region walk
/// advances both texts in lockstep, so old ranges that touch always carry
/// replacement ranges that touch as well.
fn merge_touching_edits(edits: Vec<LineDiffEdit>) -> Vec<LineDiffEdit> {
    let mut merged: Vec<LineDiffEdit> = Vec::with_capacity(edits.len());
    for edit in edits {
        match merged.last_mut() {
            Some(last) if last.old_end == edit.old_start => {
                last.old_end = edit.old_end;
                last.new_end = edit.new_end;
            }
            _ => merged.push(edit),
        }
    }
    merged
}

fn folding_ranges(document: &Document, index: &LineIndex) -> Vec<FoldingRange> {
    let tables = document
        .semantics()
        .declarations()
        .iter()
        .filter(|declaration| {
            matches!(
                declaration.kind(),
                DeclarationKind::Table | DeclarationKind::ArrayTable
            )
        })
        .map(|declaration| {
            (
                declaration.key().segments().collect::<Vec<_>>(),
                declaration,
            )
        })
        .collect::<Vec<_>>();
    let content_end = document.text().trim_end_matches(['\r', '\n']).len();
    let last_content_line =
        byte_to_position(document.text(), index, saturating_u32(content_end)).line;

    let mut ranges = tables
        .iter()
        .enumerate()
        .filter_map(|(table_index, (segments, declaration))| {
            let start_line =
                byte_to_position(document.text(), index, declaration.range().start()).line;
            // A table's fold covers its whole hierarchical extent: it ends
            // at the next table that is not a descendant, so folding
            // `[servers]` also hides `[servers.limits]` while a sibling
            // header still terminates the fold.
            let end_line = tables[table_index + 1..]
                .iter()
                .find(|(next_segments, _)| !is_proper_prefix(segments, next_segments))
                .map_or(last_content_line, |(_, next)| {
                    byte_to_position(document.text(), index, next.range().start())
                        .line
                        .saturating_sub(1)
                });
            (end_line > start_line).then_some(FoldingRange {
                start_line,
                start_character: None,
                end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            })
        })
        .collect::<Vec<_>>();
    collect_collection_folding_ranges(&document.root(), document.text(), index, &mut ranges);
    ranges.sort_unstable_by_key(|range| {
        (
            range.start_line,
            range.start_character.unwrap_or(u32::MAX),
            range.end_line,
            range.end_character.unwrap_or(u32::MAX),
        )
    });
    ranges
}

fn collect_collection_folding_ranges(
    node: &SyntaxNode,
    text: &str,
    index: &LineIndex,
    ranges: &mut Vec<FoldingRange>,
) {
    if matches!(node.kind(), SyntaxKind::Array | SyntaxKind::InlineTable) {
        let range = to_lsp_range(text, index, node.range());
        if range.end.line > range.start.line {
            ranges.push(FoldingRange {
                start_line: range.start.line,
                start_character: Some(range.start.character),
                end_line: range.end.line,
                end_character: Some(range.end.character),
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            });
        }
    }
    for child in node.children() {
        collect_collection_folding_ranges(&child, text, index, ranges);
    }
}

/// Reports whether `prefix` names a strictly shorter leading key path of
/// `path`, the table-nesting relation shared by symbol trees and folding.
fn is_proper_prefix(prefix: &[&str], path: &[&str]) -> bool {
    prefix.len() < path.len() && prefix.iter().zip(path).all(|(left, right)| left == right)
}

/// Renders key segments in TOML source spelling for symbol names. A plain
/// dotted join would violate the LSP requirement that a symbol name is
/// never empty (`[""]`) and would conflate a quoted key containing a dot
/// (`"a.b"`) with the dotted path `a.b`, so every segment that is not a
/// bare key is quoted.
fn symbol_name(segments: &[&str]) -> String {
    segments
        .iter()
        .map(|segment| {
            let bare = !segment.is_empty()
                && segment
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character));
            if bare {
                (*segment).to_owned()
            } else {
                let escaped = segment.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn covering_range(left: TextRange, right: TextRange) -> TextRange {
    TextRange::new(left.start().min(right.start()), left.end().max(right.end()))
}

fn hover(document: &Document, index: &LineIndex, position: Position) -> Option<Hover> {
    let byte_offset = position_to_byte(document.text(), index, position);
    let declaration = document
        .semantics()
        .declarations()
        .iter()
        .filter(|declaration| {
            declaration.range().start() <= byte_offset && byte_offset < declaration.range().end()
        })
        .min_by_key(|declaration| declaration.range().end() - declaration.range().start())?;
    let kind = declaration.value().map_or_else(
        || match declaration.kind() {
            DeclarationKind::KeyValue => "key",
            DeclarationKind::Table => "table",
            DeclarationKind::ArrayTable => "array table",
        },
        semantic_value_kind,
    );
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::PlainText,
            value: format!("{}\nTOML {kind}", declaration.key().dotted()),
        }),
        range: Some(to_lsp_range(document.text(), index, declaration.range())),
    })
}

fn document_symbols(
    document: &Document,
    index: &LineIndex,
    uri: &Uri,
    hierarchical: bool,
) -> DocumentSymbolResponse {
    if hierarchical {
        hierarchical_document_symbols(document, index).into()
    } else {
        // Clients that do not declare hierarchicalDocumentSymbolSupport
        // must receive SymbolInformation values instead.
        flat_document_symbols(document, index, uri).into()
    }
}

/// A table or array-of-tables header whose body is still being collected
/// while the symbol pass walks the declarations in document order.
struct OpenContainer<'a> {
    segments: Vec<&'a str>,
    symbol: DocumentSymbol,
    /// Byte extent of the header unioned with every attached descendant;
    /// it becomes the symbol's full `range` when the container closes.
    extent: TextRange,
    children: Vec<DocumentSymbol>,
}

fn hierarchical_document_symbols(document: &Document, index: &LineIndex) -> Vec<DocumentSymbol> {
    let text = document.text();
    let mut roots = Vec::new();
    let mut stack: Vec<OpenContainer> = Vec::new();
    for declaration in document.semantics().declarations() {
        let segments = declaration.key().segments().collect::<Vec<_>>();
        match declaration.kind() {
            DeclarationKind::Table | DeclarationKind::ArrayTable => {
                // A header closes every open container that cannot contain
                // it. An equal path is not a proper prefix, so a repeated
                // `[[name]]` closes the previous instance and becomes its
                // sibling instead of nesting inside it.
                while stack
                    .last()
                    .is_some_and(|top| !is_proper_prefix(&top.segments, &segments))
                {
                    if let Some(closed) = stack.pop() {
                        close_container(closed, &mut stack, &mut roots, text, index);
                    }
                }
                let name = stack.last().map_or_else(
                    || symbol_name(&segments),
                    |parent| symbol_name(&segments[parent.segments.len()..]),
                );
                stack.push(OpenContainer {
                    symbol: declaration_symbol(declaration, name, text, index),
                    segments,
                    extent: declaration.range(),
                    children: Vec::new(),
                });
            }
            DeclarationKind::KeyValue => {
                // Conflicting declarations are all retained, so a key does
                // not always belong to the top of the stack; it attaches to
                // the innermost container whose path is a proper prefix.
                let parent = stack
                    .iter()
                    .rposition(|container| is_proper_prefix(&container.segments, &segments));
                let name = parent.map_or_else(
                    || symbol_name(&segments),
                    |position| symbol_name(&segments[stack[position].segments.len()..]),
                );
                let symbol = declaration_symbol(declaration, name, text, index);
                match parent {
                    Some(position) => {
                        let container = &mut stack[position];
                        container.extent = covering_range(container.extent, declaration.range());
                        container.children.push(symbol);
                    }
                    None => roots.push(symbol),
                }
            }
        }
    }
    while let Some(container) = stack.pop() {
        close_container(container, &mut stack, &mut roots, text, index);
    }
    roots
}

fn close_container(
    mut container: OpenContainer<'_>,
    stack: &mut [OpenContainer<'_>],
    roots: &mut Vec<DocumentSymbol>,
    text: &str,
    index: &LineIndex,
) {
    // The full range is only known once every descendant has been
    // attached, so it is finalized when the container closes.
    container.symbol.range = to_lsp_range(text, index, container.extent);
    if !container.children.is_empty() {
        container.symbol.children = Some(container.children);
    }
    match stack.last_mut() {
        Some(parent) => {
            parent.extent = covering_range(parent.extent, container.extent);
            parent.children.push(container.symbol);
        }
        None => roots.push(container.symbol),
    }
}

#[allow(deprecated)]
fn declaration_symbol(
    declaration: &Declaration,
    name: String,
    text: &str,
    index: &LineIndex,
) -> DocumentSymbol {
    let header = to_lsp_range(text, index, declaration.range());
    DocumentSymbol {
        name,
        detail: declaration
            .value()
            .map(semantic_value_kind)
            .map(str::to_owned),
        kind: symbol_kind(declaration.kind()),
        tags: None,
        deprecated: None,
        range: header,
        selection_range: header,
        children: None,
    }
}

#[allow(deprecated)]
fn flat_document_symbols(
    document: &Document,
    index: &LineIndex,
    uri: &Uri,
) -> Vec<SymbolInformation> {
    // The same prefix stack as the hierarchical tree, tracking paths only:
    // flat clients keep the full dotted names but still receive the parent
    // container through `containerName`.
    let mut containers: Vec<Vec<&str>> = Vec::new();
    document
        .semantics()
        .declarations()
        .iter()
        .map(|declaration| {
            let segments = declaration.key().segments().collect::<Vec<_>>();
            let container_name = match declaration.kind() {
                DeclarationKind::Table | DeclarationKind::ArrayTable => {
                    while containers
                        .last()
                        .is_some_and(|top| !is_proper_prefix(top, &segments))
                    {
                        containers.pop();
                    }
                    let parent = containers.last().map(|parent| symbol_name(parent));
                    containers.push(segments.clone());
                    parent
                }
                DeclarationKind::KeyValue => containers
                    .iter()
                    .rev()
                    .find(|container| is_proper_prefix(container, &segments))
                    .map(|container| symbol_name(container)),
            };
            SymbolInformation {
                name: symbol_name(&segments),
                kind: symbol_kind(declaration.kind()),
                tags: None,
                deprecated: None,
                location: Location {
                    uri: uri.clone(),
                    range: to_lsp_range(document.text(), index, declaration.range()),
                },
                container_name,
            }
        })
        .collect()
}

const fn symbol_kind(kind: DeclarationKind) -> SymbolKind {
    match kind {
        DeclarationKind::KeyValue => SymbolKind::PROPERTY,
        DeclarationKind::Table => SymbolKind::NAMESPACE,
        DeclarationKind::ArrayTable => SymbolKind::ARRAY,
    }
}

const fn semantic_value_kind(value: &SemanticValue) -> &'static str {
    match value {
        SemanticValue::String(_) => "string",
        SemanticValue::Integer(_) => "integer",
        SemanticValue::Float(_) => "float",
        SemanticValue::Boolean(_) => "boolean",
        SemanticValue::DateTime(_) => "date-time",
        SemanticValue::Array(_) => "array",
        SemanticValue::InlineTable(_) => "inline table",
        SemanticValue::Table(_) => "table",
        SemanticValue::Invalid(_) => "invalid",
    }
}

fn semantic_tokens(document: &Document, index: &LineIndex) -> SemanticTokens {
    let mut absolute = document
        .highlights()
        .iter()
        .flat_map(|highlight| {
            token_segments(
                document.text(),
                index,
                highlight.range(),
                semantic_token_type(highlight.kind()),
            )
        })
        .collect::<Vec<_>>();
    absolute.sort_unstable_by_key(|token| (token.line, token.start));

    let mut previous_line = 0_u32;
    let mut previous_start = 0_u32;
    let data = absolute
        .into_iter()
        .map(|token| {
            let delta_line = token.line - previous_line;
            let delta_start = if delta_line == 0 {
                token.start - previous_start
            } else {
                token.start
            };
            previous_line = token.line;
            previous_start = token.start;
            SemanticToken {
                delta_line,
                delta_start,
                length: token.length,
                token_type: token.token_type,
                token_modifiers_bitset: 0,
            }
        })
        .collect();
    SemanticTokens {
        result_id: None,
        data,
    }
}

#[derive(Clone, Copy)]
struct AbsoluteToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
}

fn token_segments(
    text: &str,
    index: &LineIndex,
    range: TextRange,
    token_type: u32,
) -> Vec<AbsoluteToken> {
    let mut segments = Vec::new();
    let mut start = usize::try_from(range.start()).unwrap_or(text.len());
    let end = usize::try_from(range.end())
        .unwrap_or(text.len())
        .min(text.len());

    while start < end {
        let newline = text[start..end].find('\n');
        let mut segment_end = newline.map_or(end, |offset| start + offset);
        if segment_end > start && text.as_bytes()[segment_end - 1] == b'\r' {
            segment_end -= 1;
        }
        if segment_end > start {
            let start_position = byte_to_position(text, index, saturating_u32(start));
            let end_position = byte_to_position(text, index, saturating_u32(segment_end));
            if end_position.character > start_position.character {
                segments.push(AbsoluteToken {
                    line: start_position.line,
                    start: start_position.character,
                    length: end_position.character - start_position.character,
                    token_type,
                });
            }
        }
        let Some(newline) = newline else {
            break;
        };
        start += newline + 1;
    }
    segments
}

const fn semantic_token_type(kind: HighlightKind) -> u32 {
    match kind {
        HighlightKind::Key => 0,
        HighlightKind::ArrayKey | HighlightKind::ArrayTable => 9,
        HighlightKind::InlineTableKey | HighlightKind::Table => 10,
        HighlightKind::String => 2,
        HighlightKind::Number => 3,
        HighlightKind::Boolean => 4,
        HighlightKind::Comment => 5,
        HighlightKind::Punctuation => 6,
        HighlightKind::DateTime => 7,
        HighlightKind::Invalid => 8,
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn invalid_params(id: lsp_server::RequestId, error: &serde_json::Error) -> Response {
    Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string())
}

fn dropped_notification(method: &str, error: &serde_json::Error) -> Notification {
    log_message(
        MessageType::WARNING,
        format!("dropped a malformed {method} notification: {error}"),
    )
}

fn log_message(typ: MessageType, message: String) -> Notification {
    Notification::new(
        "window/logMessage".to_owned(),
        LogMessageParams { typ, message },
    )
}

fn show_message(typ: MessageType, message: String) -> Notification {
    Notification::new(
        "window/showMessage".to_owned(),
        ShowMessageParams { typ, message },
    )
}

fn panic_text(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

fn publish_diagnostics(
    uri: Uri,
    version: Option<i32>,
    document: &Document,
    index: &LineIndex,
    related_information: bool,
) -> Notification {
    let diagnostics = document
        .diagnostics()
        .iter()
        .map(|diagnostic| lsp_types::Diagnostic {
            range: to_lsp_range(document.text(), index, diagnostic.range()),
            severity: Some(match diagnostic.severity() {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
            }),
            code: Some(NumberOrString::String(
                diagnostic.code().as_str().to_owned(),
            )),
            source: Some("tomlsmith".to_owned()),
            message: diagnostic.message().to_owned(),
            // The core records the earliest conflicting declaration on the
            // diagnostic itself, so linking "first declared here" needs no
            // rescan of the declaration list.
            related_information: (related_information
                && diagnostic.code() == DiagnosticCode::DUPLICATE_KEY)
                .then(|| diagnostic.related_range())
                .flatten()
                .map(|related| {
                    vec![DiagnosticRelatedInformation {
                        location: Location {
                            uri: uri.clone(),
                            range: to_lsp_range(document.text(), index, related),
                        },
                        message: "first declared here".to_owned(),
                    }]
                }),
            ..lsp_types::Diagnostic::default()
        })
        .collect();
    Notification::new(
        "textDocument/publishDiagnostics".to_owned(),
        PublishDiagnosticsParams::new(uri, diagnostics, version),
    )
}

fn to_lsp_range(text: &str, index: &LineIndex, range: TextRange) -> Range {
    Range::new(
        byte_to_position(text, index, range.start()),
        byte_to_position(text, index, range.end()),
    )
}

fn byte_to_position(text: &str, index: &LineIndex, byte_offset: u32) -> Position {
    let offset = usize::try_from(byte_offset)
        .unwrap_or(usize::MAX)
        .min(text.len());
    let line = index.line_of(saturating_u32(offset));
    let line_start = index
        .line_start(line)
        .map_or(0, |start| start as usize)
        .min(text.len());
    let character = text[line_start..offset].encode_utf16().count();
    Position::new(
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(character).unwrap_or(u32::MAX),
    )
}

/// Converts an LSP position into a byte offset, clamping out-of-range
/// positions the way the specification requires: a character beyond the end
/// of its line maps to the line end, and a line beyond the end of the
/// document maps to the end of the text.
fn position_to_byte(text: &str, index: &LineIndex, position: Position) -> u32 {
    let requested_line = usize::try_from(position.line).unwrap_or(usize::MAX);
    let Some(line_start) = index.line_start(requested_line) else {
        return saturating_u32(text.len());
    };
    let line_start = line_start as usize;
    let mut line_end = index
        .line_start(requested_line + 1)
        .map_or(text.len(), |next| (next as usize).saturating_sub(1));
    if line_end > line_start && text.as_bytes()[line_end - 1] == b'\r' {
        line_end -= 1;
    }
    let line = &text[line_start..line_end];
    let requested_character = usize::try_from(position.character).unwrap_or(usize::MAX);
    let mut utf16_offset = 0_usize;

    for (byte_offset, character) in line.char_indices() {
        if utf16_offset >= requested_character {
            return saturating_u32(line_start + byte_offset);
        }
        utf16_offset += character.len_utf16();
    }
    saturating_u32(line_end)
}

fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        semantic_tokens_provider: Some(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend: SemanticTokensLegend {
                    token_types: vec![
                        SemanticTokenType::PROPERTY,
                        SemanticTokenType::NAMESPACE,
                        SemanticTokenType::STRING,
                        SemanticTokenType::NUMBER,
                        SemanticTokenType::KEYWORD,
                        SemanticTokenType::COMMENT,
                        SemanticTokenType::OPERATOR,
                        SemanticTokenType::new("tomlDateTime"),
                        SemanticTokenType::new("tomlInvalid"),
                        SemanticTokenType::new("tomlArrayKey"),
                        SemanticTokenType::new("tomlTableKey"),
                    ],
                    token_modifiers: Vec::new(),
                },
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            }
            .into(),
        ),
        ..ServerCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// Applies byte-range line-diff edits, asserting along the way that
    /// they are well-formed, sorted ascending, and non-overlapping.
    fn applied(original: &str, formatted: &str, edits: &[LineDiffEdit]) -> String {
        let mut result = String::with_capacity(formatted.len());
        let mut cursor = 0_usize;
        for edit in edits {
            assert!(
                cursor <= edit.old_start && edit.old_start <= edit.old_end,
                "edits must be sorted, non-overlapping, and well-formed: {edits:?}"
            );
            assert!(
                edit.old_end <= original.len(),
                "edit past the end: {edit:?}"
            );
            result.push_str(&original[cursor..edit.old_start]);
            result.push_str(&formatted[edit.new_start..edit.new_end]);
            cursor = edit.old_end;
        }
        result.push_str(&original[cursor..]);
        result
    }

    fn checked_edits(original: &str, formatted: &str) -> Vec<LineDiffEdit> {
        let edits = minimal_line_edits(original, formatted);
        assert_eq!(
            applied(original, formatted, &edits),
            formatted,
            "edits must reproduce the formatted text exactly for {original:?}"
        );
        edits
    }

    #[test]
    fn a_first_line_change_touches_only_the_first_line() {
        let edits = checked_edits("a=1\nb = 2\nc = 3\n", "a = 1\nb = 2\nc = 3\n");
        assert_eq!(edits.len(), 1, "{edits:?}");
        assert_eq!((edits[0].old_start, edits[0].old_end), (0, 4));
    }

    #[test]
    fn a_last_line_change_touches_only_the_last_line() {
        let edits = checked_edits("a = 1\nb = 2\nc=3\n", "a = 1\nb = 2\nc = 3\n");
        assert_eq!(edits.len(), 1, "{edits:?}");
        assert_eq!((edits[0].old_start, edits[0].old_end), (12, 16));
    }

    #[test]
    fn a_middle_line_change_leaves_the_first_and_last_lines_untouched() {
        let edits = checked_edits("a = 1\nb=2\nc = 3\n", "a = 1\nb = 2\nc = 3\n");
        assert_eq!(edits.len(), 1, "{edits:?}");
        // Exactly the byte extent of the middle line "b=2\n".
        assert_eq!((edits[0].old_start, edits[0].old_end), (6, 10));
    }

    #[test]
    fn interleaved_indent_changes_stay_far_below_the_document_size() {
        // The measured Cargo.lock defect shape: many stanzas where only the
        // dependency indentation changes, previously answered with one edit
        // replacing the whole document.
        let mut original = String::new();
        let mut formatted = String::new();
        for stanza in 0..120 {
            let head = format!(
                "[[package]]\n\
                 name = \"package-{stanza}\"\n\
                 version = \"1.0.{stanza}\"\n\
                 source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
                 checksum = \"checksum-{stanza}\"\n\
                 dependencies = [\n"
            );
            original.push_str(&head);
            formatted.push_str(&head);
            write!(original, " \"dep-{stanza}\",\n]\n\n").unwrap();
            write!(formatted, "  \"dep-{stanza}\",\n]\n\n").unwrap();
        }
        let edits = checked_edits(&original, &formatted);
        assert!(
            edits.len() >= 100,
            "expected roughly one edit per changed stanza, got {}",
            edits.len()
        );
        let replaced: usize = edits.iter().map(|edit| edit.new_end - edit.new_start).sum();
        assert!(
            replaced * 8 < original.len(),
            "{replaced} replacement bytes for a {}-byte document",
            original.len()
        );
    }

    #[test]
    fn crlf_documents_diff_on_whole_crlf_lines() {
        let edits = checked_edits("a=1\r\nb = 2\r\nc=3\r\n", "a = 1\r\nb = 2\r\nc = 3\r\n");
        assert_eq!(edits.len(), 2, "{edits:?}");
    }

    #[test]
    fn documents_without_a_trailing_newline_round_trip() {
        checked_edits("a = 1\nb=2", "a = 1\nb = 2");
        checked_edits("a=1", "a = 1\n");
        checked_edits("a = 1\nb = 2", "a = 1\nb = 2\nc = 3");
    }

    #[test]
    fn empty_and_nonempty_documents_degenerate_to_one_edit() {
        let inserted = checked_edits("", "a = 1\n");
        assert_eq!(inserted.len(), 1, "{inserted:?}");
        assert_eq!((inserted[0].old_start, inserted[0].old_end), (0, 0));
        let deleted = checked_edits("a = 1\n", "");
        assert_eq!(deleted.len(), 1, "{deleted:?}");
        assert_eq!(deleted[0].new_start, deleted[0].new_end);
        assert!(checked_edits("", "").is_empty());
    }

    #[test]
    fn oversized_interiors_fall_back_to_a_single_replacement() {
        // Alternating unchanged unique lines would offer thousands of
        // anchors, but the interior exceeds the line cap, so the diff must
        // stop at one replacement instead of subdividing.
        let mut original = String::new();
        let mut formatted = String::new();
        for pair in 0..=(LINE_DIFF_FALLBACK_LINES / 2) {
            writeln!(original, "changed{pair}=1\nstable{pair} = 2").unwrap();
            writeln!(formatted, "changed{pair} = 1\nstable{pair} = 2").unwrap();
        }
        let edits = checked_edits(&original, &formatted);
        assert_eq!(
            edits.len(),
            1,
            "an oversized interior must not be subdivided"
        );
    }

    /// Deterministic linear congruential generator; the property test must
    /// not pull in an external randomness crate.
    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, bound: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from(self.0 >> 33).unwrap_or(usize::MAX) % bound.max(1)
        }
    }

    fn generate_document(rng: &mut Lcg) -> String {
        let mut text = String::new();
        for table in 0..=rng.below(4) {
            writeln!(text, "[table{table}]").unwrap();
            for key in 0..=rng.below(6) {
                let value = match rng.below(4) {
                    0 => format!("{}", key * 7),
                    1 => "\"text value\"".to_owned(),
                    2 => "true".to_owned(),
                    _ => "[1, 2, 3]".to_owned(),
                };
                writeln!(text, "key{table}_{key} = {value}").unwrap();
            }
        }
        text
    }

    fn perturb_whitespace(rng: &mut Lcg, text: &str) -> String {
        let mut result = String::new();
        for line in text.split_inclusive('\n') {
            match rng.below(4) {
                0 => {
                    result.push_str("   ");
                    result.push_str(line);
                }
                1 => result.push_str(&line.replacen(" = ", "=", 1)),
                2 => result.push_str(&line.replacen(" = ", "   =  ", 1)),
                _ => result.push_str(line),
            }
        }
        result
    }

    #[test]
    fn randomized_perturbations_round_trip_through_minimal_edits() {
        let mut rng = Lcg(0x5EED_CAFE_F00D_D1FF);
        for round in 0..48_usize {
            let clean = generate_document(&mut rng);
            let mut perturbed = perturb_whitespace(&mut rng, &clean);
            if round % 3 == 0 {
                perturbed = perturbed.replace('\n', "\r\n");
            }
            if round % 5 == 0 && perturbed.ends_with('\n') {
                perturbed.pop();
                if perturbed.ends_with('\r') {
                    perturbed.pop();
                }
            }
            let document = Document::parse_as(perturbed.clone(), TomlVersion::V1_0);
            let options = FormatOptions {
                target_version: TomlVersion::V1_0,
                ..FormatOptions::default()
            };
            match document.format_with(&options) {
                FormatOutcome::Changed { text, .. } => {
                    checked_edits(&perturbed, &text);
                }
                FormatOutcome::Unchanged => {
                    assert!(minimal_line_edits(&perturbed, &perturbed).is_empty());
                }
                FormatOutcome::Refused { .. } => {
                    panic!("round {round}: the formatter refused a generated document")
                }
            }
        }
    }
}
