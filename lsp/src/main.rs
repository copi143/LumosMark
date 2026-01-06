use regex::Regex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["@".to_string(), "#".to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "LumosMark LSP initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.on_change(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().next() {
            self.on_change(params.text_document.uri, change.text).await;
        }
    }

    async fn hover(&self, _params: HoverParams) -> Result<Option<Hover>> {
        Ok(Some(Hover {
            contents: HoverContents::Scalar(MarkedString::String(
                "LumosMark element detected".to_string(),
            )),
            range: None,
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.get_completions(params).await
    }
}

impl Backend {
    async fn on_change(&self, uri: Url, text: String) {
        let mut diagnostics = Vec::new();

        // 规则 1：标记名称与左大括号之间必须有一个空格
        // 排除转义的 @@
        let re_missing_space = Regex::new(r"(?P<at>@)(?P<name>[a-zA-Z0-9]+)\{").unwrap();
        for cap in re_missing_space.captures_iter(&text) {
            let m = cap.get(0).unwrap();
            // 检查前面是否也是一个 @ (即 @@ 转义)
            let start = m.start();
            if start > 0 && &text[start - 1..start] == "@" {
                continue;
            }

            let start_pos = self.offset_to_position(m.start(), &text);
            let end_pos = self.offset_to_position(m.end(), &text);

            diagnostics.push(Diagnostic {
                range: Range::new(start_pos, end_pos),
                severity: Some(DiagnosticSeverity::WARNING),
                message: "LMM 格式规范：标记名称与左大括号之间必须有一个空格。建议改为 '@name {'"
                    .to_string(),
                source: Some("LumosMark".to_string()),
                ..Default::default()
            });
        }

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn get_completions(
        &self,
        _params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let completions = vec![
            CompletionItem {
                label: "section".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("定义章节".to_string()),
                insert_text: Some("section { $1 }".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "list".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("列表块".to_string()),
                insert_text: Some("list bullet {\n  $1\n}".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "code".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("代码块".to_string()),
                insert_text: Some("code[lang=$1] {\n  $2\n}".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "b".to_string(),
                kind: Some(CompletionItemKind::TEXT),
                detail: Some("粗体".to_string()),
                insert_text: Some("b {$1}".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
        ];
        Ok(Some(CompletionResponse::Array(completions)))
    }

    fn offset_to_position(&self, offset: usize, text: &str) -> Position {
        let mut line = 0;
        let mut character = 0;
        for (i, c) in text.char_indices() {
            if i == offset {
                break;
            }
            if c == '\n' {
                line += 1;
                character = 0;
            } else {
                character += 1;
            }
        }
        Position::new(line, character)
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend { client });
    Server::new(stdin, stdout, socket).serve(service).await;
}
