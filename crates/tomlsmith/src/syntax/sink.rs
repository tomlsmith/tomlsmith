use rowan::GreenNodeBuilder;

use super::{TokenTape, parser::Event};

pub(crate) fn finish(source: &str, tokens: &TokenTape, events: &[Event]) -> rowan::GreenNode {
    let mut builder = GreenNodeBuilder::new();

    for event in events {
        match *event {
            Event::Start(kind) => builder.start_node(kind.into()),
            Event::Token(index) => {
                builder.token(tokens.kind(index).into(), &source[tokens.range(index)]);
            }
            Event::Finish => builder.finish_node(),
        }
    }

    let green = builder.finish();
    debug_assert_eq!(green.text_len(), rowan::TextSize::of(source));
    green
}
