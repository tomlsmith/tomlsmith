//! Deterministic complexity and memory waterlines for the core.
//!
//! Each growth case runs an operation on an input and on the same input
//! scaled by 8x and asserts that allocator calls and bytes grow by at most
//! 12x (linear is 8x; a quadratic implementation reaches 64x). Each memory
//! case asserts that the peak live heap of an operation stays within a fixed
//! number of bytes per input byte. Both are properties of the code, not of
//! the machine, so they run on every CI operating system without slack for
//! runner speed.

// Ratios of small counts: precision loss in the conversion is irrelevant.
#![allow(clippy::cast_precision_loss)]

use std::{fmt::Write, hint::black_box};

use tomlsmith::{Document, FormatOptions, FormatOutcome, TomlVersion};
use tomlsmith_complexity_guards::{Allocations, measure};

const GROWTH_FACTOR: usize = 8;
const GROWTH_WATERLINE: usize = 12;

fn inline_tables(count: usize) -> String {
    let mut source = String::new();
    for index in 0..count {
        let _ = writeln!(source, "item_{index}={{left={index},right={}}}", index + 1);
    }
    source
}

fn array_tables(count: usize) -> String {
    let mut source = String::new();
    for index in 0..count {
        let _ = writeln!(source, "[[table_{index}]]\nvalue = {index}");
    }
    source
}

fn nested_chains(chains: usize, depth: usize) -> String {
    let chain = format!("{}0{}", "{ value = ".repeat(depth), " }".repeat(depth));
    let mut source = String::new();
    for index in 0..chains {
        let _ = writeln!(source, "root_{index} = {chain}");
    }
    source
}

fn blank_lines(count: usize) -> String {
    format!("head = 1\n{}[tail]\nvalue = 2\n", "\n".repeat(count))
}

fn large_array(elements: usize) -> String {
    format!("value = [{}]\n", vec!["0"; elements].join(","))
}

fn parse(source: &str) -> Document {
    let document = Document::parse_as(source, TomlVersion::V1_1);
    assert!(
        document.diagnostics().is_empty(),
        "guard inputs must be valid: {:?}",
        document.diagnostics()
    );
    document
}

fn measure_parse(source: &str) -> Allocations {
    let (document, allocations) =
        measure(|| black_box(Document::parse_as(source, TomlVersion::V1_1)));
    assert!(document.diagnostics().is_empty());
    allocations
}

fn measure_format(document: &Document, options: &FormatOptions) -> Allocations {
    let (outcome, allocations) = measure(|| black_box(document.format_with(options)));
    assert!(!matches!(outcome, FormatOutcome::Refused { .. }));
    allocations
}

fn assert_linear(name: &str, small: Allocations, large: Allocations, failures: &mut Vec<String>) {
    let calls = large.calls as f64 / small.calls.max(1) as f64;
    let bytes = large.bytes as f64 / small.bytes.max(1) as f64;
    eprintln!(
        "[guard] {name}: {GROWTH_FACTOR}x input -> {calls:.2}x calls ({} -> {}), {bytes:.2}x bytes ({} -> {})",
        small.calls, large.calls, small.bytes, large.bytes
    );
    if calls > GROWTH_WATERLINE as f64 || bytes > GROWTH_WATERLINE as f64 {
        failures.push(format!(
            "{name}: {GROWTH_FACTOR}x input grew allocations {calls:.1}x (calls) / {bytes:.1}x (bytes); waterline {GROWTH_WATERLINE}x"
        ));
    }
}

fn assert_peak_within(
    name: &str,
    input_bytes: usize,
    allocations: Allocations,
    bytes_per_input_byte: usize,
    failures: &mut Vec<String>,
) {
    let ratio = allocations.peak_live as f64 / input_bytes as f64;
    eprintln!(
        "[guard] {name}: peak live heap {} bytes for {input_bytes} input bytes ({ratio:.1} B/B, budget {bytes_per_input_byte})",
        allocations.peak_live
    );
    if allocations.peak_live > input_bytes * bytes_per_input_byte {
        failures.push(format!(
            "{name}: peak live heap {} bytes exceeds {bytes_per_input_byte} bytes per input byte ({input_bytes} input bytes)",
            allocations.peak_live
        ));
    }
}

fn flat_options() -> FormatOptions {
    FormatOptions {
        line_width: u16::MAX,
        ..FormatOptions::default()
    }
}

fn narrow_options() -> FormatOptions {
    FormatOptions {
        line_width: 24,
        ..FormatOptions::default()
    }
}

fn nested_options() -> FormatOptions {
    FormatOptions {
        line_width: 20,
        ..FormatOptions::default()
    }
}

#[test]
fn allocations_grow_linearly_and_peak_memory_stays_within_budget() {
    let mut failures = Vec::new();
    growth_cases(&mut failures);
    memory_cases(&mut failures);
    assert!(
        failures.is_empty(),
        "{} complexity guard failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn growth_cases(failures: &mut Vec<String>) {
    let (flat, narrow, nested) = (flat_options(), narrow_options(), nested_options());

    // Growth: check.
    let small = array_tables(2_048);
    let large = array_tables(2_048 * GROWTH_FACTOR);
    assert_linear(
        "check array tables",
        measure_parse(&small),
        measure_parse(&large),
        failures,
    );

    // Growth: format, flat and expanded inline tables.
    let (small, large) = (inline_tables(512), inline_tables(512 * GROWTH_FACTOR));
    let (small_document, large_document) = (parse(&small), parse(&large));
    assert_linear(
        "format flat inline tables",
        measure_format(&small_document, &flat),
        measure_format(&large_document, &flat),
        failures,
    );
    assert_linear(
        "format expanded inline tables",
        measure_format(&small_document, &narrow),
        measure_format(&large_document, &narrow),
        failures,
    );

    // Growth: nested chains (more chains, same depth).
    let (small, large) = (nested_chains(8, 64), nested_chains(8 * GROWTH_FACTOR, 64));
    let (small_document, large_document) = (parse(&small), parse(&large));
    assert_linear(
        "format nested inline tables",
        measure_format(&small_document, &nested),
        measure_format(&large_document, &nested),
        failures,
    );

    // Growth: blank-line runs and large arrays.
    let (small, large) = (blank_lines(50_000), blank_lines(50_000 * GROWTH_FACTOR));
    let (small_document, large_document) = (parse(&small), parse(&large));
    assert_linear(
        "format blank-line run",
        measure_format(&small_document, &FormatOptions::default()),
        measure_format(&large_document, &FormatOptions::default()),
        failures,
    );
    let (small, large) = (large_array(10_000), large_array(10_000 * GROWTH_FACTOR));
    let (small_document, large_document) = (parse(&small), parse(&large));
    assert_linear(
        "format large array",
        measure_format(&small_document, &flat),
        measure_format(&large_document, &flat),
        failures,
    );
}

fn memory_cases(failures: &mut Vec<String>) {
    let (narrow, nested) = (narrow_options(), nested_options());

    // Memory waterlines: bytes of peak live heap per input byte.
    let representative = {
        let mut source = String::new();
        for index in 0..4_096 {
            let _ = writeln!(
                source,
                "[section_{index}]\nvalue = {index}\nwhen = 1979-05-27 07:32:00Z\nmetadata = {{ left = {index}, right = {} }}\nitems = [1, 2, 3, 4]",
                index + 1
            );
        }
        source
    };
    assert_peak_within(
        "check representative",
        representative.len(),
        measure_parse(&representative),
        MEMORY_BUDGET_CHECK,
        failures,
    );
    let document = parse(&representative);
    assert_peak_within(
        "format representative",
        representative.len(),
        measure_format(&document, &FormatOptions::default()),
        MEMORY_BUDGET_FORMAT,
        failures,
    );
    let tables = inline_tables(16_384);
    let document = parse(&tables);
    assert_peak_within(
        "format expanded inline tables",
        tables.len(),
        measure_format(&document, &narrow),
        MEMORY_BUDGET_FORMAT,
        failures,
    );

    // A document that the parser refuses must not be laid out at all.
    let depth = 10_000;
    let refused = format!("a = {}1{}\n", "{ b = ".repeat(depth), " }".repeat(depth));
    let (outcome, allocations) = measure(|| {
        black_box(Document::parse_and_format_with(
            refused.as_str(),
            TomlVersion::V1_1,
            &nested,
        ))
    });
    assert!(matches!(outcome.1, FormatOutcome::Refused { .. }));
    assert_peak_within(
        "refused deep nesting",
        refused.len(),
        allocations,
        MEMORY_BUDGET_REFUSAL,
        failures,
    );
}

/// Peak live heap while parsing, in bytes per input byte (the lossless tree,
/// token tape, semantics, and diagnostics of a representative document).
const MEMORY_BUDGET_CHECK: usize = 64;
/// Peak live heap while formatting an already parsed document (lookahead,
/// table facts, and the output buffer).
const MEMORY_BUDGET_FORMAT: usize = 16;
/// Peak live heap of the one-shot path on a document the parser refuses.
const MEMORY_BUDGET_REFUSAL: usize = 128;
