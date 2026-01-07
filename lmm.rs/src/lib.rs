mod ast;
mod parser;

pub use crate::ast::{
    Attribute, Block, Diagnostic, Document, Node, Position, Severity, Span, Text, TextLine,
};
pub use crate::parser::{ParseOptions, ParseResult, parse_document, parse_document_with_options};

#[cfg(test)]
mod tests {
    use super::parse_document;

    #[test]
    fn parses_block_with_attrs_and_text() {
        let input = r#"
            @block key1="value1" key2="value2" {
                This is some text inside the block.
            }
        "#;
        let result = parse_document(input);
        assert_eq!(result.diagnostics.len(), 0);
        assert_eq!(result.document.nodes.len(), 1);
    }
}
