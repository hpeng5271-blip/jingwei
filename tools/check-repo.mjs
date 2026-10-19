// **仓库体检**（2026-10-20）：推到 GitHub 之前，先回答三个问题——
// 要提交多少个文件、总共多大、有没有**不该进版本库**的东西。
//
// 为什么做成门禁：用户明确提过一次"下次仓库也一并检查文件数量、有没有提交不该提交的"。
// 口头约定会忘，门禁不会。
//
// 判据（都可在下面常量里改，但要有理由）：
//   ① 跟踪文件数 ≤ 400；
//   ② 跟踪内容总大小 ≤ 12 MB；
//   ③ 不许出现构建产物/运行时状态：target/、dist/、node_modules/、.jingwei/、
//      *.zip、*.exe、*.dll、*.pdb、*.db、*.sqlite；
//   ④ `.env` 只允许出现在 `tools/demos/` 下（演示夹具，内容是假值）；
//   ⑤ 单个文件 > 2 MB 直接红；> 1 MB 只报出来（文档配图可以大，但要看得见）。

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';

export const LIMITS = {
  maxFiles: 400,
  maxTotalMB: 12,
  maxSingleMB: 2,
  warnSingleMB: 1,
};

const FORBIDDEN = [
  [/^target\//, '构建产物目录 target/'],
  [/^dist\//, '构建产物目录 dist/'],
  [/node_modules\//, '依赖目录 node_modules/'],
  [/^\.jingwei\//, '工具运行时数据 .jingwei/'],
  [/\.(zip|exe|dll|pdb|db|sqlite)$/i, '二进制/数据库文件'],
];

const ENV_ALLOW = /^tools\/demos\//;

export function checkRepo(root = '.') {
  const problems = [];
  const big = [];
  const out = execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'buffer' });
  const files = out.toString('utf8').split('\0').filter(Boolean);

  let total = 0;
  for (const f of files) {
    let size = 0;
    try {
      size = fs.statSync(`${root}/${f}`).size;
    } catch {
      problems.push(`跟踪了但磁盘上不存在：${f}（工作区不完整？）`);
      continue;
    }
    total += size;
    const mb = size / 1024 / 1024;
    if (mb > LIMITS.maxSingleMB) problems.push(`单文件过大（${mb.toFixed(1)} MB）：${f}`);
    else if (mb > LIMITS.warnSingleMB) big.push(`${mb.toFixed(1)} MB  ${f}`);

    for (const [re, why] of FORBIDDEN) {
      if (re.test(f)) {
        problems.push(`不该进版本库（${why}）：${f}`);
        break;
      }
    }
    if (/(^|\/)\.env(\.|$)/.test(f) && !ENV_ALLOW.test(f)) {
      problems.push(`.env 只允许在 tools/demos/ 下当夹具：${f}`);
    }
  }

  const totalMB = total / 1024 / 1024;
  if (files.length > LIMITS.maxFiles) {
    problems.push(`跟踪文件数 ${files.length} 超过上限 ${LIMITS.maxFiles}（清理或调阈值并写理由）`);
  }
  if (totalMB > LIMITS.maxTotalMB) {
    problems.push(`跟踪内容 ${totalMB.toFixed(1)} MB 超过上限 ${LIMITS.maxTotalMB} MB`);
  }

  return {
    files: files.length,
    totalMB,
    big,
    problems,
    line: `  · 仓库：${files.length} 个跟踪文件 / ${totalMB.toFixed(2)} MB`,
  };
}
