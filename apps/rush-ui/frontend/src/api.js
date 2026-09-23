// invoke 封装 + 共享状态（仓库路径在 Header 输入，各页签读取）。
import { invoke } from '@tauri-apps/api/core';
import { ref } from 'vue';

export function call(command, args = {}) {
  return invoke(command, args);
}

/** Header 里的仓库路径（多页签共享 + localStorage 记忆）。 */
export const repoPath = ref(localStorage.getItem('rushui.repo') || '');

export function setRepoPath(value) {
  repoPath.value = value.trim();
  localStorage.setItem('rushui.repo', repoPath.value);
}

export const repoProbe = ref(null);

export async function probeRepo() {
  const repo = repoPath.value;
  if (!repo) {
    repoProbe.value = null;
    return;
  }
  try {
    repoProbe.value = await call('probe_repo', { repo });
  } catch {
    repoProbe.value = null;
  }
}

/** 报告 → 结构化文件清单卡片数据（各页签共用）。 */
export function reportSummary(report) {
  return {
    created: report.created || report.files || [],
    edited: report.edited || [],
    skipped: report.skipped || [],
    notes: report.notes || [],
    updated: report.updated || [],
    diffs: report.diffs || [],
    checkPassed: report.check_passed ?? null,
  };
}

export const SNAKE_RE = /^[a-z][a-z0-9_]*$/;
export const UPPER_RE = /^[A-Z][A-Z0-9_]*$/;
