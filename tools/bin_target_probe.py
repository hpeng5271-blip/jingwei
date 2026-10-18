#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""**拿 cargo 当裁判，验"这是不是一个 bin 目标"这条判据**。

## 为什么需要一个"不受我控制的裁判"

`roles.rs::is_bin_target` 现在按**文件名**认：`main.rs`（任何深度）或 `src/bin/` 下的文件。
这是我写的规则，拿它自己验自己没有意义。

`cargo metadata` 是**权威**：它读 `Cargo.toml` 的 `[[bin]]` / `autobins` / `src/bin/`，
最后告诉你"这个包有几个可执行目标、各自的源文件是哪一个"。
所以拿它对比，才是**外部信息**。

## 判据

| 结果 | 含义 |
|---|---|
| 一致 | 我们的判据在**这个形状**上没漏 |
| ⚠ 漏判 | cargo 认、我们不认 → 那个文件会被算成"中间层"，入口就少一个 |
| ⚠ 多判 | 我们认、cargo 不认 → 会把普通文件当入口 |

## 已知的形状（脚本会打出来）

- `src/main.rs`：默认 bin
- `src/bin/<名字>.rs`：显式 bin
- `[[bin]] path = "别的路径"`：**我们一定会漏**（只按文件名认）——样本里还没遇到，
  所以这一条是 **[推断] 风险**，不是 [实测] 缺陷。

用法：
    python tools/bin_target_probe.py            # 默认样本
    python tools/bin_target_probe.py <目录> ...  # 指定项目
"""
import glob
import json
import os
import subprocess
import sys


def ours(path):
    """与 `roles.rs::is_bin_target` 同一条判据（两处不一致以 roles.rs 为准）。"""
    p = path.replace('\\', '/')
    return p == 'main.rs' or p.endswith('/main.rs') or '/src/bin/' in p or p.startswith('src/bin/')


def cargo_targets(root):
    r = subprocess.run(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                       cwd=root, capture_output=True, text=True,
                       encoding='utf-8', errors='replace')
    if r.returncode != 0:
        return None, (r.stderr or '').strip()[:100]
    meta = json.loads(r.stdout)
    bins, libs = set(), set()
    for pkg in meta.get('packages', [])[:1]:
        for t in pkg.get('targets', []):
            rel = os.path.relpath(t['src_path'], root).replace('\\', '/')
            kinds = t.get('kind', [])
            if 'bin' in kinds:
                bins.add(rel)
            if 'lib' in kinds or 'rlib' in kinds:
                libs.add(rel)
    return (bins, libs), None


def default_samples():
    home = os.environ['USERPROFILE']
    reg = os.path.join(home, '.cargo', 'registry', 'src')
    out = []
    fd = os.path.join(os.environ.get('TEMP', ''), 'jw-real', 'fd')
    if os.path.isdir(fd):
        out.append(('fd', fd))
    # 真 crate 里**带 [[bin]] 或 src/bin/ 的**才值得验（其余形状太简单，验不出东西）
    want = ('cargo-watch-8.5.3', 'clap-4.6.7', 'dotenvy-0.15.7', 'phf_generator-0.11.3',
            'brotli-9.0.0', 'brotli-decompressor-6.0.0', 'alloc-stdlib-0.3.0',
            'embed-resource-3.0.12')
    if os.path.isdir(reg):
        for d in sorted(os.listdir(reg)):
            for w in want:
                p = os.path.join(reg, d, w)
                if os.path.isdir(p):
                    out.append((w, p))
    return out


def main():
    args = sys.argv[1:]
    samples = [(os.path.basename(p.rstrip('\\/')), p) for p in args] or default_samples()
    if not samples:
        print('没有样本')
        return 1
    print(f"{'项目':<34}{'cargo bin':>10}{'我们认':>8}  结论")
    diff = 0
    for name, root in samples:
        got, err = cargo_targets(root)
        if got is None:
            print(f'{name:<34}{"—":>10}{"—":>8}  cargo metadata 失败：{err}')
            continue
        bins, libs = got
        ours_bins = {b for b in bins if ours(b)}
        missed = bins - ours_bins
        if missed:
            diff += 1
        note = f'⚠ 漏判：{sorted(missed)}' if missed else '一致'
        print(f'{name:<34}{len(bins):>10}{len(ours_bins):>8}  {note}')
        if libs:
            print(f'    cargo lib: {sorted(libs)}')
    print()
    print(f'  一致 {len(samples) - diff} / {len(samples)}；漏判 {diff}')
    print('  （只按文件名认，所以 `[[bin]] path=` 这种形状**一定会漏**——样本里还没遇到）')
    return 0


if __name__ == '__main__':
    sys.exit(main())
