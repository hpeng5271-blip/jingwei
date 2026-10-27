# archive UI 的外部进程/写操作命令（静态排查，未启动 GUI）

| 命令 | 触发点 | 风险 |
|---|---|---|
| `pick_folder` | projects.js:8、notes.js:237、events.js:386 | 打开系统选择框（窗口） |
| `launch_project` | projects.js:621、1313 | 启动外部进程/终端 |
| `run_task` | projects.js:585、1314 | 执行构建/测试（外部进程） |
| `open_in` | notes.js:229 | 用外部程序打开 |
| `open_path` / `open_url` / `shell` / `open` | events.js 等（按 archive invoke 清单） | 打开路径/URL/命令 |
| `set_default_opener` | events.js:564 | 改系统默认程序 |
| `create_project` | notes.js:239 | 创建目录 |
| `apply_fix` / `apply_edit` | projects.js:1080、editor.js:741 | 写文件 |

**结论**：这些必须 deny-list；shim 对它们返回 `{blocked:true}`，UI 显示“未实现（外部操作已禁用）”。
