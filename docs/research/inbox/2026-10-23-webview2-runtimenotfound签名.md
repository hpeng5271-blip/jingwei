# WebView2 RuntimeNotFound 的错误签名（StackOverflow 76065661）

- 抓取时间：2026-10-23T06:50+08:00
- 来源：https://stackoverflow.com/questions/76065661/windows-forms-webview2-runtime-not-found-exception-although-it-is-installed
- 通道：`node tools/web.mjs soq 76065661`
- 可信度：原话（问题正文，含异常签名）

## 原文摘录

> `Microsoft.Web.WebView2.Core.WebView2RuntimeNotFoundException: 'Couldn't find a compatible Webview2 Runtime installation to host WebViews.'`
> `FileNotFoundException: The system cannot find the file specified. (Exception from HRESULT: 0x80070002)`
> It happens on this line: `await PowerBi_web.EnsureCoreWebView2Async(await CoreWebView2Environment.CreateAsync(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "WebView2")));`

## 它回答了哪个问题

WebView2 运行时缺失时的准确异常签名（`WebView2RuntimeNotFoundException` + `HRESULT 0x80070002`），用于把"宿主缺运行时"和"应用坏"区分开。
