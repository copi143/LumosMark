use data_classes::derive::*;
use smol_str::SmolStr;

use crate::ast::{
    Attribute, Block, Diagnostic, Document, Node, Position, Severity, Span, Text, TextLine,
};

#[data(default, copy)]
pub struct ParseOptions {
    #[default = 1]
    pub space_width: usize,
    #[default = 2]
    pub tab_width: usize,
}

#[data]
pub struct ParseResult {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_document(input: &str) -> ParseResult {
    parse_document_with_options(input, ParseOptions::default())
}

pub fn parse_document_with_options(input: &str, options: ParseOptions) -> ParseResult {
    let mut parser = Parser::new(input, options);
    let document = parser.parse_document();
    ParseResult {
        document,
        diagnostics: parser.diagnostics,
    }
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    line_index: usize,
    col: usize,
    options: ParseOptions,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, options: ParseOptions) -> Self {
        let lines = input.lines().collect::<Vec<_>>();
        Self {
            lines,
            line_index: 0,
            col: 0,
            options,
            diagnostics: Vec::new(),
        }
    }

    fn parse_document(&mut self) -> Document {
        let attrs = self.parse_attributes_at_start();
        let nodes = self.parse_nodes_until(None);
        self.consume_trailing_comments();
        if !self.at_end() {
            let span = self.span_for_line(self.line_index, self.col, self.col);
            self.push_diag(span, Severity::Error, "unexpected trailing content");
        }
        Document { attrs, nodes }
    }

    fn parse_nodes_until(&mut self, closing: Option<&str>) -> Vec<Node> {
        let mut nodes = Vec::new();
        let mut text_buf: Vec<LineBuf> = Vec::new();
        let mut closed = closing.is_none();

        while !self.at_end() {
            if let Some(close) = closing {
                if let Some(pos) = self.find_close_in_line(close) {
                    if pos == self.col {
                        self.flush_text(&mut nodes, &mut text_buf);
                        self.col = pos + close.len();
                        closed = true;
                        break;
                    }
                    if let Some(line_buf) = self.parse_text_segment(self.col, pos) {
                        text_buf.push(line_buf);
                    }
                    self.flush_text(&mut nodes, &mut text_buf);
                    self.col = pos + close.len();
                    closed = true;
                    break;
                }
            }

            if self.is_line_start() {
                if let Some(line) = self.current_line() {
                    if is_comment_line(line) {
                        if let Some(line_buf) =
                            parse_comment_line(line, self.line_index, self.options)
                        {
                            text_buf.push(line_buf);
                        }
                        self.advance_line();
                        continue;
                    }
                    if line.trim().is_empty() {
                        self.advance_line();
                        continue;
                    }
                    if is_dollar_line(line) {
                        self.flush_text(&mut nodes, &mut text_buf);
                        self.advance_line();
                        let raw_lines = self.collect_until_dollar();
                        let mut lines = Vec::new();
                        for (line_index, raw) in raw_lines {
                            if let Some(line_buf) = parse_text_line(raw, line_index, self.options) {
                                lines.push(line_buf);
                            }
                        }
                        if !lines.is_empty() {
                            let text = finalize_text(lines);
                            nodes.push(Node::Text(text));
                        }
                        continue;
                    }
                    if let Some(block) = self.try_parse_block_header() {
                        self.flush_text(&mut nodes, &mut text_buf);
                        let attrs = self.parse_attributes_at_start();
                        let close_delim = block_close_delim(block.plus_count);
                        let children = self.parse_nodes_until(Some(&close_delim));
                        nodes.push(Node::Block(Block {
                            name: block.name,
                            args: block.args,
                            params: block.params,
                            attrs,
                            nodes: children,
                            span: block.span,
                        }));
                        continue;
                    }
                }
            }

            if let Some(line) = self.current_line() {
                let end = line.len();
                if let Some(line_buf) = self.parse_text_segment(self.col, end) {
                    text_buf.push(line_buf);
                }
                self.advance_line();
            }
        }

        self.flush_text(&mut nodes, &mut text_buf);
        if !closed {
            let span = self.span_for_line(self.line_index.saturating_sub(1), 0, 0);
            self.push_diag(span, Severity::Error, "missing closing delimiter");
        }
        nodes
    }

    fn parse_text_segment(&mut self, start: usize, end: usize) -> Option<LineBuf> {
        let line = self.current_line()?;
        parse_text_segment(line, self.line_index, start, end, self.options)
    }

    fn flush_text(&mut self, nodes: &mut Vec<Node>, text_buf: &mut Vec<LineBuf>) {
        if text_buf.is_empty() {
            return;
        }
        let mut lines = Vec::with_capacity(text_buf.len());
        for line in text_buf.drain(..) {
            let value = unescape_text(&line.value);
            lines.push(TextLine {
                indent: line.indent,
                value: value.into(),
                span: line.span,
                is_comment: line.is_comment,
            });
        }
        nodes.push(Node::Text(Text { lines }));
    }

    fn parse_attributes_at_start(&mut self) -> Vec<Attribute> {
        let mut attrs = Vec::new();
        loop {
            if !self.is_line_start() {
                break;
            }
            let Some(line) = self.current_line() else {
                break;
            };
            if is_comment_line(line) {
                break;
            }
            if line.trim().is_empty() {
                break;
            }
            if let Some(attr) = self.parse_attribute_line(line) {
                attrs.push(attr);
                self.advance_line();
                continue;
            }
            break;
        }
        attrs
    }

    fn parse_attribute_line(&mut self, line: &str) -> Option<Attribute> {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') || trimmed.starts_with("##") {
            return None;
        }
        let without_hash = &trimmed[1..];
        let Some((key, value)) = without_hash.split_once(':') else {
            let span = self.line_span_from_line(line);
            self.push_diag(span, Severity::Error, "attribute missing ':'");
            return None;
        };
        let key = key.trim();
        if key.is_empty() {
            let span = self.line_span_from_line(line);
            self.push_diag(span, Severity::Error, "attribute key is empty");
            return None;
        }
        let span = self.line_span_from_line(line);
        Some(Attribute {
            key: key.into(),
            value: value.trim().into(),
            span,
        })
    }

    fn try_parse_block_header(&mut self) -> Option<BlockHeader> {
        let line = self.current_line()?;
        let (at_col, header_start) = find_block_header_start(line)?;
        let start_pos = Position::new(self.line_index, at_col);
        let (header_raw, header_span, end_pos) = match self.collect_header(header_start) {
            Some(value) => value,
            None => {
                let span = self.line_span_from_line(line);
                self.push_diag(
                    span,
                    Severity::Error,
                    "block header missing opening delimiter",
                );
                self.advance_line();
                return None;
            }
        };

        self.set_position(end_pos);
        self.advance_line_if_eol();

        let (name, args, params, plus_count, missing_space) = match parse_header_parts(&header_raw)
        {
            Some(value) => value,
            None => {
                self.push_diag(header_span, Severity::Error, "missing block name");
                return None;
            }
        };

        if missing_space {
            self.push_diag(
                header_span,
                Severity::Warning,
                "LMM 格式规范：标记名称与左大括号之间必须有一个空格。建议改为 '@name {'",
            );
        }

        Some(BlockHeader {
            name,
            args,
            params: params
                .into_iter()
                .map(|(key, value)| Attribute {
                    key,
                    value,
                    span: header_span,
                })
                .collect(),
            plus_count,
            span: Span::new(start_pos, header_span.end),
        })
    }

    fn collect_header(&mut self, start_col: usize) -> Option<(String, Span, Position)> {
        let mut header = String::new();
        let mut line_index = self.line_index;
        let mut col = start_col;
        loop {
            let line = self.lines.get(line_index)?;
            let bytes = line.as_bytes();
            while col < bytes.len() {
                let ch = bytes[col] as char;
                if ch == '{' {
                    return Some((
                        header,
                        Span::new(
                            Position::new(self.line_index, start_col),
                            Position::new(line_index, col + 1),
                        ),
                        Position::new(line_index, col + 1),
                    ));
                }
                header.push(ch);
                col += 1;
            }
            header.push(' ');
            line_index += 1;
            col = 0;
            if line_index >= self.lines.len() {
                break;
            }
        }
        None
    }

    fn collect_until_dollar(&mut self) -> Vec<(usize, &'a str)> {
        let mut out = Vec::new();
        while !self.at_end() {
            let line = self.current_line().unwrap_or("");
            if is_dollar_line(line) {
                self.advance_line();
                return out;
            }
            out.push((self.line_index, line));
            self.advance_line();
        }
        let span = self.span_for_line(self.line_index.saturating_sub(1), 0, 0);
        self.push_diag(span, Severity::Error, "unterminated $ block");
        out
    }

    fn consume_trailing_comments(&mut self) {
        while !self.at_end() {
            let Some(line) = self.current_line() else {
                break;
            };
            if is_comment_line(line) || line.trim().is_empty() {
                self.advance_line();
                continue;
            }
            break;
        }
    }

    fn find_close_in_line(&self, close: &str) -> Option<usize> {
        let line = self.current_line()?;
        line[self.col..].find(close).map(|idx| idx + self.col)
    }

    fn is_line_start(&self) -> bool {
        self.col == 0
    }

    fn at_end(&self) -> bool {
        self.line_index >= self.lines.len()
    }

    fn current_line(&self) -> Option<&'a str> {
        self.lines.get(self.line_index).copied()
    }

    fn advance_line(&mut self) {
        self.line_index += 1;
        self.col = 0;
    }

    fn advance_line_if_eol(&mut self) {
        if let Some(line) = self.current_line() {
            if self.col >= line.len() {
                self.advance_line();
            }
        }
    }

    fn set_position(&mut self, pos: Position) {
        self.line_index = pos.line;
        self.col = pos.col;
    }

    fn push_diag(&mut self, span: Span, severity: Severity, message: &str) {
        self.diagnostics.push(Diagnostic {
            span,
            severity,
            message: message.into(),
        });
    }

    fn span_for_line(&self, line_index: usize, start_col: usize, end_col: usize) -> Span {
        Span::new(
            Position::new(line_index, start_col),
            Position::new(line_index, end_col),
        )
    }

    fn line_span_from_line(&self, line: &str) -> Span {
        let end_col = line.len();
        self.span_for_line(self.line_index, 0, end_col)
    }
}

struct BlockHeader {
    name: SmolStr,
    args: Vec<SmolStr>,
    params: Vec<Attribute>,
    plus_count: usize,
    span: Span,
}

struct LineBuf {
    indent: usize,
    value: String,
    span: Span,
    is_comment: bool,
}

fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("!!") {
        return false;
    }
    trimmed.starts_with('!')
}

fn is_dollar_line(line: &str) -> bool {
    line.trim() == "$"
}

fn block_close_delim(plus_count: usize) -> String {
    let mut out = String::from("}");
    for _ in 0..plus_count {
        out.push('+');
    }
    out
}

fn parse_text_line(line: &str, line_index: usize, options: ParseOptions) -> Option<LineBuf> {
    if is_comment_line(line) {
        return parse_comment_line(line, line_index, options);
    }
    parse_text_segment(line, line_index, 0, line.len(), options)
}

fn parse_text_segment(
    line: &str,
    line_index: usize,
    start: usize,
    end: usize,
    options: ParseOptions,
) -> Option<LineBuf> {
    if start >= end {
        return None;
    }
    let segment = &line[start..end];
    let mut indent = 0usize;
    let mut skip = 0usize;

    if start == 0 {
        for ch in segment.chars() {
            if ch == ' ' {
                indent += options.space_width;
                skip += 1;
            } else if ch == '\t' {
                indent += options.tab_width;
                skip += 1;
            } else {
                break;
            }
        }
    }

    let value_start = start + skip;
    let mut value = segment[skip..].to_string();
    if start == 0 && value.starts_with("!!") {
        value = format!("!{}", &value[2..]);
    }

    if value.is_empty() {
        return None;
    }

    let span = Span::new(
        Position::new(line_index, value_start),
        Position::new(line_index, value_start + value.len()),
    );

    Some(LineBuf {
        indent,
        value,
        span,
        is_comment: false,
    })
}

fn parse_comment_line(line: &str, line_index: usize, options: ParseOptions) -> Option<LineBuf> {
    if !is_comment_line(line) {
        return None;
    }
    let mut indent = 0usize;
    let mut skip = 0usize;
    for ch in line.chars() {
        if ch == ' ' {
            indent += options.space_width;
            skip += 1;
        } else if ch == '\t' {
            indent += options.tab_width;
            skip += 1;
        } else {
            break;
        }
    }
    let after_indent = &line[skip..];
    let rest = after_indent.strip_prefix('!').unwrap_or("");
    let trimmed = rest.trim_start();
    let leading = rest.len().saturating_sub(trimmed.len());
    let value_start = skip + 1 + leading;
    let value = trimmed.to_string();
    let span = Span::new(
        Position::new(line_index, value_start),
        Position::new(line_index, value_start + value.len()),
    );
    Some(LineBuf {
        indent,
        value,
        span,
        is_comment: true,
    })
}

fn finalize_text(lines: Vec<LineBuf>) -> Text {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let value = unescape_text(&line.value);
        out.push(TextLine {
            indent: line.indent,
            value: value.into(),
            span: line.span,
            is_comment: line.is_comment,
        });
    }
    Text { lines: out }
}

fn unescape_text(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(next) = chars.peek().copied() {
            if ch == '@' && next == '@' {
                out.push('@');
                chars.next();
                continue;
            }
            if ch == '#' && next == '#' {
                out.push('#');
                chars.next();
                continue;
            }
            if ch == '{' && next == '{' {
                out.push('{');
                chars.next();
                continue;
            }
        }
        out.push(ch);
    }

    out
}

fn find_block_header_start(line: &str) -> Option<(usize, usize)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('@') {
        let at_col = line.len() - trimmed.len();
        return Some((at_col, at_col));
    }
    None
}

fn parse_header_parts(
    header: &str,
) -> Option<(SmolStr, Vec<SmolStr>, Vec<(SmolStr, SmolStr)>, usize, bool)> {
    let bytes = header.as_bytes();
    if bytes.is_empty() || bytes[0] as char != '@' {
        return None;
    }
    let mut cursor = 1;
    let name_start = cursor;
    while cursor < bytes.len() {
        let ch = bytes[cursor] as char;
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            cursor += 1;
        } else {
            break;
        }
    }
    if cursor == name_start {
        return None;
    }
    let name = &header[name_start..cursor];
    let missing_space = cursor == bytes.len();

    cursor = skip_spaces(bytes, cursor);
    let mut args = Vec::new();
    while cursor < bytes.len() {
        let ch = bytes[cursor] as char;
        if ch == '[' || ch == '+' {
            break;
        }
        if ch.is_ascii_whitespace() {
            cursor = skip_spaces(bytes, cursor);
            continue;
        }
        let start = cursor;
        while cursor < bytes.len() {
            let ch = bytes[cursor] as char;
            if ch.is_ascii_whitespace() || ch == '[' || ch == '+' {
                break;
            }
            cursor += 1;
        }
        let arg = &header[start..cursor];
        args.push(arg.into());
        cursor = skip_spaces(bytes, cursor);
    }

    let mut params = Vec::new();
    if cursor < bytes.len() && bytes[cursor] as char == '[' {
        let (parsed, next) = parse_params(header, cursor);
        params = parsed;
        cursor = next;
        cursor = skip_spaces(bytes, cursor);
    }

    let mut plus_count = 0usize;
    while cursor < bytes.len() && bytes[cursor] as char == '+' {
        plus_count += 1;
        cursor += 1;
    }

    Some((name.into(), args, params, plus_count, missing_space))
}

fn parse_params(header: &str, start: usize) -> (Vec<(SmolStr, SmolStr)>, usize) {
    let bytes = header.as_bytes();
    let mut cursor = start;
    if bytes.get(cursor).map(|b| *b as char) != Some('[') {
        return (Vec::new(), cursor);
    }
    cursor += 1;
    let mut params = Vec::new();
    let mut token = String::new();

    while cursor < bytes.len() {
        let ch = bytes[cursor] as char;
        if ch == ']' {
            if !token.trim().is_empty() {
                params.push(parse_param_token(&token));
            }
            cursor += 1;
            return (params, cursor);
        }
        if ch == ',' {
            if !token.trim().is_empty() {
                params.push(parse_param_token(&token));
            }
            token.clear();
            cursor += 1;
            continue;
        }
        token.push(ch);
        cursor += 1;
    }

    (params, cursor)
}

fn parse_param_token(token: &str) -> (SmolStr, SmolStr) {
    let token = token.trim();
    let Some((key, value)) = token.split_once('=') else {
        return (token.into(), "".into());
    };
    (key.trim().into(), value.trim().into())
}

fn skip_spaces(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && (bytes[index] as char).is_ascii_whitespace() {
        index += 1;
    }
    index
}
