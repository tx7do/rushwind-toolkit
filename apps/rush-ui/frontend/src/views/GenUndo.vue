<script setup>
import { ref } from 'vue';
import ReportView from '../components/ReportView.vue';
import { call, repoPath, SNAKE_RE } from '../api.js';

const name = ref(localStorage.getItem('rushui.undo.name') || '');
const dryRun = ref(true);
const running = ref(false);
const report = ref(null);
const lastDryRun = ref(true);

function payload() {
  if (!repoPath.value) throw new Error('仓库根目录必填（右上角）');
  if (!SNAKE_RE.test(name.value)) throw new Error(`实体名须为 snake_case：${name.value || '（空）'}`);
  return { repo_root: repoPath.value, name: name.value, dry_run: dryRun.value };
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
    report.value = await call('gen_undo', { opts });
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
      type="warning"
      show-icon
      message="按 .rush/<name>.json 反向移除该实体的全部生成物：后端链、五处注册、菜单/路由登记与三栈页面；数据库表不动。幂等，可重复执行。"
    />
    <a-form layout="vertical" style="max-width: 640px">
      <a-form-item label="实体名">
        <a-input v-model:value="name" class="mono" placeholder="widget" />
      </a-form-item>
      <a-checkbox v-model:checked="dryRun">dry-run 预览（先看会删什么）</a-checkbox>
    </a-form>
    <div class="step-footer">
      <a-button danger type="primary" :loading="running" @click="run">回滚</a-button>
    </div>

    <template v-if="report">
      <a-alert v-if="report.__error" type="error" show-icon :message="report.__error" style="white-space: pre-wrap" />
      <ReportView v-else :report="report" :dry-run="lastDryRun" />
    </template>
  </a-space>
</template>
