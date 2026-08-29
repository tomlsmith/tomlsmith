use std::{collections::HashMap, fmt, sync::Arc};

use crate::{
    Diagnostic, DiagnosticCode, TextRange,
    literal::{self, LiteralValue},
    syntax::{
        self,
        ast::{
            ArrayTable as AstArrayTable, AstNode, KeyValue as AstKeyValue, Root as AstRoot,
            Statement, Table as AstTable,
        },
    },
};

pub(crate) const MAX_KEY_DEPTH: usize = 256;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KeyPath(Arc<[Arc<str>]>);

impl KeyPath {
    fn new(segments: Vec<String>) -> Self {
        Self(
            segments
                .into_iter()
                .map(Arc::<str>::from)
                .collect::<Vec<_>>()
                .into(),
        )
    }

    pub fn segments(&self) -> impl ExactSizeIterator<Item = &str> {
        self.0.iter().map(AsRef::as_ref)
    }

    #[must_use]
    pub fn dotted(&self) -> String {
        self.segments().collect::<Vec<_>>().join(".")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DeclarationKind {
    KeyValue,
    Table,
    ArrayTable,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DateTimeKind {
    OffsetDateTime,
    LocalDateTime,
    LocalDate,
    LocalTime,
}

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

    #[must_use]
    pub const fn kind(&self) -> DateTimeKind {
        self.kind
    }

    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SemanticValue {
    String(Arc<str>),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    DateTime(DateTimeValue),
    Array(Arc<[Self]>),
    InlineTable(Arc<[(KeyPath, Self)]>),
    Table(SemanticTable),
    Invalid(Arc<str>),
}

impl SemanticValue {
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_inline_table(&self) -> Option<&[(KeyPath, Self)]> {
        match self {
            Self::InlineTable(entries) => Some(entries),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_table(&self) -> Option<&SemanticTable> {
        match self {
            Self::Table(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_datetime(&self) -> Option<&DateTimeValue> {
        match self {
            Self::DateTime(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticTable(Arc<[(Arc<str>, SemanticValue)]>);

impl SemanticTable {
    #[must_use]
    pub fn entries(&self) -> &[(Arc<str>, SemanticValue)] {
        &self.0
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&SemanticValue> {
        self.0
            .iter()
            .find_map(|(candidate, value)| (candidate.as_ref() == key).then_some(value))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    key: KeyPath,
    kind: DeclarationKind,
    value: Option<SemanticValue>,
    range: TextRange,
    scope: u32,
    element_scope: Option<u32>,
    promotes_implicit_table: bool,
}

impl Declaration {
    #[must_use]
    pub const fn key(&self) -> &KeyPath {
        &self.key
    }

    #[must_use]
    pub const fn kind(&self) -> DeclarationKind {
        self.kind
    }

    #[must_use]
    pub const fn value(&self) -> Option<&SemanticValue> {
        self.value.as_ref()
    }

    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }
}

#[derive(Clone)]
pub struct SemanticDocument {
    declarations: Arc<[Declaration]>,
    index: Arc<HashMap<Vec<String>, Vec<usize>>>,
    root: SemanticTable,
}

impl SemanticDocument {
    #[must_use]
    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    #[must_use]
    pub const fn root(&self) -> &SemanticTable {
        &self.root
    }

    pub fn resolve<I, S>(&self, segments: I) -> Resolution<'_>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let key = segments
            .into_iter()
            .map(|segment| segment.as_ref().to_owned())
            .collect::<Vec<_>>();
        let Some(indices) = self.index.get(&key) else {
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

#[derive(Debug)]
pub enum Resolution<'document> {
    Missing,
    Unique(&'document Declaration),
    Ambiguous(Vec<&'document Declaration>),
}

pub(crate) struct Lowered {
    pub(crate) document: SemanticDocument,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn lower(source: &str, green: rowan::GreenNode) -> Lowered {
    let root = AstRoot::cast(syntax::root(green)).expect("parser always produces a root node");
    let mut state = LoweringState::new(source);
    for statement in root.statements() {
        state.lower_statement(statement);
    }
    state.finish()
}

struct LoweringState<'source> {
    source: &'source str,
    current_table: Vec<String>,
    discard_current_table_entries: bool,
    current_scope: u32,
    next_scope: u32,
    active_array_tables: Vec<(Vec<String>, u32)>,
    explicit_tables: Vec<(Vec<String>, u32)>,
    implicit_table_paths: Vec<(Vec<String>, u32)>,
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
            explicit_tables: Vec::new(),
            implicit_table_paths: Vec::new(),
            declarations: Vec::new(),
            namespace_diagnostics: Vec::new(),
        }
    }

    fn lower_statement(&mut self, statement: Statement) {
        match statement {
            Statement::Table(node) => self.lower_table(&node),
            Statement::ArrayTable(node) => self.lower_array_table(&node),
            Statement::KeyValue(node) => self.lower_key_value_statement(&node),
        }
    }

    fn lower_table(&mut self, node: &AstTable) {
        let path = node
            .key()
            .map(|key| parse_key_path(source_slice(self.source, key.syntax().range())))
            .unwrap_or_default();
        if !self.select_table_path(path) {
            return;
        }

        let scope =
            enclosing_array_scope(&self.current_table, &self.active_array_tables).unwrap_or(0);
        self.current_scope = scope;
        let promotes_implicit_table = self.promote_implicit_table();
        self.record_implicit_parents();
        self.explicit_tables
            .push((self.current_table.clone(), scope));
        self.declarations.push(Declaration {
            key: KeyPath::new(self.current_table.clone()),
            kind: DeclarationKind::Table,
            value: None,
            range: node.syntax().range(),
            scope,
            element_scope: None,
            promotes_implicit_table,
        });
    }

    fn lower_array_table(&mut self, node: &AstArrayTable) {
        let path = node
            .key()
            .map(|key| parse_key_path(source_slice(self.source, key.syntax().range())))
            .unwrap_or_default();
        if !self.select_table_path(path) {
            return;
        }

        let scope =
            enclosing_array_scope(&self.current_table, &self.active_array_tables).unwrap_or(0);
        if self
            .implicit_table_paths
            .iter()
            .any(|(path, owner)| path == &self.current_table && *owner == scope)
        {
            self.namespace_diagnostics.push(Diagnostic::error(
                DiagnosticCode::CONFLICTING_KEY,
                format!(
                    "array-of-tables conflicts with the implicitly created table `{}`",
                    self.current_table.join(".")
                ),
                node.syntax().range(),
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
        self.declarations.push(Declaration {
            key: KeyPath::new(self.current_table.clone()),
            kind: DeclarationKind::ArrayTable,
            value: None,
            range: node.syntax().range(),
            scope,
            element_scope: Some(element_scope),
            promotes_implicit_table: false,
        });
    }

    fn lower_key_value_statement(&mut self, node: &AstKeyValue) {
        if self.discard_current_table_entries {
            return;
        }
        let relative_depth = node
            .key()
            .map(|key| parse_key_path(source_slice(self.source, key.syntax().range())).len())
            .unwrap_or_default();
        if relative_depth > MAX_KEY_DEPTH {
            return;
        }
        if self.current_table.len().saturating_add(relative_depth) > MAX_KEY_DEPTH {
            self.namespace_diagnostics.push(Diagnostic::error(
                DiagnosticCode::NESTING_LIMIT,
                format!("key nesting exceeds the supported limit of {MAX_KEY_DEPTH}"),
                node.syntax().range(),
            ));
            return;
        }
        let declaration_start = self.declarations.len();
        lower_key_value(
            self.source,
            node,
            &self.current_table,
            self.current_scope,
            &mut self.declarations,
        );
        let Some(declaration) = self.declarations.get(declaration_start).cloned() else {
            return;
        };
        let key = declaration
            .key
            .segments()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        self.diagnose_array_table_extension(&declaration, &key);
        self.diagnose_cross_header_dotted_key(&declaration, &key);
    }

    fn select_table_path(&mut self, path: Vec<String>) -> bool {
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
        let Some(index) = self
            .implicit_table_paths
            .iter()
            .position(|(path, owner)| path == &self.current_table && *owner == self.current_scope)
        else {
            return false;
        };
        self.implicit_table_paths.swap_remove(index);
        true
    }

    fn record_implicit_parents(&mut self) {
        for prefix_length in 1..self.current_table.len() {
            let prefix = self.current_table[..prefix_length].to_vec();
            let scope = enclosing_array_scope(&prefix, &self.active_array_tables).unwrap_or(0);
            let explicitly_known = self.declarations.iter().any(|declaration| {
                declaration.scope == scope
                    && declaration
                        .key
                        .segments()
                        .eq(prefix.iter().map(String::as_str))
            });
            let already_implicit = self
                .implicit_table_paths
                .iter()
                .any(|(path, owner)| path == &prefix && *owner == scope);
            if !explicitly_known && !already_implicit {
                self.implicit_table_paths.push((prefix, scope));
            }
        }
    }

    fn diagnose_array_table_extension(&mut self, declaration: &Declaration, key: &[String]) {
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

    fn diagnose_cross_header_dotted_key(&mut self, declaration: &Declaration, key: &[String]) {
        let crosses_header = self.explicit_tables.iter().any(|(explicit, scope)| {
            *scope == declaration.scope
                && is_prefix(&self.current_table, explicit)
                && is_prefix(explicit, key)
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
        let (index, mut diagnostics) = index_declarations(&self.declarations);
        diagnostics.extend(self.namespace_diagnostics);
        let root = build_semantic_root(&self.declarations);

        Lowered {
            document: SemanticDocument {
                declarations: self.declarations.into(),
                index: Arc::new(index),
                root,
            },
            diagnostics,
        }
    }
}

fn index_declarations(
    declarations: &[Declaration],
) -> (HashMap<Vec<String>, Vec<usize>>, Vec<Diagnostic>) {
    let mut index: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
    let mut conflict_index = ConflictIndex::default();
    let mut diagnostics = Vec::new();
    for (declaration_index, declaration) in declarations.iter().enumerate() {
        if let Some(value) = declaration.value.as_ref() {
            collect_inline_conflicts(value, declaration.range, &mut diagnostics);
            if contains_invalid(value) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::INVALID_VALUE,
                    format!("invalid value for `{}`", declaration.key.dotted()),
                    declaration.range,
                ));
            }
        }
        let key = declaration
            .key
            .segments()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !declaration.promotes_implicit_table {
            if let Some(previous_index) = conflict_index.first_conflict(declaration) {
                let previous = &declarations[previous_index];
                let code = declaration_conflict_code(previous, declaration)
                    .expect("the conflict index only returns conflicting declarations");
                diagnostics.push(Diagnostic::error(
                    code,
                    format!("conflicting declaration for `{}`", declaration.key.dotted()),
                    declaration.range,
                ));
            }
        }
        conflict_index.insert(declaration, declaration_index);
        let matches = index.entry(key).or_default();
        matches.push(declaration_index);
    }
    (index, diagnostics)
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
    scopes: HashMap<u32, ConflictNode>,
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
    children: HashMap<Arc<str>, Self>,
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
            for (index, (key, value)) in entries.iter().enumerate() {
                let current = key.segments().collect::<Vec<_>>();
                for (previous, _) in &entries[..index] {
                    let previous = previous.segments().collect::<Vec<_>>();
                    let code = if previous == current {
                        Some(DiagnosticCode::DUPLICATE_KEY)
                    } else if is_prefix(&previous, &current) || is_prefix(&current, &previous) {
                        Some(DiagnosticCode::CONFLICTING_KEY)
                    } else {
                        None
                    };
                    if let Some(code) = code {
                        diagnostics.push(Diagnostic::error(
                            code,
                            format!("conflicting inline-table entry for `{}`", key.dotted()),
                            range,
                        ));
                        break;
                    }
                }
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
    node: &AstKeyValue,
    current_table: &[String],
    scope: u32,
    declarations: &mut Vec<Declaration>,
) {
    let Some(key_node) = node.key() else {
        return;
    };
    let Some(value_node) = node.value() else {
        return;
    };
    let mut key = current_table.to_vec();
    key.extend(parse_key_path(source_slice(
        source,
        key_node.syntax().range(),
    )));
    if key.len() == current_table.len() {
        return;
    }
    let raw_value = source_slice(source, value_node.syntax().range()).trim();
    declarations.push(Declaration {
        key: KeyPath::new(key),
        kind: DeclarationKind::KeyValue,
        value: Some(parse_value(raw_value)),
        range: node.syntax().range(),
        scope,
        element_scope: None,
        promotes_implicit_table: false,
    });
}

fn enclosing_array_scope(path: &[String], active: &[(Vec<String>, u32)]) -> Option<u32> {
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

fn retire_array_scopes_at_or_below(active: &mut Vec<(Vec<String>, u32)>, path: &[String]) {
    active.retain(|(candidate, _)| {
        candidate.as_slice() != path && !is_prefix(path, candidate.as_slice())
    });
}

fn activate_array_scope(active: &mut Vec<(Vec<String>, u32)>, path: &[String], scope: u32) {
    retire_array_scopes_at_or_below(active, path);
    active.push((path.to_vec(), scope));
}

#[derive(Default)]
struct MutableTable {
    entries: Vec<(String, MutableEntry)>,
    indices: HashMap<String, usize>,
}

enum MutableEntry {
    Value(SemanticValue),
    Table(MutableTable),
    ArrayTables(Vec<MutableTable>),
}

impl MutableTable {
    fn entry_mut(&mut self, key: &str) -> Option<&mut MutableEntry> {
        let index = *self.indices.get(key)?;
        Some(&mut self.entries[index].1)
    }

    fn insert(&mut self, key: String, entry: MutableEntry) -> bool {
        if self.indices.contains_key(&key) {
            return false;
        }
        let index = self.entries.len();
        self.indices.insert(key.clone(), index);
        self.entries.push((key, entry));
        true
    }
}

#[derive(Clone)]
enum LocationStep {
    Table(String),
    ArrayElement { key: String, index: usize },
}

#[derive(Clone)]
struct ScopeContext {
    logical_path: Vec<String>,
    location: Vec<LocationStep>,
}

fn build_semantic_root(declarations: &[Declaration]) -> SemanticTable {
    let mut root = MutableTable::default();
    let mut scopes = HashMap::from([(
        0_u32,
        ScopeContext {
            logical_path: Vec::new(),
            location: Vec::new(),
        },
    )]);

    for declaration in declarations {
        let Some(context) = scopes.get(&declaration.scope).cloned() else {
            continue;
        };
        let path = declaration
            .key
            .segments()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let relative = path
            .strip_prefix(context.logical_path.as_slice())
            .unwrap_or(path.as_slice());
        let Some(table) = table_at_location_mut(&mut root, &context.location) else {
            continue;
        };

        match declaration.kind {
            DeclarationKind::Table => {
                ensure_table_path(table, relative);
            }
            DeclarationKind::ArrayTable => {
                let Some(element_scope) = declaration.element_scope else {
                    continue;
                };
                if let Some(relative_location) = append_array_table(table, relative) {
                    let mut location = context.location;
                    location.extend(relative_location);
                    scopes.insert(
                        element_scope,
                        ScopeContext {
                            logical_path: path,
                            location,
                        },
                    );
                }
            }
            DeclarationKind::KeyValue => {
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

fn ensure_table_path(table: &mut MutableTable, path: &[String]) -> bool {
    let Some((key, remainder)) = path.split_first() else {
        return true;
    };
    if !table.indices.contains_key(key) {
        table.insert(key.clone(), MutableEntry::Table(MutableTable::default()));
    }
    let Some(MutableEntry::Table(child)) = table.entry_mut(key) else {
        return false;
    };
    ensure_table_path(child, remainder)
}

fn append_array_table(table: &mut MutableTable, path: &[String]) -> Option<Vec<LocationStep>> {
    let (key, parents) = path.split_last()?;
    let mut current = table;
    let mut location = Vec::with_capacity(path.len());

    for parent in parents {
        if !current.indices.contains_key(parent) {
            current.insert(parent.clone(), MutableEntry::Table(MutableTable::default()));
        }
        let MutableEntry::Table(child) = current.entry_mut(parent)? else {
            return None;
        };
        current = child;
        location.push(LocationStep::Table(parent.clone()));
    }

    if !current.indices.contains_key(key) {
        current.insert(key.clone(), MutableEntry::ArrayTables(Vec::new()));
    }
    let MutableEntry::ArrayTables(elements) = current.entry_mut(key)? else {
        return None;
    };
    let index = elements.len();
    elements.push(MutableTable::default());
    location.push(LocationStep::ArrayElement {
        key: key.clone(),
        index,
    });
    Some(location)
}

fn insert_value(table: &mut MutableTable, path: &[String], value: SemanticValue) -> bool {
    let Some((key, remainder)) = path.split_first() else {
        return false;
    };
    if remainder.is_empty() {
        return table.insert(key.clone(), MutableEntry::Value(value));
    }
    if !table.indices.contains_key(key) {
        table.insert(key.clone(), MutableEntry::Table(MutableTable::default()));
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
                (Arc::<str>::from(key), value)
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

fn parse_key_path(raw: &str) -> Vec<String> {
    split_top_level(raw.trim(), '.')
        .into_iter()
        .filter_map(|segment| {
            let segment = segment.trim();
            if segment.is_empty() {
                None
            } else {
                Some(decode_key(segment))
            }
        })
        .collect()
}

fn decode_key(raw: &str) -> String {
    if matches!(raw.as_bytes().first(), Some(b'"' | b'\'')) {
        if let Some(parsed) = literal::parse(raw) {
            if let LiteralValue::String(value) = parsed.value {
                return value;
            }
        }
    }
    raw.to_owned()
}

fn parse_value(raw: &str) -> SemanticValue {
    parse_value_at(raw, 0)
}

const MAX_VALUE_DEPTH: usize = 256;

fn parse_value_at(raw: &str, depth: usize) -> SemanticValue {
    let raw = raw.trim();
    if raw.starts_with('[') && raw.ends_with(']') {
        if depth >= MAX_VALUE_DEPTH {
            return SemanticValue::Invalid(raw.into());
        }
        let values = split_top_level(&raw[1..raw.len() - 1], ',')
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty() && !value.starts_with('#'))
            .map(|value| parse_value_at(&value, depth + 1))
            .collect::<Vec<_>>();
        return SemanticValue::Array(values.into());
    }
    if raw.starts_with('{') && raw.ends_with('}') {
        if depth >= MAX_VALUE_DEPTH {
            return SemanticValue::Invalid(raw.into());
        }
        let entries = split_top_level(&raw[1..raw.len() - 1], ',')
            .into_iter()
            .filter_map(|entry| {
                let (key, value) = split_once_top_level(&entry, '=')?;
                Some((
                    KeyPath::new(parse_key_path(&key)),
                    parse_value_at(&value, depth + 1),
                ))
            })
            .collect::<Vec<_>>();
        return SemanticValue::InlineTable(entries.into());
    }
    if let Some(parsed) = literal::parse(raw) {
        return match parsed.value {
            LiteralValue::String(value) => SemanticValue::String(value.into()),
            LiteralValue::Integer(value) => SemanticValue::Integer(value),
            LiteralValue::Float(value) => SemanticValue::Float(value),
            LiteralValue::Boolean(value) => SemanticValue::Boolean(value),
            LiteralValue::DateTime => SemanticValue::DateTime(DateTimeValue::from_raw(raw)),
        };
    }
    SemanticValue::Invalid(raw.into())
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

fn split_top_level(input: &str, separator: char) -> Vec<String> {
    debug_assert!(separator.is_ascii());
    let separator = separator as u8;
    let mut parts = Vec::new();
    let mut part = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut cursor = 0;
    let mut square_depth = 0_usize;
    let mut brace_depth = 0_usize;
    let mut string = None;
    let mut comment = false;

    while cursor < bytes.len() {
        if comment {
            match bytes[cursor] {
                b'\n' => {
                    part.push('\n');
                    cursor += 1;
                    comment = false;
                }
                b'\r' if bytes.get(cursor + 1) == Some(&b'\n') => {
                    part.push_str("\r\n");
                    cursor += 2;
                    comment = false;
                }
                _ => cursor += input[cursor..].chars().next().map_or(1, char::len_utf8),
            }
            continue;
        }

        if let Some(state) = string {
            let (next, closed) = advance_string(input, cursor, state);
            part.push_str(&input[cursor..next]);
            cursor = next;
            if closed {
                string = None;
            }
            continue;
        }

        if let Some((state, next)) = string_start(bytes, cursor) {
            string = Some(state);
            part.push_str(&input[cursor..next]);
            cursor = next;
            continue;
        }

        match bytes[cursor] {
            b'#' => {
                comment = true;
                cursor += 1;
            }
            b'[' => {
                square_depth += 1;
                part.push('[');
                cursor += 1;
            }
            b']' => {
                square_depth = square_depth.saturating_sub(1);
                part.push(']');
                cursor += 1;
            }
            b'{' => {
                brace_depth += 1;
                part.push('{');
                cursor += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                part.push('}');
                cursor += 1;
            }
            byte if byte == separator && square_depth == 0 && brace_depth == 0 => {
                parts.push(std::mem::take(&mut part));
                cursor += 1;
            }
            _ => {
                let length = input[cursor..].chars().next().map_or(1, char::len_utf8);
                part.push_str(&input[cursor..cursor + length]);
                cursor += length;
            }
        }
    }
    parts.push(part);
    parts
}

fn split_once_top_level(input: &str, separator: char) -> Option<(String, String)> {
    let mut parts = split_top_level(input, separator).into_iter();
    let key = parts.next()?;
    let first_value = parts.next()?;
    let mut value = first_value;
    for remainder in parts {
        value.push(separator);
        value.push_str(&remainder);
    }
    Some((key, value))
}

#[derive(Clone, Copy)]
struct StringScan {
    quote: u8,
    multiline: bool,
    basic: bool,
}

fn string_start(bytes: &[u8], cursor: usize) -> Option<(StringScan, usize)> {
    let quote = *bytes.get(cursor)?;
    if !matches!(quote, b'"' | b'\'') {
        return None;
    }
    let multiline = bytes.get(cursor..cursor + 3) == Some(&[quote, quote, quote]);
    Some((
        StringScan {
            quote,
            multiline,
            basic: quote == b'"',
        },
        cursor + if multiline { 3 } else { 1 },
    ))
}

fn advance_string(input: &str, cursor: usize, state: StringScan) -> (usize, bool) {
    let bytes = input.as_bytes();
    if state.basic && bytes[cursor] == b'\\' {
        let after_slash = cursor + 1;
        let escaped_length = input[after_slash..]
            .chars()
            .next()
            .map_or(0, char::len_utf8);
        return (after_slash + escaped_length, false);
    }
    if bytes[cursor] == state.quote {
        if !state.multiline {
            return (cursor + 1, true);
        }
        let run = bytes[cursor..]
            .iter()
            .take_while(|&&byte| byte == state.quote)
            .count();
        return (cursor + run, run >= 3);
    }
    let length = input[cursor..].chars().next().map_or(1, char::len_utf8);
    (cursor + length, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activating_array_scope_replaces_the_same_path_and_its_descendants() {
        let records = vec!["records".to_owned()];
        let mut active = vec![
            (vec!["unrelated".to_owned()], 7),
            (records.clone(), 1),
            (vec!["records".to_owned(), "details".to_owned()], 2),
        ];

        activate_array_scope(&mut active, &records, 3);

        assert_eq!(
            active,
            vec![(vec!["unrelated".to_owned()], 7), (records.clone(), 3)]
        );
        assert_eq!(
            enclosing_array_scope(&["records".to_owned(), "name".to_owned()], &active),
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
            key: KeyPath::new(path.iter().map(|segment| (*segment).to_owned()).collect()),
            kind,
            value: None,
            range: TextRange::default(),
            scope,
            element_scope: None,
            promotes_implicit_table: false,
        }
    }
}
