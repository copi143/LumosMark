mod ast;
mod backend;
mod parser;

pub use crate::ast::{
    Attribute, Block, Diagnostic, Document, Node, Position, Severity, Span, Text, TextLine,
};
pub use crate::backend::{render_html, render_markdown};
pub use crate::parser::{ParseOptions, ParseResult, parse_document, parse_document_with_options};

#[cfg(test)]
mod tests {
    use super::{parse_document, render_html, render_markdown};

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

    #[test]
    fn renders_markdown_and_html() {
        let input = r#"#title: Demo

@part Hello World {
  @list[bullet] {
    First item
    Second item
  }

  @code[lang=rust] {
    println!("hi");
  }
}
"#;
        let parsed = parse_document(input);
        assert_eq!(parsed.diagnostics.len(), 0);

        let markdown = render_markdown(&parsed.document);
        let expected_markdown = r#"# Hello World

- First item
- Second item

```rust
println!("hi");
```"#;
        assert_eq!(markdown, expected_markdown);

        let html = render_html(&parsed.document);
        let expected_html = r#"<div class="lmm-document" data-title="Demo">
<section class="lmm-part">
<h1>Hello World</h1>
<ul class="lmm-list" data-param-bullet="">
<li>First item</li>
<li>Second item</li>
</ul>
<pre class="lmm-code" data-param-lang="rust"><code class="language-rust">println!(&quot;hi&quot;);
</code></pre>
</section>
</div>
"#;
        assert_eq!(html, expected_html);
    }
}
