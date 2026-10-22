# WebView2 的 User Data Folder 可以在注册表里改（67801230）

- 抓取时间：2026-10-23T07:15+08:00
- 来源：https://stackoverflow.com/questions/67801230/how-to-set-the-webview2-user-data-folder-in-the-registry
- 通道：`node tools/web.mjs so "webview2 user data folder environment" --limit 2`（StackExchange API）
- 可信度：**原话**（API 返回的问题标题/分数/是否已解决；正文未逐句核，见下）
- 失效日期：2027-10-23（一年后复核；社区答案可能改）

## 原文摘录

> 题目标题：**How to set the WebView2 User Data Folder in the Registry**
> 元数据：3 分 · 1 个回答 · **已解决** · 标签 `registry`, `webview2`

（正文经 API 可取，但本轮只核到元数据与标题，**没有逐句引用正文**——所以只记到这个程度，不往下写。）

## 它回答了哪个问题

WebView2 的 **User Data Folder**（缓存/配置目录）是否存在"不用改应用代码、只改注册表"的设置方式。

## 对我们的意义（**不下结论**，只记录待查点）

若存在注册表级设置，它会影响我们 `%LOCALAPPDATA%\jingwei\chrome-agent` 这类"独立配置"做法的替代方案；
但它也可能只对**固定版本（Fixed Version）运行时**生效——这一点必须在正文里核实，不能凭标题推断。
