/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-22
 */
import * as vscode from "vscode";
import { spawn, ChildProcess } from "child_process";
import WebSocket from "ws";
import * as path from "path";

let statusBarItem: vscode.StatusBarItem;
let daemonProcess: ChildProcess | undefined;
let wsClient: WebSocket | undefined;
let reconnectTimer: NodeJS.Timeout;

export function activate(context: vscode.ExtensionContext) {
  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    10000,
  );
  statusBarItem.text = "$(sync~spin) 正在唤醒歌词引擎...";
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);

  const exePath = path.join(context.extensionPath, "bin", "core.exe");

  try {
    daemonProcess = spawn(exePath);
  } catch (error) {
  }

  connectWebSocket();

  context.subscriptions.push({
    dispose: () => {
      clearTimeout(reconnectTimer);
      if (wsClient) wsClient.close();
      if (daemonProcess) daemonProcess.kill();
    },
  });
}

function connectWebSocket() {
  wsClient = new WebSocket("ws://127.0.0.1:18333/ws");

  wsClient.on("open", () => {
    statusBarItem.text = "$(music) 歌词引擎就绪";
    statusBarItem.tooltip = "网易云歌词 (Hermes)";
  });

  wsClient.on("message", (data: WebSocket.RawData) => {
    const lyric = data.toString().trim();
    statusBarItem.text = `$(music) ${lyric}`;
  });

  wsClient.on("close", () => {
    statusBarItem.text = "$(error) 歌词引擎已断开";

    clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(() => {
      connectWebSocket();
    }, 3000);
  });

  wsClient.on("error", (err: Error) => {
  });
}

export function deactivate() {}
