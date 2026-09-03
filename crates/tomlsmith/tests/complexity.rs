//! In-process growth guards for the complexity contracts in
//! `docs/architecture.md`.
//!
//! Each case times one operation on an input and on the same input scaled by
//! a factor, then asserts the time grows by less than a waterline that any
//! quadratic implementation would cross. Measurements run in-process (no
//! process start), take the best of several rounds, and use large factors, so
//! they stay far from the waterline on a busy machine while still catching a
//! regression to per-token document rescans, fixed-point passes, or per-scope
//! linear lookups.

use std::{
    fmt::Write,
    time::{Duration, Instant},
};

use tomlsmith::{Document, FormatOptions, FormatOutcome, TomlVersion};

const ROUNDS: usize = 3;

fn best_of<T>(mut operation: impl FnMut() -> T) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..ROUNDS {
        let started = Instant::now();
        let result = operation();
        best = best.min(started.elapsed());
        drop(result);
    }
    best
}

fn assert_growth_below(name: &str, small: Duration, large: Duration, waterline: f64) {
    // Sub-millisecond samples are dominated by timer resolution; clamp so a
    // tiny baseline cannot inflate the ratio.
    let floor = Duration::from_micros(200);
    let ratio = large.max(floor).as_secs_f64() / small.max(floor).as_secs_f64();
    assert!(
        ratio < waterline,
        "{name}: {small:?} -> {large:?} grew {ratio:.1}x, waterline {waterline}x"
    );
}

fn format_time(source: &str, options: &FormatOptions) -> Duration {
    let document = Document::parse_as(source, options.target_version);
    assert!(
        document.diagnostics().is_empty(),
        "growth inputs must be valid: {:?}",
        document.diagnostics()
    );
    best_of(|| {
        let outcome = document.format_with(options);
        assert!(!matches!(outcome, FormatOutcome::Refused { .. }));
        outcome
    })
}

fn check_time(source: &str) -> Duration {
    best_of(|| {
        let document = Document::parse_as(source, TomlVersion::V1_1);
        assert!(document.diagnostics().is_empty());
        document
    })
}

fn independent_inline_tables(count: usize) -> String {
    let mut source = String::new();
    for index in 0..count {
        let _ = writeln!(source, "item_{index}={{left={index},right={}}}", index + 1);
    }
    source
}

#[test]
fn formatting_many_inline_tables_is_linear_in_flat_and_expanded_layout() {
    for line_width in [u16::MAX, 24] {
        let options = FormatOptions {
            line_width,
            ..FormatOptions::default()
        };
        let small = format_time(&independent_inline_tables(1_000), &options);
        let large = format_time(&independent_inline_tables(8_000), &options);
        assert_growth_below(
            &format!("inline tables at width {line_width}"),
            small,
            large,
            24.0,
        );
    }
}

#[test]
fn formatting_nested_inline_tables_scales_with_the_produced_layout() {
    // Expanded output is quadratic in depth by construction (each level adds
    // one indentation unit to every deeper line), so the input scales in
    // width rather than depth: many independent chains of the same depth.
    let chain = |depth: usize| format!("{}0{}", "{ value = ".repeat(depth), " }".repeat(depth));
    let document = |chains: usize| {
        let mut source = String::new();
        for index in 0..chains {
            let _ = writeln!(source, "root_{index} = {}", chain(64));
        }
        source
    };
    let options = FormatOptions {
        line_width: 20,
        ..FormatOptions::default()
    };
    let small = format_time(&document(16), &options);
    let large = format_time(&document(128), &options);
    assert_growth_below("nested inline-table chains", small, large, 24.0);
}

#[test]
fn formatting_blank_line_runs_and_large_arrays_is_linear() {
    let options = FormatOptions::default();
    let blank_lines = |count: usize| format!("head = 1\n{}[tail]\nvalue = 2\n", "\n".repeat(count));
    let small = format_time(&blank_lines(100_000), &options);
    let large = format_time(&blank_lines(800_000), &options);
    assert_growth_below("blank-line run", small, large, 24.0);

    let wide = FormatOptions {
        line_width: u16::MAX,
        ..FormatOptions::default()
    };
    let array = |elements: usize| format!("value = [{}]\n", vec!["0"; elements].join(","));
    let small = format_time(&array(20_000), &wide);
    let large = format_time(&array(160_000), &wide);
    assert_growth_below("large array", small, large, 24.0);
}

#[test]
fn checking_array_tables_and_implicit_parents_is_linear() {
    let array_tables = |count: usize| {
        let mut source = String::new();
        for index in 0..count {
            let _ = writeln!(source, "[[table_{index}]]\nvalue = {index}");
        }
        source
    };
    let small = check_time(&array_tables(2_000));
    let large = check_time(&array_tables(16_000));
    assert_growth_below("distinct array tables", small, large, 24.0);

    let fan_out = |count: usize| {
        let mut source = String::new();
        for index in 0..count {
            let _ = writeln!(source, "[parent.child_{index}.leaf]\nvalue = {index}");
        }
        source
    };
    let small = check_time(&fan_out(2_000));
    let large = check_time(&fan_out(16_000));
    assert_growth_below("implicit-parent fan-out", small, large, 24.0);
}
