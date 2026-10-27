// 数据核心面板（调试用）：把 core 的只读结果画出来，不参与主流程。
//
// 为什么不挂到主导航：这是"给自己看账本"的入口，不是产品界面的一部分。
// 打开方式：控制台 `openCorePanel()`（或以后加调试开关）。
(function () {
  function el(tag, cls, text) {
    const d = document.createElement(tag);
    if (cls) d.className = cls;
    if (text != null) d.textContent = text;
    return d;
  }

  async function openCorePanel() {
    let box = document.getElementById('corePanel');
    if (!box) {
      box = el('div', 'core-panel');
      box.id = 'corePanel';
      box.style.cssText =
        'position:fixed;right:16px;bottom:16px;width:420px;max-height:60vh;overflow:auto;' +
        'background:#1b1b1f;color:#ddd;border:1px solid #444;border-radius:8px;padding:12px;' +
        'font:12px/1.5 ui-monospace,Consolas,monospace;z-index:9999;white-space:pre-wrap';
      document.body.appendChild(box);
    }
    box.textContent = '数据核心：读取中…';
    if (!window.CoreData) {
      box.textContent = '数据核心：没接上（window.CoreData 不存在）';
      return box;
    }
    try {
      const [live, ver, cap] = await Promise.all([
        window.CoreData.live(),
        window.CoreData.verify(),
        window.CoreData.capability(),
      ]);
      const l = window.CoreData.unpack(live);
      const v = ver || {};
      const c = window.CoreData.unpack(cap);
      const lines = [
        '# 数据核心（只读）',
        `live: ${l.state} · ${l.items.length} 条`,
        `verify: 无支撑 ${v.unsupported ?? '?'} · 悬空 ${v.dangling_obs ?? '?'}/${v.dangling_result ?? '?'}/${v.dangling_input ?? '?'}`,
        `       缺 derive ${v.missing_derive ?? '?'} · 无归因回退 ${v.unexplained_fallback ?? '?'}`,
        `capability: ${c.state} · ${c.items.length} 条`,
      ];
      for (const it of c.items) lines.push(`  - ${it.subject}: ${it.why}`);
      box.textContent = lines.join('\n');
    } catch (e) {
      box.textContent = '数据核心：读失败 ' + (e && e.message ? e.message : String(e));
    }
    return box;
  }

  function closeCorePanel() {
    const box = document.getElementById('corePanel');
    if (box) box.remove();
  }

  window.openCorePanel = openCorePanel;
  window.closeCorePanel = closeCorePanel;
})();
