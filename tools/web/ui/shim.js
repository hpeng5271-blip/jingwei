// shim.js：把 archive UI 的 Tauri invoke/listen 映射到 jw 的 HTTP /rpc（不引入 Tauri）。
// 命令映射表见 docs/reference/ui-rpc-map.md；未知命令返回 null 并在控制台留痕。
(function () {
  const rpc = async (action, payload) => {
    const r = await fetch('/rpc', { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify(Object.assign({ action }, payload || {})) });
    return await r.json();
  };
  const DENY = new Set(['open_path','open_in','open_url','shell','open','pick_folder','launch_project',
    'run_task','set_default_opener','create_project','apply_fix','apply_edit','save_note','delete_note',
    'contract_save','profile_save','scan_start','scan_cancel','apply_edit']);
  const MAP = {
    list_projects: () => rpc('projects', {}),
    scan_status: () => rpc('projects', {}),
    default_parent: () => rpc('projects', {}).then((r) => ({ parent: r.parent || '' })),
    list_files: (a) => rpc('files', { path: a?.path || '' }),
    open_file: (a) => rpc('read_file', { path: a?.path || '' }),
    engine_check: () => rpc('check', {}),
    engine_machine: () => rpc('machine', {}),
    engine_stats: () => rpc('stats', {}),
    engine_projects: () => rpc('projects', {}),
    engine_files: (a) => rpc('files', { path: a?.path || '' }),
    engine_read_file: (a) => rpc('read_file', { path: a?.path || '' }),
    engine_cite: (a) => rpc('cite', { subject: a?.subject || '' }),
    engine_impact: (a) => rpc('impact', { target: a?.target || '' }),
    engine_card: (a) => rpc('card', { path: a?.input, history: !!a?.history }),
    engine_edges: (a) => rpc('edges', { path: a?.path || '' }),
    app_root: () => Promise.resolve({ state: 'yes', data: 'D:/projects/jingwei' }),
    engine_pick_root: () => Promise.resolve({ state: 'yes', data: 'D:/projects/jingwei' }),
    initial_tab: () => 'home',
    initial_project: (a) => a?.root || '',
    cli_json: (a) => rpc('cli', { args: a?.args || [] }),
    data_dir: () => '',
  };
  async function invoke(cmd, args) {
    if (DENY.has(cmd)) {
      window.__JW_DENIED__ = window.__JW_DENIED__ || [];
      window.__JW_DENIED__.push(cmd);
      let b = document.getElementById('denied-banner');
      if (!b) { b = document.createElement('div'); b.id = 'denied-banner';
        b.style.cssText = 'position:fixed;right:8px;bottom:34px;padding:4px 8px;background:#5a1a1a;color:#ffb4b4;font:12px sans-serif;border-radius:4px;z-index:9999';
        document.body.appendChild(b); }
      b.textContent = '未实现（外部操作已禁用）: ' + window.__JW_DENIED__.slice(-4).join(', ');
      console.warn('[jw shim] denied:', cmd);
      return { blocked: true };
    }
    const fn = MAP[cmd];
    if (!fn) {
      window.__JW_UNMAPPED__ = window.__JW_UNMAPPED__ || [];
      window.__JW_UNMAPPED__.push(cmd);
      let b = document.getElementById('unmapped-banner');
      if (!b) { b = document.createElement('div'); b.id = 'unmapped-banner';
        b.style.cssText = 'position:fixed;right:8px;bottom:8px;padding:4px 8px;background:#5a3a00;color:#ffd479;font:12px sans-serif;border-radius:4px;z-index:9999';
        document.body.appendChild(b); }
      b.textContent = '未映射（M15）: ' + window.__JW_UNMAPPED__.slice(-5).join(', ');
      console.warn('[jw shim] unmapped command:', cmd);
      return null;
    }
    const r = await fn(args || {});
    if (r && r.result !== undefined) return r.result;
    return r;
  }
  const listen = async () => () => {};
  window.__TAURI__ = { core: { invoke }, event: { listen, emit: async () => {} }, invoke };
  window.__JW_SHIM__ = { invoke, MAP };
})();
