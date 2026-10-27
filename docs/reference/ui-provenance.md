# UI 资产来源（provenance）

- 来源：`D:\jingwei-archive-20261024\jingwei\ui`（jingwei-shell 0.3.0，自家代码，只读复制）。
- 复制清单：`index.html`、`style.css`、`core.js`、`shell.js`、`events.js`、`projects.js`、`editor.js`、`notes.js`、`scan.js`、`scan-extra.js`、`datacore.js`、`datacore-panel.js`。
- **未复制**：`graph.js`（按决策删除画图）；`node_modules` 与任何构建物；未引入 Tauri/rough。
- 改动：
  1) `index.html` 删除 graph 导航/区块/`graph.js` 引用，并在最前加载 `shim.js`。
  2) `events.js` 的 `TABS` 去掉 `graph`，删除 `graphWire/graphLoad` 分支。
  3) 新增 `shim.js`：`window.__TAURI__.invoke/listen` → `POST /rpc`；命令映射见 `ui-rpc-map.md`。
- 协议：仍走 jw serve v1（NDJSON 方法），shim 只做命令名映射，不改内核。
