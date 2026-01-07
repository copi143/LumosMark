const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;
let outputChannel;
let statusBarItem;
let extensionContext;

function activate(context) {
  extensionContext = context;
  context.subscriptions.push(
    vscode.commands.registerCommand("lmm.startLanguageServer", () =>
      startLanguageServer(context)
    ),
    vscode.commands.registerCommand("lmm.stopLanguageServer", () =>
      stopLanguageServer()
    ),
    vscode.commands.registerCommand("lmm.restartLanguageServer", () =>
      restartLanguageServer(context)
    ),
    vscode.commands.registerCommand("lmm.configureLanguageServerArgs", () =>
      configureLanguageServerArgs()
    ),
    vscode.commands.registerCommand("lmm.showLanguageServerInfo", () =>
      showLanguageServerInfo()
    ),
    vscode.commands.registerCommand("lmm.showLanguageServerOutput", () =>
      showLanguageServerOutput()
    )
  );

  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration("lmm")) {
        return;
      }
      const autoRestart = getConfig().get("languageServerAutoRestart", true);
      if (autoRestart && client) {
        restartLanguageServer(context);
      } else {
        updateStatusBar("stopped");
      }
    })
  );

  if (getConfig().get("languageServerStatusBar", true)) {
    ensureStatusBar(context);
  }

  startLanguageServer(context);
}

function deactivate() {
  return stopLanguageServer();
}

function getConfig() {
  return vscode.workspace.getConfiguration("lmm");
}

function ensureOutputChannel() {
  if (!outputChannel) {
    outputChannel = vscode.window.createOutputChannel("LumosMark");
    if (extensionContext) {
      extensionContext.subscriptions.push(outputChannel);
    }
  }
  return outputChannel;
}

function ensureStatusBar(context) {
  if (statusBarItem) {
    return statusBarItem;
  }
  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    100
  );
  statusBarItem.command = "lmm.showLanguageServerInfo";
  statusBarItem.text = "LumosMark: stopped";
  statusBarItem.tooltip = "LumosMark language server status";
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);
  return statusBarItem;
}

function updateStatusBar(state) {
  if (!getConfig().get("languageServerStatusBar", true)) {
    if (statusBarItem) {
      statusBarItem.hide();
    }
    return;
  }
  if (!statusBarItem) {
    return;
  }
  statusBarItem.text = `LumosMark: ${state}`;
}

function resolveServerCommand() {
  const config = getConfig();
  const inspected = config.inspect("languageServerPath");
  const configured = (inspected && (inspected.workspaceFolderValue || inspected.workspaceValue || inspected.globalValue)) || "";
  if (configured && String(configured).trim().length > 0) {
    return String(configured).trim();
  }
  const envOverride = process.env.LUMOSMARK_ANALYZER;
  if (envOverride && envOverride.trim().length > 0) {
    return envOverride.trim();
  }
  return "lumosmark-analyzer";
}

function resolveServerCwd() {
  const config = getConfig();
  const configured = config.get("languageServerWorkingDirectory");
  if (configured && configured.trim().length > 0) {
    return configured.trim();
  }
  const folders = vscode.workspace.workspaceFolders;
  if (folders && folders.length > 0) {
    return folders[0].uri.fsPath;
  }
  return undefined;
}

function resolveServerEnv() {
  const config = getConfig();
  const env = config.get("languageServerEnv", {});
  return { ...process.env, ...env };
}

function resolveServerArgs() {
  const config = getConfig();
  return config.get("languageServerArgs", []);
}

function createClientOptions() {
  return {
    documentSelector: [{ scheme: "file", language: "lmm" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.lmm"),
    },
    outputChannel: ensureOutputChannel(),
  };
}

function createServerOptions() {
  const command = resolveServerCommand();
  const args = resolveServerArgs();
  const cwd = resolveServerCwd();
  const env = resolveServerEnv();

  const run = { command, args, transport: TransportKind.stdio, options: { cwd, env } };
  const debug = { command, args, transport: TransportKind.stdio, options: { cwd, env } };
  return { run, debug };
}

async function startLanguageServer(context) {
  if (client) {
    return;
  }
  const serverOptions = createServerOptions();
  const clientOptions = createClientOptions();

  client = new LanguageClient(
    "lumosmark-analyzer",
    "LumosMark Analyzer",
    serverOptions,
    clientOptions
  );

  client.onDidChangeState((event) => {
    if (event.newState === 2) {
      updateStatusBar("running");
    } else if (event.newState === 1) {
      updateStatusBar("starting");
    } else {
      updateStatusBar("stopped");
    }
  });

  if (getConfig().get("languageServerStatusBar", true)) {
    ensureStatusBar(context);
  }

  updateStatusBar("starting");
  await client.start();
}

async function stopLanguageServer() {
  if (!client) {
    return;
  }
  const current = client;
  client = undefined;
  updateStatusBar("stopped");
  await current.stop();
}

async function restartLanguageServer(context) {
  await stopLanguageServer();
  await startLanguageServer(context);
}

async function configureLanguageServerArgs() {
  const config = getConfig();
  const current = config.get("languageServerArgs", []);
  const input = await vscode.window.showInputBox({
    title: "Configure LumosMark language server arguments",
    prompt: "Enter arguments (space-separated) or a JSON array",
    value: current.length ? current.join(" ") : "",
  });
  if (input === undefined) {
    return;
  }
  const trimmed = input.trim();
  let args = [];
  if (trimmed.length === 0) {
    args = [];
  } else if (trimmed.startsWith("[")) {
    try {
      const parsed = JSON.parse(trimmed);
      if (Array.isArray(parsed)) {
        args = parsed.map((item) => String(item));
      } else {
        vscode.window.showErrorMessage("Arguments JSON must be an array.");
        return;
      }
    } catch (err) {
      vscode.window.showErrorMessage("Invalid JSON array.");
      return;
    }
  } else {
    args = splitArgs(trimmed);
  }
  await config.update("languageServerArgs", args, vscode.ConfigurationTarget.Workspace);
}

function splitArgs(value) {
  const result = [];
  let current = "";
  let inQuotes = false;
  let quoteChar = "";
  for (let i = 0; i < value.length; i += 1) {
    const ch = value[i];
    if ((ch === '"' || ch === "'") && !inQuotes) {
      inQuotes = true;
      quoteChar = ch;
      continue;
    }
    if (inQuotes && ch === quoteChar) {
      inQuotes = false;
      quoteChar = "";
      continue;
    }
    if (!inQuotes && /\s/.test(ch)) {
      if (current.length > 0) {
        result.push(current);
        current = "";
      }
      continue;
    }
    current += ch;
  }
  if (current.length > 0) {
    result.push(current);
  }
  return result;
}

function showLanguageServerInfo() {
  const command = resolveServerCommand();
  const args = resolveServerArgs();
  const cwd = resolveServerCwd();
  const env = getConfig().get("languageServerEnv", {});
  const info = [
    `Command: ${command}`,
    `Args: ${args.join(" ") || "(none)"}`,
    `Working Directory: ${cwd || "(default)"}`,
    `Env: ${Object.keys(env).length ? JSON.stringify(env) : "(none)"}`,
    `Status: ${client ? "running" : "stopped"}`,
  ].join("\n");
  ensureOutputChannel().appendLine(info);
  ensureOutputChannel().show(true);
}

function showLanguageServerOutput() {
  ensureOutputChannel().show(true);
}

module.exports = {
  activate,
  deactivate,
};
