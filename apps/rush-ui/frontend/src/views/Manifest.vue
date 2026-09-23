<script setup>
import { ref } from 'vue';
import { call } from '../api.js';

const flavor = ref('Proto');
const running = ref(false);
const report = ref(null);
const rebuilt = ref(null);
const lastAction = ref('');

async function run(action) {
  if (!repoPathCheck()) return;
  running.value = true;
  lastAction.value = action;
  rebuilt.value = null;
  report.value = null;
  try {
    if (action === 'check') {
      report.value = await call('manifest_check', { repo: repoPath.value, flavor: flavor.value });
    } else {
      rebuilt.value = await call('manifest_rebuild', { repo: repoPath.value, flavor: flavor.value });
    }
  } catch (error) {
    report.value = { __error: String(error) };
  } finally {
    running.value = false;
  }
}

function repoPathCheck() {
  if (!repoPath.value) {
    report.value = { __error: '仓库根目录必填（右上角）' };
    return false;
  }
  return true;
}
</script>

<template>
  <a-space direction="vertical" style="width: 100%" :size="14">
    <a-form layout="vertical" style="max-width: 420px">
      <a-form-item label="同步面">
        <a-select v-model:value="flavor">
          <a-select-option value="Proto">proto 契约面</a-select-option>
          <a-select-option value="React">react 前端快照面</a-select-option>
        </a-select>
      </a-form-item>
    </a-form>
    <div class="step-footer">
      <a-space>
        <a-button type="primary" :loading="running" @click="run('check')">校验</a-button>
        <a-button :loading="running" @click="run('rebuild')">重建基线</a-button>
      </a-space>
      <span class="path-hint">校验对行序不敏感；重建以当前树为基线。</span>
    </div>

    <a-alert
      v-if="report && report.__error"
      type="error"
      show-icon
      :message="report.__error"
      style="white-space: pre-wrap"
    />
    <a-alert v-else-if="report && lastAction === 'check' && report.is_ok" type="success" show-icon message="清单与树一致" />
    <a-card v-else-if="report && lastAction === 'check'" size="small" title="清单不一致">
      <ul class="file-list">
        <li v-for="item in report.added" :key="'a' + item" class="mono">+ {{ item }}（树上有，清单没有）</li>
        <li v-for="item in report.removed" :key="'r' + item" class="mono">- {{ item }}（清单有，树上没有）</li>
        <li v-for="item in report.modified" :key="'m' + item" class="mono">~ {{ item }}（内容不符）</li>
      </ul>
    </a-card>
    <a-alert v-else-if="rebuilt !== null" type="success" show-icon :message="`已重建基线（${rebuilt} 条）`" />
  </a-space>
</template>

<style scoped>
.file-list {
  margin: 0;
  padding-left: 20px;
  font-size: 12px;
  word-break: break-all;
}
</style>
