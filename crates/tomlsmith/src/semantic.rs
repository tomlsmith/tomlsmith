use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, OnceLock},
};

use rowan::{Language, NodeOrToken};

use crate::{
    Diagnostic, DiagnosticCode, TextRange,
    literal::{self, LiteralValue},
    syntax::{SyntaxKind, TomlLanguage},
};

pub(crate) const MAX_KEY_DEPTH: usize = 256;

/// A fast, deterministic hasher (Fx-style multiply-xor) for the internal
/// path/key maps. These maps are never iterated, so ordering cannot leak
/// into observable behavior, and the keys come from already-parsed input,
/// so hash-flooding resistance is unnecessary here.
#[derive(Default)]
struct FastHasher {
    hash: u64,
}

impl FastHasher {
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(Self::SEED);
    }
}

impl std::hash::Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_ne_bytes(chunk.try_into().expect("8-byte chunk")));
        }
        let remainder = chunks.remainder();
        if !remainder.is_empty() {
            let mut word = [0_u8; 8];
            word[..remainder.len()].copy_from_slice(remainder);
            self.add(u64::from_ne_bytes(word));
            self.add(remainder.len() as u64);
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

type FastMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FastHasher>>;

/// A decoded sequence of TOML key segments.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KeyPath(Arc<[Arc<str>]>);

impl KeyPath {
    fn new(segments: Vec<Arc<str>>) -> Self {
        Self(segments.into())
    }

    /// Iterates decoded key segments in source order.
    pub fn segments(&self) -> impl ExactSizeIterator<Item = &str> {
        self.0.iter().map(AsRef::as_ref)
    }

    /// Joins decoded segments with dots for display.
    ///
    /// This display form is not guaranteed to round-trip when a segment itself contains a dot.
    #[must_use]
    pub fn dotted(&self) -> String {
        self.segments().collect::<Vec<_>>().join(".")
    }
}

/// The source statement that introduced a declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DeclarationKind {
    /// A key-value assignment.
    KeyValue,
    /// A table header.
    Table,
    /// An array-of-tables header.
    ArrayTable,
}

/// A TOML date/time value category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DateTimeKind {
    /// A date and time with a UTC marker or numeric offset.
    OffsetDateTime,
    /// A date and time without an offset.
    LocalDateTime,
    /// A calendar date without a time.
    LocalDate,
    /// A wall-clock time without a date or offset.
    LocalTime,
}

/// A validated TOML date/time retaining source spelling and protocol-normalized spelling.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DateTimeValue {
    kind: DateTimeKind,
    raw: Arc<str>,
    canonical: Arc<str>,
}

impl DateTimeValue {
    fn from_raw(raw: &str) -> Self {
        let has_date = raw.as_bytes().get(4) == Some(&b'-') && raw.as_bytes().get(7) == Some(&b'-');
        let kind = if !has_date {
            DateTimeKind::LocalTime
        } else if raw.len() == 10 {
            DateTimeKind::LocalDate
        } else if raw[11..]
            .bytes()
            .any(|byte| matches!(byte, b'Z' | b'z' | b'+' | b'-'))
        {
            DateTimeKind::OffsetDateTime
        } else {
            DateTimeKind::LocalDateTime
        };
        Self {
            kind,
            raw: raw.into(),
            canonical: canonical_datetime(raw, kind).into(),
        }
    }

    /// Returns the date/time category.
    #[must_use]
    pub const fn kind(&self) -> DateTimeKind {
        self.kind
    }

    /// Returns the exact source spelling.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns the normalized spelling used by the `toml-test` decoder protocol.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

/// A decoded TOML value or an error-tolerant invalid placeholder.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticValue {
    /// A decoded basic or literal string.
    String(Arc<str>),
    /// A signed TOML integer.
    Integer(i64),
    /// A TOML floating-point value, including infinities and NaN.
    Float(f64),
    /// A boolean value.
    Boolean(bool),
    /// A validated date/time value.
    DateTime(DateTimeValue),
    /// An ordered TOML array.
    Array(Arc<[Self]>),
    /// Ordered decoded entries from an inline table.
    InlineTable(Arc<[(KeyPath, Self)]>),
    /// A materialized table value.
    Table(SemanticTable),
    /// Raw source for a value that could not be decoded.
    Invalid(Arc<str>),
}

impl SemanticValue {
    /// Returns the decoded string, or `None` for another value kind.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the integer, or `None` for another value kind.
    #[must_use]
    pub const fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the float, or `None` for another value kind.
    #[must_use]
    pub const fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the boolean, or `None` for another value kind.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns array elements, or `None` for another value kind.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns inline-table entries, or `None` for another value kind.
    #[must_use]
    pub fn as_inline_table(&self) -> Option<&[(KeyPath, Self)]> {
        match self {
            Self::InlineTable(entries) => Some(entries),
            _ => None,
        }
    }

    /// Returns the table, or `None` for another value kind.
    #[must_use]
    pub const fn as_table(&self) -> Option<&SemanticTable> {
        match self {
            Self::Table(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the date/time value, or `None` for another value kind.
    #[must_use]
    pub const fn as_datetime(&self) -> Option<&DateTimeValue> {
        match self {
            Self::DateTime(value) => Some(value),
            _ => None,
        }
    }
}

/// An insertion-ordered map of decoded table entries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticTable(Arc<[(Arc<str>, SemanticValue)]>);

impl SemanticTable {
    /// Returns decoded entries in declaration order.
    #[must_use]
    pub fn entries(&self) -> &[(Arc<str>, SemanticValue)] {
        &self.0
    }

    /// Looks up one direct decoded key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&SemanticValue> {
        self.0
            .iter()
            .find_map(|(candidate, value)| (candidate.as_ref() == key).then_some(value))
    }
}

/// One source declaration retained even when its key is ambiguous or conflicting.
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    key: KeyPath,
    kind: DeclarationKind,
    value: Option<SemanticValue>,
    range: TextRange,
    // Trimmed span of the first invalid payload inside `value`, so the
    // INVALID_VALUE diagnostic can point at the offending element instead
    // of the whole declaration.
    first_invalid_range: Option<TextRange>,
    scope: u32,
    element_scope: Option<u32>,
    promotes_implicit_table: bool,
}

impl Declaration {
    /// Returns the declaration's fully qualified decoded key path.
    #[must_use]
    pub const fn key(&self) -> &KeyPath {
        &self.key
    }

    /// Returns the statement kind that introduced the declaration.
    #[must_use]
    pub const fn kind(&self) -> DeclarationKind {
        self.kind
    }

    /// Returns the decoded value for key-value declarations.
    ///
    /// Table and array-of-tables declarations return `None`; use [`SemanticDocument::root`] for
    /// their materialized content.
    #[must_use]
    pub const fn value(&self) -> Option<&SemanticValue> {
        self.value.as_ref()
    }

    /// Returns the complete declaration's UTF-8 byte range.
    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }
}

/// The declaration-preserving semantic view of a parsed snapshot.
///
/// [`Self::declarations`] retains conflicts and duplicates. [`Self::root`] exposes the decoded
/// table tree, while [`Self::resolve`] reports ambiguity rather than silently choosing a value.
#[derive(Clone)]
pub struct SemanticDocument {
    declarations: Arc<[Declaration]>,
    // Built on first `resolve` call: parsing only needs the conflict scan's
    // diagnostics, so the path-lookup map is deferred until a consumer
    // actually resolves keys. `build_resolve_index` is pure, keeping racing
    // initializations deterministic.
    index: OnceLock<Arc<FastMap<KeyPath, Vec<usize>>>>,
    // Built on first access: the CLI check/format paths and most LSP
    // requests never consume the aggregated root table, so parsing skips
    // that cost. `build_semantic_root` is pure, keeping racing
    // initializations deterministic.
    root: OnceLock<SemanticTable>,
}

impl SemanticDocument {
    /// Returns all lowered declarations in source order.
    #[must_use]
    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    /// Lazily materializes and returns the decoded root table.
    #[must_use]
    pub fn root(&self) -> &SemanticTable {
        self.root
            .get_or_init(|| build_semantic_root(&self.declarations))
    }

    /// Resolves a sequence of decoded key segments without hiding duplicate declarations.
    pub fn resolve<I, S>(&self, segments: I) -> Resolution<'_>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let key = KeyPath(
            segments
                .into_iter()
                .map(|segment| Arc::<str>::from(segment.as_ref()))
                .collect(),
        );
        let index = self
            .index
            .get_or_init(|| Arc::new(build_resolve_index(&self.declarations)));
        let Some(indices) = index.get(&key) else {
            return Resolution::Missing;
        };
        if indices.len() == 1 {
            Resolution::Unique(&self.declarations[indices[0]])
        } else {
            Resolution::Ambiguous(
                indices
                    .iter()
                    .map(|&index| &self.declarations[index])
                    .collect(),
            )
        }
    }
}

impl fmt::Debug for SemanticDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticDocument")
            .field("declarations", &self.declarations)
            .finish_non_exhaustive()
    }
}

/// The result of resolving one fully qualified decoded key path.
#[derive(Debug)]
pub enum Resolution<'document> {
    /// No declaration has this path.
    Missing,
    /// Exactly one declaration has this path.
    Unique(&'document Declaration),
    /// Multiple declarations have this path.
    Ambiguous(Vec<&'document Declaration>),
}

pub(crate) struct Lowered {
    pub(crate) document: SemanticDocument,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn lower(source: &str, green: &rowan::GreenNode) -> Lowered {
    let mut state = LoweringState::new(source);
    let statement_count = green
        .children()
        .filter(|child| matches!(child, NodeOrToken::Node(_)))
        .count();
    state.declarations.reserve(statement_count);
    let mut offset = 0_usize;
    for child in green.children() {
        if let NodeOrToken::Node(node) = child {
            state.lower_statement(node, offset);
        }
        offset += usize::from(child.text_len());
    }
    state.finish()
}

fn node_range(node: &rowan::GreenNodeData, offset: usize) -> TextRange {
    TextRange::from_usize(offset, offset + usize::from(node.text_len()))
}

/// Returns the first child node with the given kind together with its
/// absolute source offset, mirroring the typed AST's `child` accessor.
fn child_node(
    node: &rowan::GreenNodeData,
    offset: usize,
    kind: SyntaxKind,
) -> Option<(&rowan::GreenNodeData, usize)> {
    let mut child_offset = offset;
    for child in node.children() {
        if let NodeOrToken::Node(child_node) = child {
            if TomlLanguage::kind_from_raw(child_node.kind()) == kind {
                return Some((child_node, child_offset));
            }
        }
        child_offset += usize::from(child.text_len());
    }
    None
}

struct LoweringState<'source> {
    source: &'source str,
    current_table: Vec<Arc<str>>,
    discard_current_table_entries: bool,
    current_scope: u32,
    next_scope: u32,
    active_array_tables: Vec<(Vec<Arc<str>>, u32)>,
    // Path-keyed indexes so per-statement namespace checks stay O(depth)
    // instead of rescanning every prior declaration. The scope lists are
    // almost always a single element.
    explicit_tables: FastMap<Vec<Arc<str>>, Vec<u32>>,
    implicit_table_paths: FastMap<Vec<Arc<str>>, Vec<u32>>,
    declared_paths: FastMap<Vec<Arc<str>>, Vec<u32>>,
    declarations: Vec<Declaration>,
    namespace_diagnostics: Vec<Diagnostic>,
}

impl<'source> LoweringState<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            current_table: Vec::new(),
            discard_current_table_entries: false,
            current_scope: 0,
            next_scope: 1,
            active_array_tables: Vec::new(),
            explicit_tables: FastMap::default(),
            implicit_table_paths: FastMap::default(),
            declared_paths: FastMap::default(),
            declarations: Vec::new(),
            namespace_diagnostics: Vec::new(),
        }
    }

    fn lower_statement(&mut self, node: &rowan::GreenNodeData, offset: usize) {
        match TomlLanguage::kind_from_raw(node.kind()) {
            SyntaxKind::Table => self.lower_table(node, offset),
            SyntaxKind::ArrayTable => self.lower_array_table(node, offset),
            SyntaxKind::KeyValue => self.lower_key_value_statement(node, offset),
            _ => {}
        }
    }

    fn lower_table(&mut self, node: &rowan::GreenNodeData, offset: usize) {
        let path = lower_statement_key_path(self.source, node, offset);
        if !self.select_table_path(path) {
            return;
        }

        let scope =
            enclosing_array_scope(&self.current_table, &self.active_array_tables).unwrap_or(0);
        self.current_scope = scope;
        let promotes_implicit_table = self.promote_implicit_table();
        self.record_implicit_parents();
        self.explicit_tables
            .entry(self.current_table.clone())
            .or_default()
            .push(scope);
        Self::record_declared_path(&mut self.declared_paths, &self.current_table, scope);
        self.declarations.push(Declaration {
            key: KeyPath::new(self.current_table.clone()),
            kind: DeclarationKind::Table,
            value: None,
            range: node_range(node, offset),
            first_invalid_range: None,
            scope,
            element_scope: None,
            promotes_implicit_table,
        });
    }

    fn lower_array_table(&mut self, node: &rowan::GreenNodeData, offset: usize) {
        let path = lower_statement_key_path(self.source, node, offset);
        if !self.select_table_path(path) {
            return;
        }

        let scope =
            enclosing_array_scope(&self.current_table, &self.active_array_tables).unwrap_or(0);
        if self
            .implicit_table_paths
            .get(&self.current_table)
            .is_some_and(|owners| owners.contains(&scope))
        {
            self.namespace_diagnostics.push(Diagnostic::error(
                DiagnosticCode::CONFLICTING_KEY,
                format!(
                    "array-of-tables conflicts with the implicitly created table `{}`",
                    self.current_table.join(".")
                ),
                node_range(node, offset),
            ));
        }
        self.record_implicit_parents();

        let element_scope = self.next_scope;
        self.next_scope = self.next_scope.saturating_add(1);
        activate_array_scope(
            &mut self.active_array_tables,
            &self.current_table,
            element_scope,
        );
        self.current_scope = element_scope;
        Self::record_declared_path(&mut self.declared_paths, &self.current_table, scope);
        self.declarations.push(Declaration {
            key: KeyPath::new(self.current_table.clone()),
            kind: DeclarationKind::ArrayTable,
            value: None,
            range: node_range(node, offset),
            first_invalid_range: None,
            scope,
            element_scope: Some(element_scope),
            promotes_implicit_table: false,
        });
    }

    fn lower_key_value_statement(&mut self, node: &rowan::GreenNodeData, offset: usize) {
        if self.discard_current_table_entries {
            return;
        }
        let relative_path = lower_statement_key_path(self.source, node, offset);
        if relative_path.len() > MAX_KEY_DEPTH {
            return;
        }
        if self.current_table.len().saturating_add(relative_path.len()) > MAX_KEY_DEPTH {
            self.namespace_diagnostics.push(Diagnostic::error(
                DiagnosticCode::NESTING_LIMIT,
                format!("key nesting exceeds the supported limit of {MAX_KEY_DEPTH}"),
                node_range(node, offset),
            ));
            return;
        }
        let declaration_start = self.declarations.len();
        lower_key_value(
            self.source,
            node,
            offset,
            &self.current_table,
            relative_path,
            self.current_scope,
            &mut self.declarations,
        );
        let Some(declaration) = self.declarations.get(declaration_start).cloned() else {
            return;
        };
        let key = Arc::clone(&declaration.key.0);
        Self::record_declared_path(&mut self.declared_paths, &key, declaration.scope);
        self.diagnose_array_table_extension(&declaration, &key);
        self.diagnose_cross_header_dotted_key(&declaration, &key);
    }

    fn select_table_path(&mut self, path: Vec<Arc<str>>) -> bool {
        self.current_scope = 0;
        self.discard_current_table_entries = path.is_empty() || path.len() > MAX_KEY_DEPTH;
        self.current_table = if self.discard_current_table_entries {
            Vec::new()
        } else {
            path
        };
        !self.discard_current_table_entries
    }

    fn promote_implicit_table(&mut self) -> bool {
        let Some(owners) = self.implicit_table_paths.get_mut(&self.current_table) else {
            return false;
        };
        let Some(index) = owners.iter().position(|owner| *owner == self.current_scope) else {
            return false;
        };
        owners.swap_remove(index);
        true
    }

    fn record_declared_path(
        declared_paths: &mut FastMap<Vec<Arc<str>>, Vec<u32>>,
        path: &[Arc<str>],
        scope: u32,
    ) {
        if let Some(owners) = declared_paths.get_mut(path) {
            if !owners.contains(&scope) {
                owners.push(scope);
            }
        } else {
            declared_paths.insert(path.to_vec(), vec![scope]);
        }
    }

    fn record_implicit_parents(&mut self) {
        for prefix_length in 1..self.current_table.len() {
            let prefix = &self.current_table[..prefix_length];
            let scope = enclosing_array_scope(prefix, &self.active_array_tables).unwrap_or(0);
            let explicitly_known = self
                .declared_paths
                .get(prefix)
                .is_some_and(|owners| owners.contains(&scope));
            let already_implicit = self
                .implicit_table_paths
                .get(prefix)
                .is_some_and(|owners| owners.contains(&scope));
            if !explicitly_known && !already_implicit {
                self.implicit_table_paths
                    .entry(prefix.to_vec())
                    .or_default()
                    .push(scope);
            }
        }
    }

    fn diagnose_array_table_extension(&mut self, declaration: &Declaration, key: &[Arc<str>]) {
        let Some(array_scope) = enclosing_array_scope(key, &self.active_array_tables) else {
            return;
        };
        if array_scope != declaration.scope {
            self.namespace_diagnostics.push(Diagnostic::error(
                DiagnosticCode::CONFLICTING_KEY,
                format!(
                    "dotted key cannot extend an array-of-tables outside its current element: `{}`",
                    declaration.key.dotted()
                ),
                declaration.range,
            ));
        }
    }

    fn diagnose_cross_header_dotted_key(&mut self, declaration: &Declaration, key: &[Arc<str>]) {
        // A conflicting explicit table lies strictly between the current
        // header path and the full key, so only key prefixes of those
        // lengths need to be probed.
        let crosses_header = (self.current_table.len() + 1..key.len()).any(|prefix_length| {
            self.explicit_tables
                .get(&key[..prefix_length])
                .is_some_and(|owners| owners.contains(&declaration.scope))
        });
        if crosses_header {
            self.namespace_diagnostics.push(Diagnostic::error(
                DiagnosticCode::CONFLICTING_KEY,
                format!(
                    "dotted key cannot extend a table declared under a different header: `{}`",
                    declaration.key.dotted()
                ),
                declaration.range,
            ));
        }
    }

    fn finish(self) -> Lowered {
        let mut diagnostics = declaration_diagnostics(&self.declarations);
        diagnostics.extend(self.namespace_diagnostics);

        Lowered {
            document: SemanticDocument {
                declarations: self.declarations.into(),
                index: OnceLock::new(),
                root: OnceLock::new(),
            },
            diagnostics,
        }
    }
}

fn build_resolve_index(declarations: &[Declaration]) -> FastMap<KeyPath, Vec<usize>> {
    let mut index: FastMap<KeyPath, Vec<usize>> = FastMap::default();
    for (declaration_index, declaration) in declarations.iter().enumerate() {
        index
            .entry(declaration.key.clone())
            .or_default()
            .push(declaration_index);
    }
    index
}

fn declaration_diagnostics(declarations: &[Declaration]) -> Vec<Diagnostic> {
    let mut conflict_index = ConflictIndex::default();
    let mut diagnostics = Vec::new();
    for (declaration_index, declaration) in declarations.iter().enumerate() {
        if let Some(value) = declaration.value.as_ref() {
            collect_inline_conflicts(value, declaration.range, &mut diagnostics);
            if contains_invalid(value) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::INVALID_VALUE,
                    format!("invalid value for `{}`", declaration.key.dotted()),
                    declaration.first_invalid_range.unwrap_or(declaration.range),
                ));
            }
        }
        if !declaration.promotes_implicit_table {
            if let Some(previous_index) = conflict_index.first_conflict(declaration) {
                let previous = &declarations[previous_index];
                let code = declaration_conflict_code(previous, declaration)
                    .expect("the conflict index only returns conflicting declarations");
                diagnostics.push(
                    Diagnostic::error(
                        code,
                        format!("conflicting declaration for `{}`", declaration.key.dotted()),
                        declaration.range,
                    )
                    .with_related_range(previous.range),
                );
            }
        }
        conflict_index.insert(declaration, declaration_index);
    }
    diagnostics
}

fn declaration_conflict_code(
    previous: &Declaration,
    current: &Declaration,
) -> Option<DiagnosticCode> {
    let previous_key = &previous.key.0;
    let current_key = &current.key.0;
    let same_key = previous_key == current_key;
    let prefix_conflict = (is_prefix(previous_key, current_key)
        && previous.kind == DeclarationKind::KeyValue)
        || (is_prefix(current_key, previous_key)
            && (current.kind != DeclarationKind::Table
                || previous.kind == DeclarationKind::KeyValue));
    if same_key
        && previous.kind == DeclarationKind::KeyValue
        && current.kind == DeclarationKind::KeyValue
    {
        Some(DiagnosticCode::DUPLICATE_KEY)
    } else if (same_key || prefix_conflict)
        && !(previous.kind == DeclarationKind::ArrayTable
            && current.kind == DeclarationKind::ArrayTable)
    {
        Some(DiagnosticCode::CONFLICTING_KEY)
    } else {
        None
    }
}

#[derive(Default)]
struct ConflictIndex {
    scopes: FastMap<u32, ConflictNode>,
}

impl ConflictIndex {
    fn first_conflict(&self, declaration: &Declaration) -> Option<usize> {
        let mut node = self.scopes.get(&declaration.scope)?;
        let mut first = None;

        for segment in declaration.key.0.iter() {
            // The node at this point is a strict ancestor of the current key.
            // Only scalar declarations make their descendant namespace invalid.
            take_earlier(&mut first, node.exact.key_value);
            let Some(child) = node.children.get(segment) else {
                return first;
            };
            node = child;
        }

        let exact = match declaration.kind {
            DeclarationKind::ArrayTable => node.exact.non_array_table(),
            DeclarationKind::KeyValue | DeclarationKind::Table => node.exact.any(),
        };
        take_earlier(&mut first, exact);

        let descendant = match declaration.kind {
            DeclarationKind::KeyValue => node.descendants.any(),
            DeclarationKind::Table => node.descendants.key_value,
            DeclarationKind::ArrayTable => node.descendants.non_array_table(),
        };
        take_earlier(&mut first, descendant);
        first
    }

    fn insert(&mut self, declaration: &Declaration, declaration_index: usize) {
        let mut node = self.scopes.entry(declaration.scope).or_default();
        for segment in declaration.key.0.iter() {
            node.descendants.record(declaration.kind, declaration_index);
            node = node.children.entry(Arc::clone(segment)).or_default();
        }
        node.exact.record(declaration.kind, declaration_index);
    }
}

#[derive(Default)]
struct ConflictNode {
    children: FastMap<Arc<str>, Self>,
    exact: EarliestByKind,
    descendants: EarliestByKind,
}

#[derive(Default)]
struct EarliestByKind {
    key_value: Option<usize>,
    table: Option<usize>,
    array_table: Option<usize>,
}

impl EarliestByKind {
    fn record(&mut self, kind: DeclarationKind, declaration_index: usize) {
        let slot = match kind {
            DeclarationKind::KeyValue => &mut self.key_value,
            DeclarationKind::Table => &mut self.table,
            DeclarationKind::ArrayTable => &mut self.array_table,
        };
        slot.get_or_insert(declaration_index);
    }

    fn any(&self) -> Option<usize> {
        earliest([self.key_value, self.table, self.array_table])
    }

    fn non_array_table(&self) -> Option<usize> {
        earliest([self.key_value, self.table])
    }
}

fn earliest<const N: usize>(indices: [Option<usize>; N]) -> Option<usize> {
    indices.into_iter().flatten().min()
}

fn take_earlier(first: &mut Option<usize>, candidate: Option<usize>) {
    if let Some(candidate) = candidate {
        *first = Some(first.map_or(candidate, |first| first.min(candidate)));
    }
}

#[derive(Default)]
struct InlineConflictNode {
    children: FastMap<Arc<str>, Self>,
    exact: Option<usize>,
    descendant: Option<usize>,
}

fn take_earlier_conflict(
    first: &mut Option<(usize, DiagnosticCode)>,
    candidate: Option<usize>,
    code: DiagnosticCode,
) {
    if let Some(candidate) = candidate {
        if first.is_none_or(|(existing, _)| candidate < existing) {
            *first = Some((candidate, code));
        }
    }
}

fn collect_inline_conflicts(
    value: &SemanticValue,
    range: TextRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match value {
        SemanticValue::Array(values) => {
            for value in values.iter() {
                collect_inline_conflicts(value, range, diagnostics);
            }
        }
        SemanticValue::InlineTable(entries) => {
            let mut trie = InlineConflictNode::default();
            for (index, (key, value)) in entries.iter().enumerate() {
                // Mirror the pairwise scan this trie replaces: report the
                // conflict against the earliest previous entry, at most
                // once per entry.
                let mut conflict: Option<(usize, DiagnosticCode)> = None;
                let mut node = &trie;
                let mut reached_end = true;
                for segment in key.0.iter() {
                    take_earlier_conflict(
                        &mut conflict,
                        node.exact,
                        DiagnosticCode::CONFLICTING_KEY,
                    );
                    let Some(child) = node.children.get(segment) else {
                        reached_end = false;
                        break;
                    };
                    node = child;
                }
                if reached_end {
                    take_earlier_conflict(&mut conflict, node.exact, DiagnosticCode::DUPLICATE_KEY);
                    take_earlier_conflict(
                        &mut conflict,
                        node.descendant,
                        DiagnosticCode::CONFLICTING_KEY,
                    );
                }
                if let Some((_, code)) = conflict {
                    diagnostics.push(Diagnostic::error(
                        code,
                        format!("conflicting inline-table entry for `{}`", key.dotted()),
                        range,
                    ));
                }

                let mut node = &mut trie;
                for segment in key.0.iter() {
                    node.descendant.get_or_insert(index);
                    node = node.children.entry(Arc::clone(segment)).or_default();
                }
                node.exact.get_or_insert(index);

                collect_inline_conflicts(value, range, diagnostics);
            }
        }
        SemanticValue::Table(table) => {
            for (_, value) in table.entries() {
                collect_inline_conflicts(value, range, diagnostics);
            }
        }
        SemanticValue::String(_)
        | SemanticValue::Integer(_)
        | SemanticValue::Float(_)
        | SemanticValue::Boolean(_)
        | SemanticValue::DateTime(_)
        | SemanticValue::Invalid(_) => {}
    }
}

fn contains_invalid(value: &SemanticValue) -> bool {
    match value {
        SemanticValue::Invalid(_) => true,
        SemanticValue::Array(values) => values.iter().any(contains_invalid),
        SemanticValue::InlineTable(entries) => {
            entries.iter().any(|(_, value)| contains_invalid(value))
        }
        SemanticValue::Table(table) => table
            .entries()
            .iter()
            .any(|(_, value)| contains_invalid(value)),
        SemanticValue::String(_)
        | SemanticValue::Integer(_)
        | SemanticValue::Float(_)
        | SemanticValue::Boolean(_)
        | SemanticValue::DateTime(_) => false,
    }
}

fn lower_key_value(
    source: &str,
    node: &rowan::GreenNodeData,
    offset: usize,
    current_table: &[Arc<str>],
    relative_path: Vec<Arc<str>>,
    scope: u32,
    declarations: &mut Vec<Declaration>,
) {
    let Some((value_node, value_offset)) = child_node(node, offset, SyntaxKind::Value) else {
        return;
    };
    if relative_path.is_empty() {
        return;
    }
    let mut key = current_table.to_vec();
    key.extend(relative_path);
    let mut first_invalid_range = None;
    let value = lower_value(source, value_node, value_offset, &mut first_invalid_range);
    declarations.push(Declaration {
        key: KeyPath::new(key),
        kind: DeclarationKind::KeyValue,
        value: Some(value),
        range: node_range(node, offset),
        first_invalid_range,
        scope,
        element_scope: None,
        promotes_implicit_table: false,
    });
}

fn enclosing_array_scope(path: &[Arc<str>], active: &[(Vec<Arc<str>>, u32)]) -> Option<u32> {
    let mut selected = None;
    let mut selected_length = 0;
    for (candidate, scope) in active {
        if is_prefix(candidate, path) && candidate.len() >= selected_length {
            selected = Some(*scope);
            selected_length = candidate.len();
        }
    }
    selected
}

fn retire_array_scopes_at_or_below(active: &mut Vec<(Vec<Arc<str>>, u32)>, path: &[Arc<str>]) {
    active.retain(|(candidate, _)| {
        candidate.as_slice() != path && !is_prefix(path, candidate.as_slice())
    });
}

fn activate_array_scope(active: &mut Vec<(Vec<Arc<str>>, u32)>, path: &[Arc<str>], scope: u32) {
    retire_array_scopes_at_or_below(active, path);
    active.push((path.to_vec(), scope));
}

#[derive(Default)]
struct MutableTable {
    entries: Vec<(Arc<str>, MutableEntry)>,
    // Built lazily: small tables are faster to scan than to hash.
    indices: Option<FastMap<Arc<str>, usize>>,
}

const TABLE_INDEX_THRESHOLD: usize = 8;

enum MutableEntry {
    Value(SemanticValue),
    Table(MutableTable),
    ArrayTables(Vec<MutableTable>),
}

impl MutableTable {
    fn position(&self, key: &str) -> Option<usize> {
        if let Some(indices) = &self.indices {
            indices.get(key).copied()
        } else {
            self.entries
                .iter()
                .position(|(candidate, _)| candidate.as_ref() == key)
        }
    }

    fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    fn entry_mut(&mut self, key: &str) -> Option<&mut MutableEntry> {
        let index = self.position(key)?;
        Some(&mut self.entries[index].1)
    }

    fn insert(&mut self, key: &Arc<str>, entry: MutableEntry) -> bool {
        if self.contains_key(key.as_ref()) {
            return false;
        }
        let index = self.entries.len();
        if self.indices.is_none() && index >= TABLE_INDEX_THRESHOLD {
            self.indices = Some(
                self.entries
                    .iter()
                    .enumerate()
                    .map(|(position, (existing, _))| (Arc::clone(existing), position))
                    .collect(),
            );
        }
        if let Some(indices) = &mut self.indices {
            indices.insert(Arc::clone(key), index);
        }
        self.entries.push((Arc::clone(key), entry));
        true
    }
}

#[derive(Clone)]
enum LocationStep {
    Table(Arc<str>),
    ArrayElement { key: Arc<str>, index: usize },
}

struct ScopeContext {
    logical_path: Arc<[Arc<str>]>,
    location: Vec<LocationStep>,
}

fn build_semantic_root(declarations: &[Declaration]) -> SemanticTable {
    let mut root = MutableTable::default();
    let mut scopes = HashMap::from([(
        0_u32,
        ScopeContext {
            logical_path: Arc::from([]),
            location: Vec::new(),
        },
    )]);

    for declaration in declarations {
        let Some(context) = scopes.get(&declaration.scope) else {
            continue;
        };
        let path = &declaration.key.0;
        let relative = path
            .strip_prefix(context.logical_path.as_ref())
            .unwrap_or(path.as_ref());

        match declaration.kind {
            DeclarationKind::Table => {
                let Some(table) = table_at_location_mut(&mut root, &context.location) else {
                    continue;
                };
                ensure_table_path(table, relative);
            }
            DeclarationKind::ArrayTable => {
                let Some(element_scope) = declaration.element_scope else {
                    continue;
                };
                let mut location = context.location.clone();
                let Some(table) = table_at_location_mut(&mut root, &context.location) else {
                    continue;
                };
                if let Some(relative_location) = append_array_table(table, relative) {
                    location.extend(relative_location);
                    scopes.insert(
                        element_scope,
                        ScopeContext {
                            logical_path: Arc::clone(path),
                            location,
                        },
                    );
                }
            }
            DeclarationKind::KeyValue => {
                let Some(table) = table_at_location_mut(&mut root, &context.location) else {
                    continue;
                };
                if let Some(value) = declaration.value.clone() {
                    insert_value(table, relative, value);
                }
            }
        }
    }

    freeze_table(root)
}

fn table_at_location_mut<'table>(
    table: &'table mut MutableTable,
    location: &[LocationStep],
) -> Option<&'table mut MutableTable> {
    let Some((step, remainder)) = location.split_first() else {
        return Some(table);
    };
    let next = match step {
        LocationStep::Table(key) => match table.entry_mut(key)? {
            MutableEntry::Table(table) => table,
            MutableEntry::Value(_) | MutableEntry::ArrayTables(_) => return None,
        },
        LocationStep::ArrayElement { key, index } => match table.entry_mut(key)? {
            MutableEntry::ArrayTables(tables) => tables.get_mut(*index)?,
            MutableEntry::Value(_) | MutableEntry::Table(_) => return None,
        },
    };
    table_at_location_mut(next, remainder)
}

fn ensure_table_path(table: &mut MutableTable, path: &[Arc<str>]) -> bool {
    let Some((key, remainder)) = path.split_first() else {
        return true;
    };
    if !table.contains_key(key.as_ref()) {
        table.insert(key, MutableEntry::Table(MutableTable::default()));
    }
    let Some(MutableEntry::Table(child)) = table.entry_mut(key) else {
        return false;
    };
    ensure_table_path(child, remainder)
}

fn append_array_table(table: &mut MutableTable, path: &[Arc<str>]) -> Option<Vec<LocationStep>> {
    let (key, parents) = path.split_last()?;
    let mut current = table;
    let mut location = Vec::with_capacity(path.len());

    for parent in parents {
        if !current.contains_key(parent.as_ref()) {
            current.insert(parent, MutableEntry::Table(MutableTable::default()));
        }
        let MutableEntry::Table(child) = current.entry_mut(parent)? else {
            return None;
        };
        current = child;
        location.push(LocationStep::Table(Arc::clone(parent)));
    }

    if !current.contains_key(key.as_ref()) {
        current.insert(key, MutableEntry::ArrayTables(Vec::new()));
    }
    let MutableEntry::ArrayTables(elements) = current.entry_mut(key)? else {
        return None;
    };
    let index = elements.len();
    elements.push(MutableTable::default());
    location.push(LocationStep::ArrayElement {
        key: Arc::clone(key),
        index,
    });
    Some(location)
}

fn insert_value(table: &mut MutableTable, path: &[Arc<str>], value: SemanticValue) -> bool {
    let Some((key, remainder)) = path.split_first() else {
        return false;
    };
    if remainder.is_empty() {
        return table.insert(key, MutableEntry::Value(value));
    }
    if !table.contains_key(key.as_ref()) {
        table.insert(key, MutableEntry::Table(MutableTable::default()));
    }
    let Some(MutableEntry::Table(child)) = table.entry_mut(key) else {
        return false;
    };
    insert_value(child, remainder, value)
}

fn freeze_table(table: MutableTable) -> SemanticTable {
    SemanticTable(
        table
            .entries
            .into_iter()
            .map(|(key, entry)| {
                let value = match entry {
                    MutableEntry::Value(value) => value,
                    MutableEntry::Table(table) => SemanticValue::Table(freeze_table(table)),
                    MutableEntry::ArrayTables(tables) => SemanticValue::Array(
                        tables
                            .into_iter()
                            .map(|table| SemanticValue::Table(freeze_table(table)))
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                };
                (key, value)
            })
            .collect::<Vec<_>>()
            .into(),
    )
}

fn is_prefix<T: PartialEq>(prefix: &[T], path: &[T]) -> bool {
    prefix.len() < path.len() && path.starts_with(prefix)
}

fn source_slice(source: &str, range: TextRange) -> &str {
    &source[range.start() as usize..range.end() as usize]
}

/// Lowers the key path of a table header, array-of-tables header, or
/// key-value statement from its `Key` child node.
fn lower_statement_key_path(
    source: &str,
    statement: &rowan::GreenNodeData,
    offset: usize,
) -> Vec<Arc<str>> {
    let Some((key_node, key_offset)) = child_node(statement, offset, SyntaxKind::Key) else {
        return Vec::new();
    };
    lower_key_node(source, key_node, key_offset)
}

/// Token-driven dotted-key split: segments are the runs between `Dot`
/// tokens at bracket depth zero, where depth counts bracket tokens with two
/// independent saturating counters. String tokens are opaque, so dots and
/// quotes inside them never split. Segments trim to their text, empty
/// segments drop out, and quoted segments decode through the literal
/// parser.
fn lower_key_node(source: &str, key_node: &rowan::GreenNodeData, offset: usize) -> Vec<Arc<str>> {
    let mut segments = Vec::new();
    let mut square_depth = 0_usize;
    let mut brace_depth = 0_usize;
    let mut segment_start = offset;
    let mut child_offset = offset;
    for child in key_node.children() {
        let length = usize::from(child.text_len());
        let start = child_offset;
        child_offset += length;
        let NodeOrToken::Token(token) = child else {
            // Key nodes only hold tokens; treat anything else as inert
            // segment text.
            continue;
        };
        match TomlLanguage::kind_from_raw(token.kind()) {
            SyntaxKind::Dot if square_depth == 0 && brace_depth == 0 => {
                push_key_segment(source, segment_start, start, &mut segments);
                segment_start = start + length;
            }
            SyntaxKind::LeftBracket => square_depth += 1,
            SyntaxKind::RightBracket => square_depth = square_depth.saturating_sub(1),
            SyntaxKind::LeftBrace => brace_depth += 1,
            SyntaxKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }
    push_key_segment(source, segment_start, child_offset, &mut segments);
    segments
}

fn push_key_segment(source: &str, start: usize, end: usize, segments: &mut Vec<Arc<str>>) {
    let segment = source[start..end].trim();
    if !segment.is_empty() {
        segments.push(decode_key(segment));
    }
}

fn decode_key(raw: &str) -> Arc<str> {
    if matches!(raw.as_bytes().first(), Some(b'"' | b'\'')) {
        if let Some(parsed) = literal::parse(raw) {
            if let LiteralValue::String(value) = parsed.value {
                return value.into();
            }
        }
    }
    raw.into()
}

const MAX_VALUE_DEPTH: usize = 256;

/// Lowers a statement's `Value` node from the green subtree alone: the
/// lexer/parser structure is the single source of truth. Collections lower
/// element by element from their nodes, scalars from the trimmed node text,
/// and any payload that is not one well-formed value becomes
/// [`SemanticValue::Invalid`] carrying the trimmed source slice of its own
/// span. `first_invalid` receives the range of the first invalid payload in
/// source order, which the `INVALID_VALUE` diagnostic points at.
fn lower_value(
    source: &str,
    value_node: &rowan::GreenNodeData,
    offset: usize,
    first_invalid: &mut Option<TextRange>,
) -> SemanticValue {
    lower_value_node(source, value_node, offset, 0, first_invalid)
}

/// `depth` counts enclosing collections; the parser flattens the collection
/// that would exceed [`MAX_VALUE_DEPTH`], and lowering mirrors that bound.
fn lower_value_node(
    source: &str,
    value_node: &rowan::GreenNodeData,
    offset: usize,
    depth: usize,
    first_invalid: &mut Option<TextRange>,
) -> SemanticValue {
    let mut collection: Option<(&rowan::GreenNodeData, usize)> = None;
    let mut stray_token = false;
    let mut child_offset = offset;
    for child in value_node.children() {
        let length = usize::from(child.text_len());
        let start = child_offset;
        child_offset += length;
        match child {
            NodeOrToken::Node(child_node) => {
                if collection.is_some() {
                    stray_token = true;
                } else {
                    collection = Some((child_node, start));
                }
            }
            NodeOrToken::Token(token) => {
                if TomlLanguage::kind_from_raw(token.kind()) != SyntaxKind::Whitespace {
                    stray_token = true;
                }
            }
        }
    }
    match collection {
        Some((node, node_offset)) if !stray_token => {
            lower_collection(source, node, node_offset, depth, first_invalid)
        }
        // A bare token run, an empty node, or a collection followed by
        // stray tokens: the payload is not one well-formed value, so it
        // lowers as one scalar-or-invalid over the trimmed node text.
        _ => lower_scalar(source, offset, child_offset, first_invalid),
    }
}

fn lower_scalar(
    source: &str,
    start: usize,
    end: usize,
    first_invalid: &mut Option<TextRange>,
) -> SemanticValue {
    let trimmed = source[start..end].trim();
    if let Some(parsed) = literal::parse(trimmed) {
        return match parsed.value {
            LiteralValue::String(value) => SemanticValue::String(value.into()),
            LiteralValue::Integer(value) => SemanticValue::Integer(value),
            LiteralValue::Float(value) => SemanticValue::Float(value),
            LiteralValue::Boolean(value) => SemanticValue::Boolean(value),
            LiteralValue::DateTime => SemanticValue::DateTime(DateTimeValue::from_raw(trimmed)),
        };
    }
    invalid_value(source, start, end, first_invalid)
}

/// Builds an invalid payload over the trimmed portion of `[start, end)` and
/// records the first such range for the `INVALID_VALUE` diagnostic.
fn invalid_value(
    source: &str,
    start: usize,
    end: usize,
    first_invalid: &mut Option<TextRange>,
) -> SemanticValue {
    let text = &source[start..end];
    let leading = text.len() - text.trim_start().len();
    let trimmed = text.trim();
    let range = TextRange::from_usize(start + leading, start + leading + trimmed.len());
    if first_invalid.is_none() {
        *first_invalid = Some(range);
    }
    SemanticValue::Invalid(trimmed.into())
}

fn lower_collection(
    source: &str,
    node: &rowan::GreenNodeData,
    offset: usize,
    depth: usize,
    first_invalid: &mut Option<TextRange>,
) -> SemanticValue {
    let end = offset + usize::from(node.text_len());
    if depth >= MAX_VALUE_DEPTH {
        // The parser stored this collection as a flat token soup without
        // inner structure; the payload lowers as one invalid value.
        return invalid_value(source, offset, end, first_invalid);
    }
    match TomlLanguage::kind_from_raw(node.kind()) {
        SyntaxKind::Array => lower_array(source, node, offset, depth, first_invalid),
        SyntaxKind::InlineTable => lower_inline_table(source, node, offset, depth, first_invalid),
        // Value nodes only nest Array and InlineTable nodes.
        _ => invalid_value(source, offset, end, first_invalid),
    }
}

fn lower_array(
    source: &str,
    node: &rowan::GreenNodeData,
    offset: usize,
    depth: usize,
    first_invalid: &mut Option<TextRange>,
) -> SemanticValue {
    let mut elements = Vec::new();
    let mut stray: Option<(usize, usize)> = None;
    let mut child_offset = offset;
    for child in node.children() {
        let length = usize::from(child.text_len());
        let start = child_offset;
        child_offset += length;
        match child {
            NodeOrToken::Node(element)
                if TomlLanguage::kind_from_raw(element.kind()) == SyntaxKind::Value =>
            {
                flush_stray(source, &mut stray, &mut elements, first_invalid);
                // Whitespace-only parts do not produce elements.
                if source_slice(source, node_range(element, start))
                    .trim()
                    .is_empty()
                {
                    continue;
                }
                elements.push(lower_value_node(
                    source,
                    element,
                    start,
                    depth + 1,
                    first_invalid,
                ));
            }
            NodeOrToken::Node(_) => extend_stray(&mut stray, start, length),
            NodeOrToken::Token(token) => match TomlLanguage::kind_from_raw(token.kind()) {
                SyntaxKind::Newline
                | SyntaxKind::Comment
                | SyntaxKind::Comma
                | SyntaxKind::RightBracket => {
                    flush_stray(source, &mut stray, &mut elements, first_invalid);
                }
                SyntaxKind::LeftBracket if start == offset => {}
                SyntaxKind::Whitespace => {
                    if let Some((_, end)) = stray.as_mut() {
                        *end = start + length;
                    }
                }
                // Parser-recovery leftovers between separators become one
                // invalid element per contiguous run.
                _ => extend_stray(&mut stray, start, length),
            },
        }
    }
    flush_stray(source, &mut stray, &mut elements, first_invalid);
    SemanticValue::Array(elements.into())
}

fn extend_stray(stray: &mut Option<(usize, usize)>, start: usize, length: usize) {
    match stray {
        Some((_, end)) => *end = start + length,
        None => *stray = Some((start, start + length)),
    }
}

fn flush_stray(
    source: &str,
    stray: &mut Option<(usize, usize)>,
    elements: &mut Vec<SemanticValue>,
    first_invalid: &mut Option<TextRange>,
) {
    if let Some((start, end)) = stray.take() {
        if !source[start..end].trim().is_empty() {
            elements.push(invalid_value(source, start, end, first_invalid));
        }
    }
}

fn lower_inline_table(
    source: &str,
    node: &rowan::GreenNodeData,
    offset: usize,
    depth: usize,
    first_invalid: &mut Option<TextRange>,
) -> SemanticValue {
    let mut entries = Vec::new();
    let mut child_offset = offset;
    for child in node.children() {
        let length = usize::from(child.text_len());
        let start = child_offset;
        child_offset += length;
        // Delimiters, separators, trivia, and recovery leftovers carry no
        // entries.
        let NodeOrToken::Node(entry) = child else {
            continue;
        };
        if TomlLanguage::kind_from_raw(entry.kind()) != SyntaxKind::KeyValue {
            continue;
        }
        if let Some(entry) = lower_entry(source, entry, start, depth, first_invalid) {
            entries.push(entry);
        }
    }
    SemanticValue::InlineTable(entries.into())
}

/// Returns `None` for an entry with no `=` token: it binds no key to a
/// value, the parser has already diagnosed it, and it drops out of the
/// semantic table.
fn lower_entry(
    source: &str,
    kv_node: &rowan::GreenNodeData,
    offset: usize,
    depth: usize,
    first_invalid: &mut Option<TextRange>,
) -> Option<(KeyPath, SemanticValue)> {
    let mut key: Option<(&rowan::GreenNodeData, usize)> = None;
    let mut equals = false;
    let mut value: Option<(&rowan::GreenNodeData, usize)> = None;
    let mut child_offset = offset;
    for child in kv_node.children() {
        let length = usize::from(child.text_len());
        let start = child_offset;
        child_offset += length;
        match child {
            NodeOrToken::Node(child_node) => match TomlLanguage::kind_from_raw(child_node.kind()) {
                SyntaxKind::Key if key.is_none() => key = Some((child_node, start)),
                SyntaxKind::Value if value.is_none() => value = Some((child_node, start)),
                _ => {}
            },
            NodeOrToken::Token(token) => {
                if TomlLanguage::kind_from_raw(token.kind()) == SyntaxKind::Equals {
                    equals = true;
                }
            }
        }
    }
    if !equals {
        return None;
    }
    let (key_node, key_offset) = key?;
    let (value_node, value_offset) = value?;
    let key_path = lower_key_node(source, key_node, key_offset);
    let value = lower_value_node(source, value_node, value_offset, depth + 1, first_invalid);
    Some((KeyPath::new(key_path), value))
}

fn canonical_datetime(raw: &str, kind: DateTimeKind) -> String {
    match kind {
        DateTimeKind::LocalDate => raw.to_owned(),
        DateTimeKind::LocalTime => canonical_time(raw),
        DateTimeKind::OffsetDateTime | DateTimeKind::LocalDateTime => {
            let mut canonical = String::with_capacity(raw.len() + 3);
            canonical.push_str(&raw[..10]);
            canonical.push('T');
            canonical.push_str(&canonical_time(&raw[11..]));
            canonical
        }
    }
}

fn canonical_time(raw: &str) -> String {
    let suffix_start = raw
        .char_indices()
        .find_map(|(index, character)| {
            matches!(character, '.' | 'Z' | 'z' | '+' | '-').then_some(index)
        })
        .unwrap_or(raw.len());
    let (clock, suffix) = raw.split_at(suffix_start);
    let mut canonical = if clock.bytes().filter(|byte| *byte == b':').count() == 1 {
        format!("{clock}:00{suffix}")
    } else {
        raw.to_owned()
    };
    if canonical.ends_with('z') {
        canonical.pop();
        canonical.push('Z');
    }
    canonical
}

#[cfg(test)]
mod value_lowering_tests {
    use proptest::prelude::*;

    use super::*;

    /// Structural invariants that hold for every input, however degenerate:
    /// lowering terminates, every invalid payload is a trimmed slice of the
    /// source, and the recorded first-invalid range points at in-bounds,
    /// self-trimmed text inside its declaration. The explicit semantics of
    /// each degenerate payload class live in
    /// `tests/value_lowering_edges.rs` at the public API level.
    fn assert_invariants(source: &str) {
        let parse = crate::syntax::parse(source);
        let lowered = super::lower(source, &parse.green);
        for declaration in lowered.document.declarations() {
            if let Some(value) = declaration.value.as_ref() {
                assert_invalid_payloads_are_trimmed_slices(source, value);
                if declaration.first_invalid_range.is_none() {
                    assert!(
                        !contains_invalid(value),
                        "unrecorded invalid payload in {source:?}"
                    );
                }
            }
            if let Some(range) = declaration.first_invalid_range {
                assert!(
                    declaration.range.start() <= range.start()
                        && range.end() <= declaration.range.end(),
                    "first-invalid range {range:?} escapes {:?} in {source:?}",
                    declaration.range,
                );
                let slice = &source[range.start() as usize..range.end() as usize];
                assert_eq!(
                    slice.trim(),
                    slice,
                    "untrimmed diagnostic slice in {source:?}"
                );
            }
        }
        for diagnostic in &lowered.diagnostics {
            let range = diagnostic.range();
            assert!(
                range.start() <= range.end() && (range.end() as usize) <= source.len(),
                "diagnostic range {range:?} out of bounds in {source:?}"
            );
        }
    }

    fn assert_invalid_payloads_are_trimmed_slices(source: &str, value: &SemanticValue) {
        match value {
            SemanticValue::Invalid(text) => {
                assert_eq!(text.trim(), &**text, "untrimmed payload in {source:?}");
                assert!(
                    text.is_empty() || source.contains(&**text),
                    "payload {text:?} is not a slice of {source:?}"
                );
            }
            SemanticValue::Array(values) => {
                for value in values.iter() {
                    assert_invalid_payloads_are_trimmed_slices(source, value);
                }
            }
            SemanticValue::InlineTable(entries) => {
                for (_, value) in entries.iter() {
                    assert_invalid_payloads_are_trimmed_slices(source, value);
                }
            }
            SemanticValue::Table(table) => {
                for (_, value) in table.entries() {
                    assert_invalid_payloads_are_trimmed_slices(source, value);
                }
            }
            SemanticValue::String(_)
            | SemanticValue::Integer(_)
            | SemanticValue::Float(_)
            | SemanticValue::Boolean(_)
            | SemanticValue::DateTime(_) => {}
        }
    }

    // Degenerate payload corpus collected while retiring the splitter; the
    // invariants above must hold on every one of them.
    const EDGE_CASES: &[&str] = &[
        // Clean structural anchors.
        "a = [1, 2, 3]\n",
        "a = []\n",
        "a = [ ]\n",
        "a = [1,]\n",
        "a = [,]\n",
        "a = [1,, 2]\n",
        "a = [\n  1,\n  # retained\n  2,\n]\n",
        "a = {}\n",
        "a = { }\n",
        "a = {x = 1, y = \"two\", }\n",
        "a = {x.y = 1, \"q\".z = 2}\n",
        "a = { x = { y = [1, {z = 2}] } }\n",
        "a = [1,\r\n2]\r\n",
        "a = [1.5, 07:32:00]\n",
        "a = [\"a,b\", 2]\n",
        "a = [1, \"]\" , 2]\n",
        "values = ['''don't, split''', 2]\n",
        // Scalars.
        "a = nan\n",
        "a = -nan\n",
        "a = +inf\n",
        "a = 1979-05-27 07:32z\n",
        "a = 07:32\n",
        "a = \"b#c\"\n",
        "a = \"\"\n",
        "a = \"\"\"m\nl\"\"\"\n",
        "a = '''a'b'''\n",
        "a = \"es\\\\\"c\"\n",
        "a = 01\n",
        // Multi-token degenerate payloads.
        "a = 1 2\n",
        "a = [1] x\n",
        "a = [1]]\n",
        "a = [1, 2 [3]]\n",
        "a = [[1][2], 3]\n",
        "a = [[1] 2, 3]\n",
        "a = [1 2]\n",
        "a = [1 { , 2]\n",
        "a = [=, 2]\n",
        "a = [}]\n",
        "a = [a=b]\n",
        "a = \"x\" y\n",
        "a = \"\"\"x\"\"\" y\n",
        "a = =1\n",
        // Quote-run corners around the multiline closing delimiter.
        "a = \"\"\"\"\"\"\n",
        "a = \"\"\"\"\"\"\"\n",
        "a = \"\"\"x\"\"\"\"\n",
        "a = \"\"\"x\"\"\"\"\"\n",
        "a = \"\"\"x\"\"\"\"\"\"\n",
        "a = [\"\"\"a\"\"\"\"\"\"x\"]\n",
        // Unterminated strings.
        "a = [\"x, 1]\n",
        "a = [ \"unterm\n  1, 2 ]\n",
        "a = [\"a\rb\",\"c\"]\n",
        "a = [ \"a\r\" ]\n",
        "\"unterm = 1\n",
        "\"a.b = 1\n",
        "\"a\rb.c = 1\n",
        // Lone carriage returns inside comments and bare runs.
        "a = [1, #c\r2]\n",
        "a = [1 #c\n, 2]\n",
        "a = [1 #c\n 2, 3]\n",
        "a = [1#c]\nb = 2\n",
        // Unicode whitespace that only `str::trim` removes.
        "a = \u{a0}[1]\n",
        "a = [\u{a0}[1], 2]\n",
        "a = [1\u{a0}, 2]\n",
        "a = [\u{a0}]\n",
        "key = { \u{a0}x = 1 }\n",
        // Inline-table recovery corners.
        "a = {x = 1 = 2}\n",
        "a = {x = ], y = 1}\n",
        "a = { x }\n",
        "a = {,}\n",
        "a = {. = 1}\n",
        "a = { # c\n x = 1 }\n",
        "a = { x # c\n = 1 }\n",
        "a = {x = [1}\n",
        "a = {x = [1] 2}\n",
        "a = {x = a=b}\n",
        "a = {x = }\n",
        "key = {0 \"\r, \u{a0}x = }\n",
        "A = {0 [[''], A = }\n",
        "- = {0 = {0 = ,{, 0 = }}\n",
        "key = {a[b = 1, c = 2}\n",
        "A = [[{]},]]\n",
        "\u{a0}x = [[{\"unterm = , - = \"\"\"]},]]\n",
        "a = [{x = ]}, 2]\n",
        "a = [[{x = ]}], 2]\n",
        // Unclosed collections.
        "a = [1, 2\n",
        "a = {x = 1\n",
        "a = [\n",
        "a = {\n",
        "a = [{]}\n",
        // Key-path corners.
        "\"a.b\" = 1\n",
        "a[b.c] = 1\n",
        "[a[b]\n",
        "[\"a.b]\nx = 1\n",
        "a . b = 1\n",
        "a,b = 1\n",
        ". = 1\n",
        ".. = 1\n",
        "[a=b]\n",
        "[a.'b.c'.d]\n",
        // Statement-shape corners.
        "x =\n",
        "= 1\n",
        "a\n",
        "\u{feff}a = 1\n",
        "[a.b]\nc = 1\n",
        "[[a]]\nb = [1, {x = 2}]\n",
    ];

    #[test]
    fn edge_case_corpus_upholds_the_invariants() {
        for source in EDGE_CASES {
            assert_invariants(source);
        }
    }

    #[test]
    fn nested_collections_around_the_depth_limit_uphold_the_invariants() {
        for depth in [1, 2, 255, 256, 257, 258, 300] {
            assert_invariants(&format!(
                "a = {}0{}\n",
                "[".repeat(depth),
                "]".repeat(depth)
            ));
            let nested_table =
                (0..depth).fold("0".to_owned(), |inner, _| format!("{{ k = {inner} }}"));
            assert_invariants(&format!("a = {nested_table}\n"));
            assert_invariants(&format!(
                "a = {}0 # deep\n{}\n",
                "[".repeat(depth),
                "]".repeat(depth)
            ));
        }
    }

    #[test]
    fn mismatched_flat_soups_uphold_the_invariants() {
        const SOUPS: &[&str] = &[
            "[{]},]",
            "[{]},x]",
            "{[}],}",
            "[{]}]",
            "[{,}]",
            "[}]",
            "[{]}[],]",
            "[{]} ]",
            "[{]},'s']",
            "[{]}#c\n,]",
        ];
        for depth in [255, 256, 257, 258, 260] {
            for soup in SOUPS {
                let payload = format!("a = {}{}{}\n", "[".repeat(depth), soup, "]".repeat(depth));
                assert_invariants(&payload);
            }
        }
    }

    #[test]
    fn the_invalid_value_diagnostic_points_at_the_first_offending_element() {
        let source = "a = [1, oops, 2]\n";
        let parse = crate::syntax::parse(source);
        let lowered = super::lower(source, &parse.green);
        let invalid = lowered
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == DiagnosticCode::INVALID_VALUE)
            .expect("`oops` is not a value");
        let start = u32::try_from(source.find("oops").expect("literal present"))
            .expect("offset fits in u32");
        assert_eq!(invalid.range(), TextRange::new(start, start + 4));
    }

    fn bracket_soup() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                Just("[".to_owned()),
                Just("]".to_owned()),
                Just("{".to_owned()),
                Just("}".to_owned()),
                Just(",".to_owned()),
                Just("0".to_owned()),
                Just(" ".to_owned()),
                Just("=".to_owned()),
                Just("# c\n".to_owned()),
                Just("\"s\"".to_owned()),
                Just("'x,'".to_owned()),
            ],
            0..12,
        )
        .prop_map(|fragments| fragments.concat())
    }

    fn scalar_payload() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            "-?[0-9_]{1,6}",
            "0x[0-9a-fA-F_]{1,4}",
            "[+-]?(nan|inf)",
            "[0-9]{1,3}\\.[0-9]{1,3}(e[+-]?[0-9]{1,2})?",
            Just("true".to_owned()),
            Just("false".to_owned()),
            Just("1979-05-27T07:32:00Z".to_owned()),
            Just("1979-05-27 07:32z".to_owned()),
            Just("07:32".to_owned()),
            Just("01".to_owned()),
            Just("not-a-toml-value".to_owned()),
            "[a-z0-9:+._ -]{0,10}",
        ]
    }

    fn string_payload() -> impl Strategy<Value = String> {
        let content = proptest::collection::vec(
            prop_oneof![
                8 => "[a-z0-9 ]{0,4}",
                2 => Just("\\\"".to_owned()),
                2 => Just("\\\\".to_owned()),
                1 => Just("\\n".to_owned()),
                2 => Just("\"".to_owned()),
                2 => Just("\"\"".to_owned()),
                1 => Just("'".to_owned()),
                1 => Just(",".to_owned()),
                1 => Just("]".to_owned()),
                1 => Just("}".to_owned()),
                1 => Just("#".to_owned()),
                1 => Just("\r".to_owned()),
                1 => Just("\n".to_owned()),
                1 => Just("=".to_owned()),
                1 => Just(".".to_owned()),
            ],
            0..6,
        )
        .prop_map(|fragments| fragments.concat());
        (content, 0_u8..8_u8).prop_map(|(content, shape)| match shape {
            0 => format!("\"{content}\""),
            1 => format!("'{content}'"),
            2 => format!("\"\"\"{content}\"\"\""),
            3 => format!("'''{content}'''"),
            4 => format!("\"{content}"),
            5 => format!("'{content}"),
            6 => format!("\"\"\"{content}"),
            _ => format!("\"\"\"{content}\"\"\"\"\""),
        })
    }

    fn garbage_payload() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                Just("[".to_owned()),
                Just("]".to_owned()),
                Just("{".to_owned()),
                Just("}".to_owned()),
                Just(",".to_owned()),
                Just("=".to_owned()),
                Just(".".to_owned()),
                Just("\"".to_owned()),
                Just("'".to_owned()),
                Just("#".to_owned()),
                Just(" ".to_owned()),
                Just("\t".to_owned()),
                Just("\r".to_owned()),
                Just("\n".to_owned()),
                Just("\\".to_owned()),
                Just("\u{a0}".to_owned()),
                "[a-z0-9]{1,3}",
            ],
            0..12,
        )
        .prop_map(|fragments| fragments.concat())
    }

    fn key_payload() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-zA-Z0-9_-]{1,6}",
            "[a-z]{1,3}\\.[a-z]{1,3}",
            "\"[a-z.# ]{0,5}\"",
            "'[a-z.]{0,5}'",
            Just("a[b.c]".to_owned()),
            Just("a b".to_owned()),
            Just(".".to_owned()),
            Just(String::new()),
            Just("\"unterm".to_owned()),
            Just("\u{a0}x".to_owned()),
        ]
    }

    fn value_payload() -> impl Strategy<Value = String> {
        let leaf = prop_oneof![
            4 => scalar_payload(),
            3 => string_payload(),
            2 => garbage_payload(),
        ];
        leaf.prop_recursive(5, 48, 6, |inner| {
            let separator = prop_oneof![
                4 => Just(", ".to_owned()),
                1 => Just(" ,".to_owned()),
                1 => Just(",\n  ".to_owned()),
                1 => Just(", # note\n ".to_owned()),
                1 => Just(",\r\n".to_owned()),
                1 => Just(",".to_owned()),
            ];
            let array = (
                proptest::collection::vec(inner.clone(), 0..4),
                separator.clone(),
                proptest::bool::ANY,
            )
                .prop_map(|(items, separator, trailing)| {
                    let mut body = items.join(&separator);
                    if trailing && !body.is_empty() {
                        body.push(',');
                    }
                    format!("[{body}]")
                });
            let inline = (
                proptest::collection::vec(
                    (
                        key_payload(),
                        prop_oneof![9 => Just(" = "), 1 => Just(" ")],
                        inner,
                    ),
                    0..4,
                ),
                separator,
            )
                .prop_map(|(entries, separator)| {
                    let body = entries
                        .iter()
                        .map(|(key, equals, value)| format!("{key}{equals}{value}"))
                        .collect::<Vec<_>>()
                        .join(&separator);
                    format!("{{{body}}}")
                });
            prop_oneof![3 => array, 2 => inline]
        })
    }

    fn statement() -> impl Strategy<Value = String> {
        prop_oneof![
            6 => (key_payload(), value_payload())
                .prop_map(|(key, value)| format!("{key} = {value}")),
            2 => (key_payload(), scalar_payload())
                .prop_map(|(key, value)| format!("{key} = {value}")),
            1 => key_payload().prop_map(|key| format!("[{key}]")),
            1 => key_payload().prop_map(|key| format!("[[{key}]]")),
            1 => Just("# comment".to_owned()),
            1 => garbage_payload(),
        ]
    }

    fn document() -> impl Strategy<Value = String> {
        (
            proptest::collection::vec(statement(), 0..6),
            prop_oneof![3 => Just("\n"), 1 => Just("\r\n")],
            proptest::bool::ANY,
        )
            .prop_map(|(lines, newline, trailing)| {
                let mut text = lines.join(newline);
                if trailing {
                    text.push_str(newline);
                }
                text
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn generated_value_payloads_uphold_the_invariants(payload in value_payload()) {
            assert_invariants(&format!("key = {payload}\n"));
        }

        #[test]
        fn generated_documents_uphold_the_invariants(source in document()) {
            assert_invariants(&source);
        }

        #[test]
        fn arbitrary_utf8_documents_uphold_the_invariants(source in any::<String>()) {
            assert_invariants(&source);
        }

        #[test]
        fn deep_mismatched_soups_uphold_the_invariants(
            depth in 250_usize..=260,
            soup in bracket_soup(),
        ) {
            let payload = format!("a = {}{}{}\n", "[".repeat(depth), soup, "]".repeat(depth));
            assert_invariants(&payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(path: &[&str]) -> Vec<Arc<str>> {
        path.iter().map(|segment| Arc::from(*segment)).collect()
    }

    #[test]
    fn activating_array_scope_replaces_the_same_path_and_its_descendants() {
        let records = segments(&["records"]);
        let mut active = vec![
            (segments(&["unrelated"]), 7),
            (records.clone(), 1),
            (segments(&["records", "details"]), 2),
        ];

        activate_array_scope(&mut active, &records, 3);

        assert_eq!(active, vec![(segments(&["unrelated"]), 7), (records, 3)]);
        assert_eq!(
            enclosing_array_scope(&segments(&["records", "name"]), &active),
            Some(3)
        );
    }

    #[test]
    fn conflict_index_matches_the_quadratic_declaration_contract() {
        let paths = [
            &["a"][..],
            &["a", "b"],
            &["a", "b", "c"],
            &["b"],
            &["b", "a"],
        ];
        let kinds = [
            DeclarationKind::KeyValue,
            DeclarationKind::Table,
            DeclarationKind::ArrayTable,
        ];
        let declarations = [0, 1]
            .into_iter()
            .flat_map(|scope| {
                paths.into_iter().flat_map(move |path| {
                    kinds
                        .into_iter()
                        .map(move |kind| test_declaration(path, kind, scope))
                })
            })
            .collect::<Vec<_>>();

        for first in &declarations {
            for second in &declarations {
                let mut index = ConflictIndex::default();
                index.insert(first, 0);
                index.insert(second, 1);
                for current in &declarations {
                    let expected = [first, second].into_iter().position(|previous| {
                        previous.scope == current.scope
                            && declaration_conflict_code(previous, current).is_some()
                    });
                    assert_eq!(
                        index.first_conflict(current),
                        expected,
                        "first={first:?}, second={second:?}, current={current:?}"
                    );
                }
            }
        }
    }

    fn test_declaration(path: &[&str], kind: DeclarationKind, scope: u32) -> Declaration {
        Declaration {
            key: KeyPath::new(segments(path)),
            kind,
            value: None,
            range: TextRange::default(),
            first_invalid_range: None,
            scope,
            element_scope: None,
            promotes_implicit_table: false,
        }
    }
}
