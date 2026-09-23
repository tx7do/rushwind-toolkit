<script setup>
import { ref, onMounted } from 'vue';
import { call, repoPath } from '../api.js';

const report = ref(null);
const running = ref(false);

async function run() {
  running.value = true;
  try {
    report.value = await call('doctor', { repo: repoPath.value || null });
  } catch (error) {
    report.value = { checks: [], __error: String(error) };
  } finally {
    running.value = false;
  }
}

const statusColor = { Ok: 'success', Warn: 'warning', Fail: 'error' };
const statusMark = { Ok: '✓', Warn: '⚠', Fail: '✗' };
const failures = () => (report.value?.checks || []).filter((check) => check.status === 'Fail').length;

onMounted(run);
</script>

<template>
  <a-space direction="vertical" style="width: 100%" :size="14">
    <a-space>
      <a-button type="primary" :loading="running" @click="run">体检</a-button>
      <span class="path-hint">
        工具链（cargo / rustfmt / buf / node / docker…）{{ repoPath ? '；含仓形状与清单一致性' : '' }}。有 ✗ 时退出码非零（可进 CI）。
      </span>
    </a-space>

    <a-alert
      v-if="report"
      :type="failures() ? 'error' : 'success'"
      :message="failures() ? `${failures()} 项失败` : '全部通过'"
      show-icon
    />

    <a-list v-if="report" :data-source="report.checks || []" size="small" bordered>
      <template #renderItem="{ item }">
        <a-list-item>
          <a-space align="start" style="width: 100%">
            <a-tag :color="statusColor[item.status]" style="min-width: 28px; text-align: center">
              {{ statusMark[item.status] }}
            </a-tag>
            <div>
              <strong>{{ item.name }}</strong>
              <span style="margin-left: 8px">{{ item.detail }}</span>
              <div v-if="item.hint" class="path-hint">↳ {{ item.hint }}</div>
            </div>
          </a-space>
        </a-list-item>
      </template>
    </a-list>
  </a-space>
</template>
