//! 开发工具：`--report <路径>` 把关键 DOM 元素的**实际状态**写进一个文件。
//!
//! ## 它是拿什么换来的
//!
//! 2026-10-16 用 `--tab graph` 深链接拉起窗口，截图里图谱页**只有画布、没有浮层卡片**。
//! 而外壳测试是过得去的——因为它读的是 `textContent`，**元素隐藏时照样读得到**。
//! 也就是说：那一整类"元素在不在、看不看得见、多大、画在哪"的问题，
//! 测试**一条都验不了**，只能靠人拉窗口看——而人看容易漏、也没法回归。
//!
//! 加了这个之后，同一个问题一眼看清：`#ghSub` 停在初始文案「正在读取…」、
//! `#ghRuns` 是 `hidden`——于是根因（切页与打开项目并行，`S.root` 还没赋值）当场现形。
//!
//! ## 它是工具，不是产品功能
//!
//! 只有显式传 `--report` 才跑；不传时这一段代码一次都不执行。

use jingwei_engine::envelope::Envelope;
use serde_json::Value;
use tauri::{AppHandle, Manager};

/// `--report <路径>`：启动后把关键 DOM 元素的**实际状态**写进一个文件。
///
/// ## 为什么要它
///
/// 2026-10-16 用 `--tab graph` 深链接拉起窗口截图，发现图谱页**只有画布、没有浮层卡片**。
/// 而外壳测试是过得去的——因为它读的是 `textContent`，**元素隐藏时照样读得到**。
/// 也就是说：那一整类"元素在不在、看不看得见、多大、画在哪"的问题，
/// 现有测试**一条都验不了**，只能靠人拉窗口看——而人看容易漏、也没法回归。
///
/// 这个东西让**程序自己把状态报出来**：元素有没有、可不可见、几何多大、
/// 计算出来的 `display` / `max-width` 是什么。有了它，同一件事就能自动比对。
///
/// 它是**开发工具**，不是产品功能：只有显式传 `--report` 才会跑。
#[tauri::command]
pub(crate) fn save_report(path: String, json: String) -> Value {
    match std::fs::write(&path, json) {
        Ok(()) => Envelope::yes(path).to_value(),
        Err(e) => Envelope::<Value>::unknown(format!("自检结果写不进去：{e}")).to_value(),
    }
}

// ── 已撤掉：`--shot`（截图，2026-10-16）──────────────────────────────
//
// 曾经加过：注入脚本里 `canvas.toDataURL()` → Rust 落盘 PNG（含一段手写 base64 解码）。
// **代码是好的，但它从来没被接通过**——`--shot` 的参数没走到注入脚本里，
// 而**这个进程里没有任何输出通道能看见为什么**：
// 这个 crate 是 **Windows GUI 子系统**，`eprintln!` 的 stderr 不存在（输出被静默丢掉），
// `dlog` 也不落盘。**看不见叠着看不见**，只能"改一行、重跑一遍"，试了 7 次没收敛。
//
// 按 SOP「失败 2 次就停」，**整条撤掉**（含 `save_shot` 命令与 base64 解码）。
// 撤干净而不是留一个没有调用方的函数：**留着会让人以为有截图这条路**——
// 这个项目为"看起来有、其实没有"付过太多次账（见 `事故账本.md`）。
//
// **下次要做它，先做这两件**（顺序不能反）：
//   ① **先建诊断通道**：GUI 子系统里 stderr 与 dlog 都可能没有，
//      唯一实测能用的是 `save_report` 那条 invoke 通道 ——
//      所以**第一件事是让"截图状态"写进报告**，而不是先写截图；
//   ② 或者走 Tauri 自己的 window 截图 API（不依赖页面里的 JS）。
//
// 需要那段实现时，从 git 历史取（`save_shot` + `decode_b64`）。

pub(crate) fn start_report(handle: AppHandle, path: String, sweep: Vec<String>) {
    std::thread::spawn(move || {
        // **等一下再注入**：`eval` 打在还没加载完的页面上会落空。
        // 这是"第一次注入"的等待，不是"等页面就绪"——后者由脚本自己轮询。
        std::thread::sleep(std::time::Duration::from_millis(1500));
        // 逐页扫（`--sweep`）。空数组 = 只报告当前这一页。
        let list = sweep
            .iter()
            .map(|t| format!("{t:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        // `handle.eval` **拿不到脚本的返回值**，所以让脚本把结果回调进来。
        let js = format!(
            r#"
        (async function () {{
          const pick = (sel) => {{
            const el = document.querySelector(sel);
            if (!el) return {{ sel, exists: false }};
            const cs = getComputedStyle(el);
            const r = el.getBoundingClientRect();
            return {{
              sel,
              exists: true,
              hidden: el.hasAttribute('hidden'),
              display: cs.display,
              visibility: cs.visibility,
              opacity: cs.opacity,
              maxWidth: cs.maxWidth,
              rect: [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)],
              // **用 `innerText` 而不是 `textContent`**（2026-10-17 改）。
            //
            // `textContent` 把排版**拍平**：`<b>卡在哪</b><span>1</span><div>1 个依赖没装</div>`
            // 读出来是 `"卡在哪11 个依赖没装"`——**序号徽章与正文粘在一起**，
            // 于是"1 个依赖没装"看起来像"11 个"。我因此在第 40 轮差点记下一个不存在的 bug。
            // `innerText` 尊重布局，块级元素之间会带换行。
            // **两个都报，别只报一个**（2026-10-17）：
            //
            // `textContent` **完整**（连 `<details>` 收起的内容也算），但**没有排版**——
            // 序号徽章 `1` 与正文"1 个依赖没装"会粘成 "11 个"。
            // `innerText` **尊重布局**（块之间带换行），但**不含被折叠的内容**——
            // `#audit` 的条目就在 `<details>` 里，只读 `innerText` 会以为"没内容"。
            //
            // 所以：**判断"有没有"读 `text`，判断"怎么排的"读 `visible`。**
            // 上一版我把 `text` 换成了 `innerText`，当场把界面门禁的一条断言弄红了——
            // **红得跟产品无关**，又一次是我的量具。
            text: (el.textContent || '').trim().slice(0, 2000),
            visible: (el.innerText || el.textContent || '').trim().slice(0, 2000),
            // **结构也给一份**：直接子元素各自的内容分开列。
            // 需要"这是第几条 / 徽章是什么"的时候，读这个而不是拍平的 text。
            parts: Array.from(el.children)
              .map((c) => (c.innerText || c.textContent || '').trim())
              .filter((x) => x.length > 0)
              .slice(0, 20),
            }};
          }};
          // 每页都看这几个：主内容区、它的卡片、以及"有没有东西画出来"。
          //
          // **布局容器是 2026-10-16 加的**，理由值得记：
          // 界面门禁报"画布被挤出窗口"，而**算不出来**——
          // `100vh` = 996、`--top-h` = 66、`--bar-h` = 30，算式给的 900px
          // 却渲染在 y=155（本该 y=66）。**算不出来就得量出来。**
          // 这三个不是"设计要的"，是"为了查一个具体缺陷"加的；
          // 查完可以删——**但它现在拦得住下一次，所以先留着**。
          // **2026-10-16 撤掉了三项：`#ghDeps` / `#ghDepsList` / `#ghCard`。**
          //
          // 它们原来在这里，是那批"图谱页工作"的一部分——而那批工作
          // 在一次 `git checkout -- ui/` 里丢了（丢之前没提交，见 AGENTS.md §5.2）。
          // **没实现的东西留在巡查清单里，是最坏的那种判据**：它会如实报
          // `exists:false`，但**没有任何断言在读它** → 门禁照样绿。
          // 一条几乎什么都放过的判据比没有更坏，因为它看上去在守。
          //
          // 所以现在**只在实现时再加回来**，而且加回一项的同时必须有断言读它。
          // `#ghRuns` 是范本：它在清单里，门禁有 4 条断言钉它（列表/失败标记/明细/三态）。
          const SEL = ['#view-home', '#guideCard', '#statusCard', '#cardNotes',
                       // **「验一下能不能构建」**：按钮要在、结果区要在（2026-10-18）。
                       '#verifyBtn', '#verifyHint', '#verifyResult', '#view-graph', '#graphCv',
                       '#graphHud', '#ghSub', '#ghRoles', '#ghDirs', '#ghRuns',
                       // `#cmdRun` = 运行那一步实际要敲的命令。**它必须与首页指南的
                       // `nextStep` 说的是同一件事**——不然首页让人敲 `cargo run`、
                       // 而这里的按钮跑的是另一个东西，用户就不知道该信哪个。
                       '#view-run', '#cmdRun', '#view-editor', '#edFiles',
                       '#view-note', '#view-machine',
                       // **机器页里"条目真的画出来了吗"**（2026-10-17 加）。
                       //
                       // 原来只有 `#view-machine`——也就是**只验了"这一页可见"**，
                       // 没验里面的东西。这与账本里那条
                       // 「界面门禁'等页面就绪'只兑现了一半」是同一个形状：
                       // 视图在了、内容可能是空的，而门禁照样绿。
                       //
                       // 我新加的两条 Rust 规则（`cargo-multi-bin-no-default` /
                       // `cargo-edition-missing`）在界面上有没有渲染，**此前没有任何东西在看**。
                       '#probList', '#audit', '#view-clean',
                       'body', 'main', '.view.on', '.graph-wrap'];
          const snapshot = (tab) => ({{
            tab,
            viewport: [window.innerWidth, window.innerHeight],
            activeView: (document.querySelector('.view.on') || {{}}).id || null,
            navOn: (document.querySelector('#nav button.on') || {{}}).dataset
              ? document.querySelector('#nav button.on').dataset.tab : null,
            items: SEL.map(pick),
          }});

          // 等"页面就绪"，别盲等：主内容区真的画出来了就算就绪。
          const ready = (tab) => {{
            const v = document.getElementById('view-' + tab);
            if (!v || !v.classList.contains('on')) return false;
            const g = document.getElementById('ghSub');
            if (tab === 'graph' && g) return !/正在读取/.test(g.textContent || '');
            const r = v.getBoundingClientRect();
            return r.width > 0 && r.height > 0;
          }};
          const rest = (ms) => new Promise((r) => setTimeout(r, ms));
          const waitReady = async (tab, capMs) => {{
            const t0 = Date.now();
            while (Date.now() - t0 < capMs) {{
              if (ready(tab)) return true;
              await rest(120);
            }}
            return false;
          }};

          const tabs = [{list}];
          if (!tabs.length) {{
            // **单页路径也要等"这一页就绪"，不能盲等。**
            //
            // 这里原来是：1500ms 之后直接拍一张。而"1500ms"对
            // `--tab graph` 是**不够的**——图谱页要等 `graph_stats` 回来、
            // 要等力导向收敛，那可能超过 1.5 秒。于是报告拍到的是
            // 「— 节点 · — 边 / 正在读取…」，而**报告只写一次**，
            // 那个"还没画好"的状态就永久留在文件里。
            //
            // 症状极具欺骗性：**门禁读到"正在读取…"，看起来像产品坏了，
            // 其实是取证工具拍早了**。实测：同一条命令第二次跑就全过（时序运气）。
            // 而一个"跑两次里过一次"的判据**不是判据**。
            const cur = (document.querySelector('.view.on') || {{}}).id || '';
            const tab = cur.replace(/^view-/, '');
            // ⚠️ **没有活动页就先别拍**（2026-10-16 修，真缺陷）。
            //
            // 原来这里是 `if (tab) await waitReady(tab, 8000);`——
            // 页面还没切到任何一页时 `tab` 是空串，于是**整句跳过、立刻拍**，
            // 拍到的就是「— 节点 · — 边 / 正在读取…」。而**报告只写一次**，
            // 那个"还没画好"的状态就永久留在文件里，门禁读到它就判产品坏了。
            //
            // 深链接（`--tab graph`）现在要等 `initial_project` 回来才切页，
            // 所以 1.5 秒的固定等待**不够稳**。改成两步：先等"有活动页"，
            // 再等"这页就绪"，各有上限。**这是取证工具修自己，不是产品修。**
            const waitAnyView = async (capMs) => {{
              const t0 = Date.now();
              while (Date.now() - t0 < capMs) {{
                const v = document.querySelector('.view.on');
                if (v && v.id) return v.id.replace(/^view-/, '');
                await rest(100);
              }}
              return '';
            }};
            const realTab = tab || (await waitAnyView(10000)); // 空串 = 10 秒内都没有活动页
            // **上限要晚于看门狗的 20 秒**（2026-10-18 抓到抖动后改）：
            // 图谱页数据卡住时，`ui/graph.js` 的看门狗会在 20 秒把「正在读取…」
            // 改成「读不到（20 秒还没回来）」。
            // 而这里原来只等 12 秒就拍——**拍得比改得早**，拍到的仍是「正在读取…」，
            // 而报告只写一次 → 门禁拿到一份"永远没准备好"的报告，干等 90 秒。
            // 现在等 22 秒：要么数据回来了、要么看门狗已经改成"读不到"（那也是终端状态）。
            if (realTab) await waitReady(realTab, 22000);
            window.__TAURI__.core.invoke('save_report',
              {{ path: {path:?}, json: JSON.stringify([snapshot(realTab)]) }});
            return;
          }}

          const out = [];
          for (const tab of tabs) {{
            // **点真的导航按钮**：走用户那条路（按钮的 dataset + 事件处理器都过一遍）。
            // 直接调 `switchTab` 会漏掉"按钮本身没接上"这类问题。
            const btn = document.querySelector('#nav button[data-tab="' + tab + '"]');
            if (btn) btn.click();
            else if (typeof switchTab === 'function') switchTab(tab);
            // **图谱页要等数据回来，3 秒不够**（2026-10-18）：卡住时看门狗 20 秒
            // 才把「正在读取…」改成「读不到」，所以这里给 22 秒——要么数据到了，
            // 要么已经是终态。**拍早了，那份报告就永远停在"没准备好"。**
            await waitReady(tab, tab === "graph" ? 22000 : 3000);
            out.push(snapshot(tab));
          }}
          // 扫完回到第一页——**别把窗口留在最后一页**，那不是它启动时的样子。
          const first = document.querySelector('#nav button[data-tab="' + tabs[0] + '"]');
          if (first) first.click();
          window.__TAURI__.core.invoke('save_report',
            {{ path: {path:?}, json: JSON.stringify(out) }});
        }})()
        "#
        );
        // `eval` 在**窗口**上，不在 AppHandle 上（Tauri v2）。
        let Some(win) = handle.get_webview_window("main") else {
            jingwei_engine::dlog::w("7002", "找不到主窗口，自检跳过");
            return;
        };
        match win.eval(&js) {
            Ok(()) => jingwei_engine::dlog::i("7000", "自检脚本已发出"),
            Err(e) => jingwei_engine::dlog::w("7001", &format!("自检脚本发不出去：{e}")),
        }
    });
}
