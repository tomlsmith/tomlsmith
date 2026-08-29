use rowan::GreenNodeBuilder;

use super::{Token, parser::Event};

pub(crate) fn finish(source: &str, tokens: &[Token], events: &[Event]) -> rowan::GreenNode {
    let mut builder = GreenNodeBuilder::new();

    for event in events {
        match *event {
            Event::Start(kind) => builder.start_node(kind.into()),
            Event::Token(index) => {
                let token = &tokens[index];
                builder.token(token.kind.into(), &source[token.range.clone()]);
            }
            Event::Finish => builder.finish_node(),
        }
    }

    let green = builder.finish();
    debug_assert_eq!(green.text_len(), rowan::TextSize::of(source));
    green
}
