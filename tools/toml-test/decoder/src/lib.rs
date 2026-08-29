use std::fmt;

use serde_json::{Map, Value, json};
use tomlsmith::{DateTimeKind, Document, SemanticTable, SemanticValue, Severity, TomlVersion};

#[derive(Debug)]
pub struct DecodeError(String);

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

/// Decodes one TOML document into the tagged JSON required by `toml-test`.
///
/// # Errors
///
/// Returns an error when `TomlSmith` reports an invalid document or when its
/// semantic value cannot be represented by the decoder protocol.
pub fn decode(source: &str, version: TomlVersion) -> Result<Value, DecodeError> {
    let document = Document::parse_as(source, version);
    let errors = document
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.severity() == Severity::Error)
        .map(|diagnostic| format!("{}: {}", diagnostic.code(), diagnostic.message()))
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(DecodeError(errors.join("\n")));
    }

    tagged_table(document.semantics().root())
}

fn tagged_table(table: &SemanticTable) -> Result<Value, DecodeError> {
    table
        .entries()
        .iter()
        .map(|(key, value)| Ok((key.to_string(), tagged_value(value)?)))
        .collect::<Result<Map<_, _>, _>>()
        .map(Value::Object)
}

fn insert_value(output: &mut Value, path: &[&str], value: Value) -> Result<(), DecodeError> {
    let (key, parent_path) = path
        .split_last()
        .ok_or_else(|| DecodeError("value has an empty key path".to_owned()))?;
    let parent = table_at_path(output, parent_path)?;
    if parent.insert((*key).to_owned(), value).is_some() {
        return Err(DecodeError(format!(
            "value path `{}` was already populated",
            path.join(".")
        )));
    }
    Ok(())
}

fn table_at_path<'output>(
    output: &'output mut Value,
    path: &[&str],
) -> Result<&'output mut Map<String, Value>, DecodeError> {
    let mut cursor = output;
    for segment in path {
        let table = current_table(cursor)?;
        cursor = table
            .entry((*segment).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    current_table(cursor)
}

fn current_table(value: &mut Value) -> Result<&mut Map<String, Value>, DecodeError> {
    match value {
        Value::Object(table) => Ok(table),
        Value::Array(elements) => {
            let current = elements
                .last_mut()
                .ok_or_else(|| DecodeError("array table has no current element".to_owned()))?;
            current_table(current)
        }
        _ => Err(DecodeError(
            "a table path traverses a scalar value".to_owned(),
        )),
    }
}

fn tagged_value(value: &SemanticValue) -> Result<Value, DecodeError> {
    match value {
        SemanticValue::String(value) => Ok(json!({"type": "string", "value": value.as_ref()})),
        SemanticValue::Integer(value) => Ok(json!({"type": "integer", "value": value.to_string()})),
        SemanticValue::Float(value) => Ok(json!({"type": "float", "value": value.to_string()})),
        SemanticValue::Boolean(value) => Ok(json!({"type": "bool", "value": value.to_string()})),
        SemanticValue::DateTime(value) => {
            let kind = match value.kind() {
                DateTimeKind::OffsetDateTime => "datetime",
                DateTimeKind::LocalDateTime => "datetime-local",
                DateTimeKind::LocalDate => "date-local",
                DateTimeKind::LocalTime => "time-local",
            };
            Ok(json!({"type": kind, "value": value.canonical()}))
        }
        SemanticValue::Array(values) => values
            .iter()
            .map(tagged_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        SemanticValue::InlineTable(entries) => {
            let mut table = Value::Object(Map::new());
            for (key, value) in entries.iter() {
                let path = key.segments().collect::<Vec<_>>();
                insert_value(&mut table, &path, tagged_value(value)?)?;
            }
            Ok(table)
        }
        SemanticValue::Table(table) => tagged_table(table),
        SemanticValue::Invalid(raw) => Err(DecodeError(format!(
            "invalid semantic value `{raw}` reached the decoder"
        ))),
    }
}
