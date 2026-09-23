<script setup>
import { ref } from 'vue';
import ReportView from '../components/ReportView.vue';
import { call, repoPath } from '../api.js';

const dryRun = ref(true);
const keepGates = ref(false);
const prune = ref(false);
const keepScripts = ref(false);
const running = ref(false);
const report = ref(null);
const lastDryRun = ref(true);

function payload() {
  if (!repoPath.value) throw new Error('仓库根目录必填（右上角）');
  return {
    repo_root: repoPath.value,
    dry_run: dryRun.value,
    keep_gates: keepGates.value,
    skip_proto: false,
    skip_react: false,
    prune_upstream_baseline: prune.value,
    keep_sync_scripts: keepScripts.value,
  };
}

async function run() {
  let opts;
  try {
    opts = payload();
  } catch (error) {
    report.value = { __error: String(error.message || error) };
    return;
  }
  running.value = true;
  try {
    report.value = await call('adopt', { opts });
    lastDryRun.value = dryRun.value;
  } catch (error) {
    report.value = { __error: String(error) };
  } finally {
    running.value = false;
  }
}
</script>

<template>
  <a-space direction="vertical" style="width: 100%" :size="14">
    <a-alert
      type="info"
      show-icon
      message="把 rushwind-admin 快照从上游镜像转为下游自有仓：重建双 MANIFEST 基线 + 剥离 CI 同步门禁步。"
    />
    <a-space wrap :size="18">
      <a-checkbox v-model:checked="dryRun">dry-run 预览</a-checkbox>
      <a-checkbox v-model:checked="keepGates">保留 CI 门禁步（仅重建清单）</a-checkbox>
      <a-checkbox v-model:checked="prune">删除 react.UPSTREAM.sha256</a-checkbox>
      <a-checkbox v-model:checked="keepScripts">保留 sync 脚本（默认退役为 stub）</a-checkbox>
    </a-space>
    <div class="step-footer">
      <a-button type="primary" :loading="running" @click="run">接管</a-button>
    </div>
    <ReportView v-if="report" :report="report" :dry-run="lastDryRun" />
  </a-space>
</template>
