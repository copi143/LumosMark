const path = require("path");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function activate(context) {
  // 路径指向编译后的 Rust LSP 二进制文件
  // 在开发环境下，我们通常指向 target/debug
  const serverPath = context.asAbsolutePath(
    path.join("..", "lsp", "target", "debug", "lumosmark-analyzer")
  );

  const serverOptions = {
    run: { command: serverPath, transport: TransportKind.stdio },
    debug: { command: serverPath, transport: TransportKind.stdio },
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "lmm" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.lmm"),
    },
  };

  client = new LanguageClient(
    "lumosmark-analyzer",
    "LumosMark Analyzer",
    serverOptions,
    clientOptions
  );

  client.start();
}

function deactivate() {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

module.exports = {
  activate,
  deactivate,
};
