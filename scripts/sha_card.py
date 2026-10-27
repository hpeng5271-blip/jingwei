#!/usr/bin/env python
"""card 输出的可复现 SHA：subprocess 直接捕获原始 bytes -> sha256。

为什么不用 PowerShell Out-File：Out-File -Encoding utf8 会带 BOM，
同内容不同字节 => 假红（2026-10-27 事故）。**所有 SHA 一律走本脚本。**

用法：
  python scripts/sha_card.py --repo D:\\projects\\repos\\rg --card crates/core/flags/defs.rs [--json]
输出：<sha256>（同一命令连跑两次必须一致，否则 exit 1）
"""
import argparse
import hashlib
import subprocess
import sys
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JW = os.environ.get("JW") or os.path.join(ROOT, "target", "debug", "jw.exe")


def one(repo, card, as_json):
    args = [JW, "card", card]
    if as_json:
        args.append("--json")
    args.append(".")
    r = subprocess.run(args, cwd=repo, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    if r.returncode != 0:
        raise SystemExit("jw card failed: %s" % r.returncode)
    return hashlib.sha256(r.stdout).hexdigest().upper()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True)
    ap.add_argument("--card", required=True)
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    h1 = one(a.repo, a.card, a.json)
    h2 = one(a.repo, a.card, a.json)
    if h1 != h2:
        print("UNSTABLE %s %s" % (h1, h2))
        return 1
    print(h1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
