use lmm::{Node, Severity, Span, parse_document};
use lsp::jsonrpc::Result;
use lsp::lsp_types::*;
use lsp::{Client, LanguageServer, LspService, Server};
use std::collections::HashMap;
use tokio::sync::RwLock;

extern crate tower_lsp as lsp;

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: RwLock<HashMap<Url, String>>,
}

#[lsp::async_trait]
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
                document_symbol_provider: Some(OneOf::Left(true)),
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
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.store_document(uri.clone(), text.clone()).await;
        self.on_change(uri, text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().next() {
            let uri = params.text_document.uri;
            let text = change.text;
            self.store_document(uri.clone(), text.clone()).await;
            self.on_change(uri, text).await;
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

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let Some(text) = self.get_document(&uri).await else {
            return Ok(None);
        };
        let result = parse_document(&text);
        let symbols = collect_part_symbols(&result.document.nodes);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}

impl Backend {
    async fn store_document(&self, uri: Url, text: String) {
        let mut docs = self.documents.write().await;
        docs.insert(uri, text);
    }

    async fn get_document(&self, uri: &Url) -> Option<String> {
        let docs = self.documents.read().await;
        docs.get(uri).cloned()
    }

    async fn on_change(&self, uri: Url, text: String) {
        let mut diagnostics = Vec::new();

        let result = parse_document(&text);
        for diag in result.diagnostics {
            let start = Position::new(diag.span.start.line as u32, diag.span.start.col as u32);
            let end = Position::new(diag.span.end.line as u32, diag.span.end.col as u32);
            let severity = match diag.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
            };
            diagnostics.push(Diagnostic {
                range: Range::new(start, end),
                severity: Some(severity),
                message: diag.message.to_string(),
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
                label: "part".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("定义章节".to_string()),
                insert_text: Some("part { $1 }".to_string()),
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
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: RwLock::new(HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}

fn collect_part_symbols(nodes: &[Node]) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for node in nodes {
        if let Node::Block(block) = node {
            let children = collect_part_symbols(&block.nodes);
            if block.name == "part" {
                let name = if block.args.is_empty() {
                    "part".to_string()
                } else {
                    block
                        .args
                        .iter()
                        .map(|arg| arg.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                #[allow(deprecated)]
                symbols.push(DocumentSymbol {
                    name,
                    detail: None,
                    kind: SymbolKind::NAMESPACE,
                    tags: None,
                    deprecated: None,
                    range: span_to_range(block.span),
                    selection_range: span_to_range(block.span),
                    children: Some(children),
                });
            } else {
                symbols.extend(children);
            }
        }
    }
    symbols
}

fn span_to_range(span: Span) -> Range {
    let start = Position::new(span.start.line as u32, span.start.col as u32);
    let end = Position::new(span.end.line as u32, span.end.col as u32);
    Range::new(start, end)
}
