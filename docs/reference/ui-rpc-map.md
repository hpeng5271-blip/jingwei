# UI ↔ RPC 映射（0.4.0 迁移中）

| archive 命令 | jw RPC | 说明 |
|---|---|---|
| `graph_stats` | `stats` | 文件/符号/边计数 |
| `list_projects` / `scan_status` | `projects` | 当前仓库与父目录项目 |
| `list_files` | `files` | 文件列表（待接） |
| `open_file` | `read_file` | 文本内容（待接） |
| `hardware_fast` / `machine_report` | `machine` | OS/CPU/内存 |
| `clean_items` | `check` | 十二项计数 |
| `cli_json` | `cli` | 只读子集（map 内列出的） |
| 其余 | — | 未映射：返回 null + console.warn（不装样子） |

> `graph.js`/canvas/rough 已删除；`graph_dump` 不再提供。
