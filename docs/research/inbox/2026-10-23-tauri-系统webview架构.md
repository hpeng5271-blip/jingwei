# Tauri 用系统 WebView（WRY/WebView2）渲染（Tauri 官方 README）

- 抓取时间：2026-10-23T06:53+08:00
- 来源：https://github.com/tauri-apps/tauri（raw README）
- 通道：`node tools/web.mjs fetch https://raw.githubusercontent.com/tauri-apps/tauri/dev/README.md`
- 可信度：原话（README 正文）

## 原文摘录

> Tauri is a framework for building tiny, blazingly fast binaries for all major operating systems.
> To render your application, Tauri uses WRY, a library which provides a unified interface to the system webview, leveraging WKWebView on macOS & iOS, WebView2 on Windows, WebKitGTK on Linux and Android System WebView on Android.
> Native WebView Protocol (tauri doesn't create a localhost http(s) server to serve the WebView contents)

## 它回答了哪个问题

Tauri 体积/内存低的架构根因：用系统 WebView（Windows 上是 WebView2），不捆绑 Chromium、不起本地 HTTP 服务。
