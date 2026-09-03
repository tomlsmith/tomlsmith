//! Helpers shared by the formatter invariant suites.

use tomlsmith::{SemanticTable, SemanticValue};

/// Structural equality of decoded roots where every NaN equals every other NaN.
///
/// `SemanticValue::Float` follows IEEE 754, so a document containing `nan`
/// would never compare equal to itself; the formatter contract is spelling
/// preservation, which this comparison checks through the decoded value.
#[must_use]
pub fn semantic_roots_equal(left: &SemanticTable, right: &SemanticTable) -> bool {
    left.entries().len() == right.entries().len()
        && left.entries().iter().zip(right.entries()).all(
            |((left_key, left_value), (right_key, right_value))| {
                left_key == right_key && semantic_values_equal(left_value, right_value)
            },
        )
}

fn semantic_values_equal(left: &SemanticValue, right: &SemanticValue) -> bool {
    match (left, right) {
        (SemanticValue::Float(left), SemanticValue::Float(right)) => {
            (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
        }
        (SemanticValue::Array(left), SemanticValue::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| semantic_values_equal(left, right))
        }
        (SemanticValue::InlineTable(left), SemanticValue::InlineTable(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|((left_key, left), (right_key, right))| {
                        left_key == right_key && semantic_values_equal(left, right)
                    })
        }
        (SemanticValue::Table(left), SemanticValue::Table(right)) => {
            semantic_roots_equal(left, right)
        }
        (left, right) => left == right,
    }
}
