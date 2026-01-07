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
    input: &'a str,
    idx: usize,
    line_start_idx: usize,
    pos: Position,
    options: ParseOptions,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, options: ParseOptions) -> Self {
        Self {
            input,
            idx: 0,
            line_start_idx: 0,
            pos: Position::new(0, 0, 0, 0),
            options,
            diagnostics: Vec::new(),
        }
    }

    fn parse_document(&mut self) -> Document {
        let attrs = self.parse_attributes_at_start();
        let nodes = self.parse_nodes_until(None);
        self.consume_trailing_comments();
        if !self.at_end() {
            let span = span_at_line_start(self.pos.line);
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
                if let Some(close_idx) = self.find_close_in_line(close) {
                    if close_idx == self.idx {
                        self.flush_text(&mut nodes, &mut text_buf);
                        self.advance_to_idx(close_idx + close.len());
                        closed = true;
                        break;
                    }
                    let line = self.current_line_slice().unwrap_or("");
                    let start_offset = self.current_line_offset();
                    let end_offset = close_idx - self.line_start_idx;
                    if let Some(line_buf) =
                        self.parse_text_segment(line, self.pos.line, start_offset, end_offset)
                    {
                        text_buf.push(line_buf);
                    }
                    self.flush_text(&mut nodes, &mut text_buf);
                    self.advance_to_idx(close_idx + close.len());
                    closed = true;
                    break;
                }
            }

            if self.is_line_start() {
                if let Some(line) = self.current_line_slice() {
                    if is_comment_line(line) {
                        if let Some(line_buf) =
                            parse_comment_line(line, self.pos.line, self.options)
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

            if let Some(line) = self.current_line_slice() {
                let line_end = self.line_end_idx();
                let start_offset = self.current_line_offset();
                let end_offset = line_end - self.line_start_idx;
                if let Some(line_buf) =
                    self.parse_text_segment(line, self.pos.line, start_offset, end_offset)
                {
                    text_buf.push(line_buf);
                }
                self.advance_line();
            }
        }

        self.flush_text(&mut nodes, &mut text_buf);
        if !closed {
            let line_index = self.pos.line.saturating_sub(1);
            let span = span_at_line_start(line_index);
            self.push_diag(span, Severity::Error, "missing closing delimiter");
        }
        nodes
    }

    fn parse_text_segment(
        &self,
        line: &str,
        line_index: usize,
        start: usize,
        end: usize,
    ) -> Option<LineBuf> {
        parse_text_segment(line, line_index, start, end, self.options)
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
            let Some(line) = self.current_line_slice() else {
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
            let span = line_span_from_line(self.pos.line, line);
            self.push_diag(span, Severity::Error, "attribute missing ':'");
            return None;
        };
        let key = key.trim();
        if key.is_empty() {
            let span = line_span_from_line(self.pos.line, line);
            self.push_diag(span, Severity::Error, "attribute key is empty");
            return None;
        }
        let span = line_span_from_line(self.pos.line, line);
        Some(Attribute {
            key: key.into(),
            value: value.trim().into(),
            span,
        })
    }

    fn try_parse_block_header(&mut self) -> Option<BlockHeader> {
        let line = self.current_line_slice()?;
        let (at_col, header_start) = find_block_header_start(line)?;
        let start_pos = position_for_line_offset(self.pos.line, line, at_col);
        let start_idx = self.line_start_idx + header_start;
        let Some((header_raw, header_span, end_idx)) = self.scan_header(start_idx, start_pos)
        else {
            let span = line_span_from_line(self.pos.line, line);
            self.push_diag(
                span,
                Severity::Error,
                "block header missing opening delimiter",
            );
            self.advance_line();
            return None;
        };

        self.advance_to_idx(end_idx);
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

    fn scan_header(&self, start_idx: usize, start_pos: Position) -> Option<(String, Span, usize)> {
        let mut header = String::new();
        let mut idx = start_idx;
        let mut pos = start_pos;

        while idx < self.input.len() {
            let slice = &self.input[idx..];
            let Some(ch) = slice.chars().next() else {
                break;
            };
            if ch == '{' {
                let end_pos = advance_position(pos, ch);
                let end_idx = idx + ch.len_utf8();
                return Some((header, Span::new(start_pos, end_pos), end_idx));
            }
            if ch == '\n' {
                header.push(' ');
                pos = Position::new(pos.line + 1, 0, 0, 0);
                idx += ch.len_utf8();
                continue;
            }
            header.push(ch);
            pos = advance_position(pos, ch);
            idx += ch.len_utf8();
        }
        None
    }

    fn collect_until_dollar(&mut self) -> Vec<(usize, &'a str)> {
        let mut out = Vec::new();
        while !self.at_end() {
            let line = self.current_line_slice().unwrap_or("");
            if is_dollar_line(line) {
                self.advance_line();
                return out;
            }
            out.push((self.pos.line, line));
            self.advance_line();
        }
        let line_index = self.pos.line.saturating_sub(1);
        let span = span_at_line_start(line_index);
        self.push_diag(span, Severity::Error, "unterminated $ block");
        out
    }

    fn consume_trailing_comments(&mut self) {
        while !self.at_end() {
            let Some(line) = self.current_line_slice() else {
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
        let line_end = self.line_end_idx();
        if self.idx > line_end {
            return None;
        }
        let line_tail = &self.input[self.idx..line_end];
        line_tail.find(close).map(|idx| self.idx + idx)
    }

    fn is_line_start(&self) -> bool {
        self.idx == self.line_start_idx
    }

    fn at_end(&self) -> bool {
        self.idx >= self.input.len()
    }

    fn current_line_slice(&self) -> Option<&'a str> {
        if self.line_start_idx > self.input.len() {
            return None;
        }
        let end = self.line_end_idx();
        Some(&self.input[self.line_start_idx..end])
    }

    fn line_end_idx(&self) -> usize {
        if self.line_start_idx >= self.input.len() {
            return self.input.len();
        }
        let tail = &self.input[self.line_start_idx..];
        match tail.find('\n') {
            Some(offset) => self.line_start_idx + offset,
            None => self.input.len(),
        }
    }

    fn current_line_offset(&self) -> usize {
        self.idx.saturating_sub(self.line_start_idx)
    }

    fn advance_line(&mut self) {
        let line_end = self.line_end_idx();
        if self.idx < line_end {
            self.advance_to_idx(line_end);
        }
        if self.peek_char() == Some('\n') {
            self.advance_char();
        }
    }

    fn advance_line_if_eol(&mut self) {
        let line_end = self.line_end_idx();
        if self.idx >= line_end {
            if self.peek_char() == Some('\n') {
                self.advance_char();
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.idx..].chars().next()
    }

    fn advance_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        let len = ch.len_utf8();
        self.idx += len;
        if ch == '\n' {
            self.pos.line += 1;
            self.pos.col8 = 0;
            self.pos.col16 = 0;
            self.pos.col32 = 0;
            self.line_start_idx = self.idx;
        } else {
            self.pos.col8 += len;
            self.pos.col16 += ch.len_utf16();
            self.pos.col32 += 1;
        }
        Some(ch)
    }

    fn advance_to_idx(&mut self, target: usize) {
        if target <= self.idx {
            return;
        }
        let slice = &self.input[self.idx..target];
        for ch in slice.chars() {
            let len = ch.len_utf8();
            self.idx += len;
            if ch == '\n' {
                self.pos.line += 1;
                self.pos.col8 = 0;
                self.pos.col16 = 0;
                self.pos.col32 = 0;
                self.line_start_idx = self.idx;
            } else {
                self.pos.col8 += len;
                self.pos.col16 += ch.len_utf16();
                self.pos.col32 += 1;
            }
        }
    }

    fn push_diag(&mut self, span: Span, severity: Severity, message: &str) {
        self.diagnostics.push(Diagnostic {
            span,
            severity,
            message: message.into(),
        });
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
                skip += ch.len_utf8();
            } else if ch == '\t' {
                indent += options.tab_width;
                skip += ch.len_utf8();
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

    let span = span_for_line_offsets(line_index, line, value_start, value_start + value.len());

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
            skip += ch.len_utf8();
        } else if ch == '\t' {
            indent += options.tab_width;
            skip += ch.len_utf8();
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
    let span = span_for_line_offsets(line_index, line, value_start, value_start + value.len());
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

fn span_at_line_start(line_index: usize) -> Span {
    let pos = Position::new(line_index, 0, 0, 0);
    Span::new(pos, pos)
}

fn line_span_from_line(line_index: usize, line: &str) -> Span {
    let end = position_for_line_offset(line_index, line, line.len());
    Span::new(Position::new(line_index, 0, 0, 0), end)
}

fn span_for_line_offsets(line_index: usize, line: &str, start: usize, end: usize) -> Span {
    let start_pos = position_for_line_offset(line_index, line, start);
    let end_pos = position_for_line_offset(line_index, line, end);
    Span::new(start_pos, end_pos)
}

fn position_for_line_offset(line_index: usize, line: &str, byte_offset: usize) -> Position {
    let mut col8 = 0usize;
    let mut col16 = 0usize;
    let mut col32 = 0usize;
    let prefix = &line[..byte_offset];
    for ch in prefix.chars() {
        col8 += ch.len_utf8();
        col16 += ch.len_utf16();
        col32 += 1;
    }
    Position::new(line_index, col8, col16, col32)
}

fn advance_position(pos: Position, ch: char) -> Position {
    if ch == '\n' {
        Position::new(pos.line + 1, 0, 0, 0)
    } else {
        Position::new(
            pos.line,
            pos.col8 + ch.len_utf8(),
            pos.col16 + ch.len_utf16(),
            pos.col32 + 1,
        )
    }
}
