use data_classes::derive::*;
use smol_str::SmolStr;

/// A zero-based line/column position in the source text.
#[data(copy, new)]
pub struct Position {
    /// Line number (zero-based).
    pub line: usize,
    /// Column number (zero-based).
    pub col: usize,
}

/// A half-open span in the source text.
#[data(copy, new)]
pub struct Span {
    /// Start position (inclusive).
    pub start: Position,
    /// End position (exclusive).
    pub end: Position,
}

/// Severity for diagnostics emitted during parsing.
#[data]
pub enum Severity {
    Error,
    Warning,
}

/// A diagnostic message tied to a source span.
#[data]
pub struct Diagnostic {
    pub span: Span,
    pub severity: Severity,
    pub message: SmolStr,
}

/// Parsed document root containing attributes and nodes.
#[data]
pub struct Document {
    pub attrs: Vec<Attribute>,
    pub nodes: Vec<Node>,
}

/// Key/value attribute with a source span.
#[data]
pub struct Attribute {
    pub key: SmolStr,
    pub value: SmolStr,
    pub span: Span,
}

/// Top-level node kinds in the document.
#[data]
pub enum Node {
    Block(Block),
    Text(Text),
}

/// A block node with parameters, attributes, children, and span.
#[data]
pub struct Block {
    pub name: SmolStr,
    pub args: Vec<SmolStr>,
    pub params: Vec<Attribute>,
    pub attrs: Vec<Attribute>,
    pub nodes: Vec<Node>,
    pub span: Span,
}

/// A text node containing parsed lines.
#[data]
pub struct Text {
    pub lines: Vec<TextLine>,
}

/// A single text line with indentation, span, and comment marker.
#[data]
pub struct TextLine {
    pub indent: usize,
    pub value: SmolStr,
    pub span: Span,
    pub is_comment: bool,
}
