#![forbid(unsafe_code)]

use std::{collections::HashMap, error::Error};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, FoldingRange, FoldingRangeKind, FoldingRangeParams,
    FoldingRangeProviderCapability, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeResult, MarkupContent, MarkupKind, NumberOrString, OneOf, Position,
    PositionEncodingKind, PublishDiagnosticsParams, Range, SemanticToken, SemanticTokenType,
    SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensParams, ServerCapabilities, ServerInfo, SymbolKind, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, WorkDoneProgressOptions,
};
use tomlsmith::{
    DeclarationKind, Document, FormatOptions, FormatOutcome, HighlightKind, LineEnding,
    SemanticValue, Severity, SyntaxKind, SyntaxNode, TextChange, TextRange, TomlVersion,
};

pub type ServerResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

/// Runs one Language Server Protocol session over the supplied transport.
///
/// # Errors
///
/// Returns an error when the initialize/shutdown handshake is invalid or when
/// protocol values cannot be serialized.
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

    let toml_version = initialize_toml_version(&initialize_params);
    let mut session = Session {
        toml_version,
        format_options: initialize_format_options(&initialize_params, toml_version),
        initialized_indent_width: initialize_indent_width(&initialize_params),
        ..Session::default()
    };
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                if connection
                    .sender
                    .send(session.handle_request(request).into())
                    .is_err()
                {
                    return Ok(());
                }
            }
            Message::Notification(notification) if notification.method == "exit" => return Ok(()),
            Message::Notification(notification) => {
                let send_failed = session
                    .handle_notification(notification)
                    .is_some_and(|response| connection.sender.send(response.into()).is_err());
                if send_failed {
                    return Ok(());
                }
            }
            Message::Response(_) => {}
        }
    }

    Ok(())
}

#[derive(Default)]
struct Session {
    documents: HashMap<Uri, OpenDocument>,
    toml_version: TomlVersion,
    format_options: FormatOptions,
    initialized_indent_width: Option<u8>,
}

struct OpenDocument {
    version: i32,
    document: Document,
}

impl Session {
    fn handle_request(&self, request: Request) -> Response {
        match request.method.as_str() {
            "textDocument/formatting" => {
                let id = request.id;
                let params: DocumentFormattingParams = match serde_json::from_value(request.params)
                {
                    Ok(params) => params,
                    Err(error) => return invalid_params(id, &error),
                };
                let Some(open) = self.documents.get(&params.text_document.uri) else {
                    return Response::new_ok(id, Option::<Vec<lsp_types::TextEdit>>::None);
                };
                let mut format_options = self.format_options.clone();
                format_options.target_version = open.document.version();
                let requested_indent = self
                    .initialized_indent_width
                    .is_none()
                    .then(|| u8::try_from(params.options.tab_size).ok())
                    .flatten()
                    .filter(|tab_size| *tab_size > 0);
                if let Some(tab_size) = requested_indent {
                    format_options.indent_width = tab_size;
                }
                let edits = match open.document.format_with(&format_options) {
                    FormatOutcome::Unchanged | FormatOutcome::Refused { .. } => Vec::new(),
                    FormatOutcome::Changed { edits, .. } => edits
                        .iter()
                        .map(|edit| lsp_types::TextEdit {
                            range: to_lsp_range(open.document.text(), edit.range()),
                            new_text: edit.replacement().to_owned(),
                        })
                        .collect(),
                };
                Response::new_ok(id, edits)
            }
            "textDocument/semanticTokens/full" => {
                let id = request.id;
                let params: SemanticTokensParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return invalid_params(id, &error),
                };
                let Some(open) = self.documents.get(&params.text_document.uri) else {
                    return Response::new_ok(id, Option::<SemanticTokens>::None);
                };
                Response::new_ok(id, semantic_tokens(&open.document))
            }
            "textDocument/documentSymbol" => {
                let id = request.id;
                let params: DocumentSymbolParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return invalid_params(id, &error),
                };
                let Some(open) = self.documents.get(&params.text_document.uri) else {
                    return Response::new_ok(id, Option::<DocumentSymbolResponse>::None);
                };
                Response::new_ok(id, document_symbols(&open.document))
            }
            "textDocument/hover" => {
                let id = request.id;
                let params: HoverParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return invalid_params(id, &error),
                };
                let text_document_position = params.text_document_position_params;
                let Some(open) = self
                    .documents
                    .get(&text_document_position.text_document.uri)
                else {
                    return Response::new_ok(id, Option::<Hover>::None);
                };
                Response::new_ok(id, hover(&open.document, text_document_position.position))
            }
            "textDocument/foldingRange" => {
                let id = request.id;
                let params: FoldingRangeParams = match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => return invalid_params(id, &error),
                };
                let Some(open) = self.documents.get(&params.text_document.uri) else {
                    return Response::new_ok(id, Option::<Vec<FoldingRange>>::None);
                };
                Response::new_ok(id, folding_ranges(&open.document))
            }
            _ => Response::new_err(
                request.id,
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request `{}`", request.method),
            ),
        }
    }

    fn handle_notification(&mut self, notification: Notification) -> Option<Notification> {
        match notification.method.as_str() {
            "textDocument/didOpen" => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params).ok()?;
                let uri = params.text_document.uri;
                let version = params.text_document.version;
                let document = Document::parse_as(params.text_document.text, self.toml_version);
                let published = publish_diagnostics(uri.clone(), Some(version), &document);
                self.documents
                    .insert(uri, OpenDocument { version, document });
                Some(published)
            }
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params).ok()?;
                self.documents.remove(&params.text_document.uri);
                Some(Notification::new(
                    "textDocument/publishDiagnostics".to_owned(),
                    PublishDiagnosticsParams::new(params.text_document.uri, Vec::new(), None),
                ))
            }
            "textDocument/didChange" => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params).ok()?;
                let uri = params.text_document.uri;
                let version = params.text_document.version;
                let open = self.documents.get_mut(&uri)?;
                if version <= open.version {
                    return None;
                }

                let mut document = open.document.clone();
                for change in params.content_changes {
                    let text_change = if let Some(range) = change.range {
                        let start = position_to_byte(document.text(), range.start)?;
                        let end = position_to_byte(document.text(), range.end)?;
                        if start > end {
                            return None;
                        }
                        TextChange::edit(TextRange::new(start, end), change.text)
                    } else {
                        TextChange::replace(change.text)
                    };
                    document = document.with_changes([text_change]).ok()?;
                }

                open.version = version;
                open.document = document;
                Some(publish_diagnostics(uri, Some(version), &open.document))
            }
            _ => None,
        }
    }
}

fn initialize_toml_version(params: &serde_json::Value) -> TomlVersion {
    match params
        .pointer("/initializationOptions/tomlVersion")
        .and_then(serde_json::Value::as_str)
    {
        Some("1.0") => TomlVersion::V1_0,
        _ => TomlVersion::V1_1,
    }
}

fn initialize_format_options(
    params: &serde_json::Value,
    toml_version: TomlVersion,
) -> FormatOptions {
    let mut options = FormatOptions {
        target_version: toml_version,
        ..FormatOptions::default()
    };
    let Some(format) = params.pointer("/initializationOptions/format") else {
        return options;
    };

    if let Some(indent_width) = initialize_indent_width(params) {
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

fn initialize_indent_width(params: &serde_json::Value) -> Option<u8> {
    params
        .pointer("/initializationOptions/format/indentWidth")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn folding_ranges(document: &Document) -> Vec<FoldingRange> {
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
        .collect::<Vec<_>>();
    let content_end = document.text().trim_end_matches(['\r', '\n']).len();
    let last_content_line = byte_to_position(document.text(), saturating_u32(content_end)).line;

    let mut ranges = tables
        .iter()
        .enumerate()
        .filter_map(|(index, declaration)| {
            let start_line = byte_to_position(document.text(), declaration.range().start()).line;
            let end_line = tables.get(index + 1).map_or(last_content_line, |next| {
                byte_to_position(document.text(), next.range().start())
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
    collect_collection_folding_ranges(&document.root(), document.text(), &mut ranges);
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
    ranges: &mut Vec<FoldingRange>,
) {
    if matches!(node.kind(), SyntaxKind::Array | SyntaxKind::InlineTable) {
        let range = to_lsp_range(text, node.range());
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
        collect_collection_folding_ranges(&child, text, ranges);
    }
}

fn hover(document: &Document, position: Position) -> Option<Hover> {
    let byte_offset = position_to_byte(document.text(), position)?;
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
        range: Some(to_lsp_range(document.text(), declaration.range())),
    })
}

#[allow(deprecated)]
fn document_symbols(document: &Document) -> DocumentSymbolResponse {
    document
        .semantics()
        .declarations()
        .iter()
        .map(|declaration| {
            let range = to_lsp_range(document.text(), declaration.range());
            DocumentSymbol {
                name: declaration.key().dotted(),
                detail: declaration
                    .value()
                    .map(semantic_value_kind)
                    .map(str::to_owned),
                kind: match declaration.kind() {
                    DeclarationKind::KeyValue => SymbolKind::PROPERTY,
                    DeclarationKind::Table => SymbolKind::NAMESPACE,
                    DeclarationKind::ArrayTable => SymbolKind::ARRAY,
                },
                tags: None,
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            }
        })
        .collect::<Vec<_>>()
        .into()
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

fn semantic_tokens(document: &Document) -> SemanticTokens {
    let mut absolute = document
        .highlights()
        .iter()
        .flat_map(|highlight| {
            token_segments(
                document.text(),
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

fn token_segments(text: &str, range: TextRange, token_type: u32) -> Vec<AbsoluteToken> {
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
            let start_position = byte_to_position(text, saturating_u32(start));
            let end_position = byte_to_position(text, saturating_u32(segment_end));
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
        HighlightKind::Table => 1,
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

fn publish_diagnostics(uri: Uri, version: Option<i32>, document: &Document) -> Notification {
    let diagnostics = document
        .diagnostics()
        .iter()
        .map(|diagnostic| lsp_types::Diagnostic {
            range: to_lsp_range(document.text(), diagnostic.range()),
            severity: Some(match diagnostic.severity() {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
            }),
            code: Some(NumberOrString::String(
                diagnostic.code().as_str().to_owned(),
            )),
            source: Some("tomlsmith".to_owned()),
            message: diagnostic.message().to_owned(),
            ..lsp_types::Diagnostic::default()
        })
        .collect();
    Notification::new(
        "textDocument/publishDiagnostics".to_owned(),
        PublishDiagnosticsParams::new(uri, diagnostics, version),
    )
}

fn to_lsp_range(text: &str, range: TextRange) -> Range {
    Range::new(
        byte_to_position(text, range.start()),
        byte_to_position(text, range.end()),
    )
}

fn byte_to_position(text: &str, byte_offset: u32) -> Position {
    let offset = usize::try_from(byte_offset)
        .unwrap_or(usize::MAX)
        .min(text.len());
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let character = text[line_start..offset].encode_utf16().count();
    Position::new(
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(character).unwrap_or(u32::MAX),
    )
}

fn position_to_byte(text: &str, position: Position) -> Option<u32> {
    let requested_line = usize::try_from(position.line).ok()?;
    let line_start = if requested_line == 0 {
        0
    } else {
        text.match_indices('\n')
            .nth(requested_line - 1)
            .map(|(offset, _)| offset + 1)?
    };
    let line_with_ending = &text[line_start..];
    let mut line_end = line_with_ending
        .find('\n')
        .map_or(text.len(), |offset| line_start + offset);
    if line_end > line_start && text.as_bytes()[line_end - 1] == b'\r' {
        line_end -= 1;
    }
    let line = &text[line_start..line_end];
    let requested_character = usize::try_from(position.character).ok()?;
    let mut utf16_offset = 0_usize;

    for (byte_offset, character) in line.char_indices() {
        if utf16_offset == requested_character {
            return u32::try_from(line_start + byte_offset).ok();
        }
        utf16_offset += character.len_utf16();
        if utf16_offset > requested_character {
            return None;
        }
    }
    if utf16_offset == requested_character {
        u32::try_from(line_end).ok()
    } else {
        None
    }
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
