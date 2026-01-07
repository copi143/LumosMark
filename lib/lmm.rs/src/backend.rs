use crate::ast::{Attribute, Block, Document, Node, Text, TextLine};

pub fn render_markdown(document: &Document) -> String {
    let mut out = String::new();
    render_nodes_markdown(&document.nodes, &mut out, 0);
    trim_trailing_newlines(&mut out);
    out
}

pub fn render_html(document: &Document) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"lmm-document\"");
    push_html_attrs(&mut out, &document.attrs, None);
    out.push_str(">\n");
    render_nodes_html(&document.nodes, &mut out, 0);
    out.push_str("</div>\n");
    out
}

fn render_nodes_markdown(nodes: &[Node], out: &mut String, part_level: usize) {
    for node in nodes {
        match node {
            Node::Text(text) => render_text_markdown(text, out),
            Node::Block(block) => render_block_markdown(block, out, part_level),
        }
    }
}

fn render_block_markdown(block: &Block, out: &mut String, part_level: usize) {
    match block.name.as_str() {
        "part" => {
            let level = (part_level + 1).min(6);
            let title = if block.args.is_empty() {
                "part".to_string()
            } else {
                block
                    .args
                    .iter()
                    .map(|arg| arg.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            out.push_str(&"#".repeat(level));
            out.push(' ');
            out.push_str(&title);
            out.push_str("\n\n");
            render_nodes_markdown(&block.nodes, out, part_level + 1);
        }
        "list" => {
            let style = list_style(block);
            render_list_markdown(block, out, style);
        }
        "code" => {
            let lang = block
                .params
                .iter()
                .find(|param| param.key.as_str() == "lang")
                .map(|param| param.value.as_str())
                .unwrap_or("");
            out.push_str("```");
            if !lang.is_empty() {
                out.push_str(lang);
            }
            out.push('\n');
            render_text_only_markdown(&block.nodes, out);
            out.push_str("```\n\n");
        }
        _ => {
            render_nodes_markdown(&block.nodes, out, part_level);
        }
    }
}

fn render_text_markdown(text: &Text, out: &mut String) {
    for line in &text.lines {
        if line.is_comment {
            continue;
        }
        push_indent(out, line.indent);
        out.push_str(line.value.as_str());
        out.push('\n');
    }
    out.push('\n');
}

fn render_list_markdown(block: &Block, out: &mut String, style: ListStyle) {
    let mut had_text = false;
    for node in &block.nodes {
        match node {
            Node::Text(text) => {
                for line in &text.lines {
                    if line.is_comment {
                        continue;
                    }
                    had_text = true;
                    match style {
                        ListStyle::Bullet => {
                            out.push_str("- ");
                            out.push_str(line.value.as_str());
                            out.push('\n');
                        }
                        ListStyle::Line => {
                            out.push_str(line.value.as_str());
                            out.push('\n');
                        }
                    }
                }
            }
            Node::Block(child) => render_block_markdown(child, out, 0),
        }
    }
    if had_text {
        out.push('\n');
    }
}

fn render_text_only_markdown(nodes: &[Node], out: &mut String) {
    for node in nodes {
        if let Node::Text(text) = node {
            for line in &text.lines {
                if line.is_comment {
                    continue;
                }
                out.push_str(line.value.as_str());
                out.push('\n');
            }
        }
    }
}

fn render_nodes_html(nodes: &[Node], out: &mut String, part_level: usize) {
    for node in nodes {
        match node {
            Node::Text(text) => render_text_html(text, out),
            Node::Block(block) => render_block_html(block, out, part_level),
        }
    }
}

fn render_block_html(block: &Block, out: &mut String, part_level: usize) {
    match block.name.as_str() {
        "part" => {
            let level = (part_level + 1).min(6);
            let title = if block.args.is_empty() {
                "part".to_string()
            } else {
                block
                    .args
                    .iter()
                    .map(|arg| arg.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            out.push_str("<section class=\"lmm-part\"");
            push_html_attrs(out, &block.attrs, Some(&block.params));
            out.push_str(">\n");
            out.push_str(&format!("<h{level}>", level = level));
            escape_html_into(out, &title);
            out.push_str(&format!("</h{level}>\n", level = level));
            render_nodes_html(&block.nodes, out, part_level + 1);
            out.push_str("</section>\n");
        }
        "list" => {
            let style = list_style(block);
            render_list_html(block, out, style);
        }
        "code" => {
            let lang = block
                .params
                .iter()
                .find(|param| param.key.as_str() == "lang")
                .map(|param| param.value.as_str())
                .unwrap_or("");
            out.push_str("<pre class=\"lmm-code\"");
            push_html_attrs(out, &block.attrs, Some(&block.params));
            out.push_str("><code");
            if !lang.is_empty() {
                out.push_str(" class=\"language-");
                escape_html_into(out, lang);
                out.push('\"');
            }
            out.push_str(">");
            render_text_only_html(&block.nodes, out);
            out.push_str("</code></pre>\n");
        }
        _ => {
            let class_name = format!(
                "lmm-block lmm-block-{}",
                sanitize_html_ident(block.name.as_str())
            );
            out.push_str("<div class=\"");
            out.push_str(&class_name);
            out.push('\"');
            push_html_attrs(out, &block.attrs, Some(&block.params));
            out.push_str(">\n");
            render_nodes_html(&block.nodes, out, part_level);
            out.push_str("</div>\n");
        }
    }
}

fn render_text_html(text: &Text, out: &mut String) {
    for line in &text.lines {
        if line.is_comment {
            continue;
        }
        out.push_str("<p>");
        escape_html_into(out, line.value.as_str());
        out.push_str("</p>\n");
    }
}

fn render_list_html(block: &Block, out: &mut String, style: ListStyle) {
    match style {
        ListStyle::Bullet => {
            out.push_str("<ul class=\"lmm-list\"");
            push_html_attrs(out, &block.attrs, Some(&block.params));
            out.push_str(">\n");
            for node in &block.nodes {
                match node {
                    Node::Text(text) => {
                        for line in &text.lines {
                            if line.is_comment {
                                continue;
                            }
                            out.push_str("<li>");
                            escape_html_into(out, line.value.as_str());
                            out.push_str("</li>\n");
                        }
                    }
                    Node::Block(child) => render_block_html(child, out, 0),
                }
            }
            out.push_str("</ul>\n");
        }
        ListStyle::Line => {
            out.push_str("<div class=\"lmm-lines\"");
            push_html_attrs(out, &block.attrs, Some(&block.params));
            out.push_str(">\n");
            for node in &block.nodes {
                match node {
                    Node::Text(text) => {
                        for line in &text.lines {
                            if line.is_comment {
                                continue;
                            }
                            out.push_str("<div class=\"lmm-line\">");
                            escape_html_into(out, line.value.as_str());
                            out.push_str("</div>\n");
                        }
                    }
                    Node::Block(child) => render_block_html(child, out, 0),
                }
            }
            out.push_str("</div>\n");
        }
    }
}

fn render_text_only_html(nodes: &[Node], out: &mut String) {
    for node in nodes {
        if let Node::Text(text) = node {
            for line in &text.lines {
                if line.is_comment {
                    continue;
                }
                escape_html_into(out, line.value.as_str());
                out.push('\n');
            }
        }
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push(' ');
    }
}

#[derive(Copy, Clone)]
enum ListStyle {
    Bullet,
    Line,
}

fn list_style(block: &Block) -> ListStyle {
    if has_param(block, "bullet") {
        return ListStyle::Bullet;
    }
    if has_param(block, "line") {
        return ListStyle::Line;
    }
    ListStyle::Bullet
}

fn has_param(block: &Block, key: &str) -> bool {
    block.params.iter().any(|param| param.key.as_str() == key)
        || block.args.iter().any(|arg| arg.as_str() == key)
}

fn trim_trailing_newlines(out: &mut String) {
    while out.ends_with('\n') {
        out.pop();
    }
}

fn push_html_attrs(out: &mut String, attrs: &[Attribute], params: Option<&[Attribute]>) {
    for attr in attrs {
        let key = sanitize_html_ident(attr.key.as_str());
        if key.is_empty() {
            continue;
        }
        out.push_str(" data-");
        out.push_str(&key);
        out.push_str("=\"");
        escape_html_into(out, attr.value.as_str());
        out.push('\"');
    }
    if let Some(params) = params {
        for param in params {
            let key = sanitize_html_ident(param.key.as_str());
            if key.is_empty() {
                continue;
            }
            out.push_str(" data-param-");
            out.push_str(&key);
            out.push_str("=\"");
            escape_html_into(out, param.value.as_str());
            out.push('\"');
        }
    }
}

fn sanitize_html_ident(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_whitespace() || ch == ':' {
            out.push('-');
        }
    }
    out
}

fn escape_html_into(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}
