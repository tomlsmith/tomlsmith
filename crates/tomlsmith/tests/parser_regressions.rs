use tomlsmith::{DiagnosticCode, Document, SyntaxElement, SyntaxKind, SyntaxNode};

#[test]
fn multiline_strings_keep_four_and_five_quote_closings_in_one_token() {
    let cases = [
        (
            "multiline basic string ending in one quote",
            "value = \"\"\"one quote at the end.\"\"\"\"\n",
            SyntaxKind::BasicString,
        ),
        (
            "multiline basic string ending in two quotes",
            "value = \"\"\"two quotes at the end.\"\"\"\"\"\n",
            SyntaxKind::BasicString,
        ),
        (
            "multiline literal string ending in one quote",
            "value = '''one quote at the end.''''\n",
            SyntaxKind::LiteralString,
        ),
        (
            "multiline literal string ending in two quotes",
            "value = '''two quotes at the end.'''''\n",
            SyntaxKind::LiteralString,
        ),
    ];

    for (name, source, string_kind) in cases {
        let document = Document::parse(source);

        assert_eq!(document.root().text(), source, "{name} must be lossless");
        assert!(
            document.diagnostics().is_empty(),
            "{name} is valid TOML: {:?}",
            document.diagnostics()
        );

        let tokens = tokens_of_kind(&document.root(), string_kind);
        assert_eq!(tokens.len(), 1, "{name} must remain one lexer token");
        assert_eq!(
            tokens[0],
            source
                .strip_prefix("value = ")
                .and_then(|value| value.strip_suffix('\n'))
                .expect("fixture has a value and newline"),
            "{name} must include the complete closing quote run",
        );
    }
}

#[test]
fn table_headers_report_tokens_after_the_closing_delimiter_losslessly() {
    for source in ["[a] garbage\n", "[[a]]]\n", "[a] = 1\n"] {
        let document = Document::parse(source);

        assert_eq!(document.root().text(), source, "recovery must be lossless");
        assert!(
            document
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == DiagnosticCode::TRAILING_TOKENS),
            "expected trailing-token diagnostic for {source:?}: {:?}",
            document.diagnostics(),
        );
    }
}

#[test]
fn table_headers_allow_trailing_whitespace_and_comments() {
    for source in ["[a]\n", "[a] # retained\n", "[[a]] \t# retained\n"] {
        let document = Document::parse(source);

        assert_eq!(document.root().text(), source);
        assert!(
            document
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.code() != DiagnosticCode::TRAILING_TOKENS),
            "whitespace and comments are valid after a table header: {:?}",
            document.diagnostics(),
        );
    }
}

fn tokens_of_kind(node: &SyntaxNode, kind: SyntaxKind) -> Vec<String> {
    let mut tokens = Vec::new();
    collect_tokens(node, kind, &mut tokens);
    tokens
}

fn collect_tokens(node: &SyntaxNode, kind: SyntaxKind, tokens: &mut Vec<String>) {
    for element in node.children_with_tokens() {
        match element {
            SyntaxElement::Node(child) => collect_tokens(&child, kind, tokens),
            SyntaxElement::Token(token) if token.kind() == kind => {
                tokens.push(token.text().to_owned());
            }
            SyntaxElement::Token(_) => {}
        }
    }
}
