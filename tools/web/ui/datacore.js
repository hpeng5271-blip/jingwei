// 数据核心读侧（V1 第一版）：UI 只通过这里看 core，不自己拼参数。
//
// 约定：
// - 只调只读子命令（live/find/search/why/stats）；写入口不在这里。
// - `cli_json` 由 Tauri 侧补 `--root` 和 `--json`，这里只传子命令与参数。
// - 解包统一走 `unpack`：state / summary / items，别在界面里各读各的。
(function () {
  function coreJson(args) {
    const root = (typeof S !== 'undefined' && S.root) ? S.root : null;
    if (!root) return Promise.reject(new Error('没有打开的项目'));
    return window.__TAURI__.core.invoke('cli_json', { root, args: ['core'].concat(args) });
  }

  const CoreData = {
    live: () => coreJson(['live']),
    stats: () => coreJson(['stats']),
    find: (name) => coreJson(['find', String(name)]),
    search: (q) => coreJson(['search', String(q)]),
    why: (factId) => coreJson(['why', String(factId)]),
    verify: () => coreJson(['verify']),
    recompute: (n) => coreJson(['recompute', '--sample', String(n || 20)]),
    capability: () => coreJson(['capability']),
    context: (factId, windowS) =>
      coreJson(['context', String(factId), '--window', String(windowS || 1800)]),

    // 编辑器 buffer → 投影：把"未落盘的内容"交给 core，不写盘。
    projectOverlay: (map) => coreJson(['project', '--overlay-json', JSON.stringify(map || {})]),

    // 统一成 { state, summary, items }；未知一律当 unknown，不许当"没有"。
    unpack(v) {
      const state = (v && v.state) || 'unknown';
      return {
        state,
        summary: (v && (v.summary || v.reason)) || '',
        items: (v && Array.isArray(v.items)) ? v.items : [],
      };
    },
  };

  window.CoreData = CoreData;
})();
