<script setup>
import { computed } from 'vue';

const props = defineProps({
  report: { type: Object, default: null },
  dryRun: { type: Boolean, default: false },
  showCheck: { type: Boolean, default: false },
});

const created = computed(() => props.report?.created || report_files.value);
// eslint-disable-next-line vue/no-dupe-keys
const report_files = computed(() => props.report?.files || []);
const edited = computed(() => props.report?.edited || []);
const updated = computed(() => props.report?.updated || []);
const skipped = computed(() => props.report?.skipped || []);
const notes = computed(() => props.report?.notes || []);
const diffs = computed(() => props.report?.diffs || []);
const raw = computed(() => JSON.stringify(props.report, null, 2));
const totalCount = computed(
  () => created.value.length + updated.value.length + edited.value.length + report_files.value.length,
);
</script>

<template>
  <div v-if="report">
    <a-alert
      :type="dryRun ? 'warning' : 'success'"
      :message="dryRun ? 'dry-run 预览（未落盘）' : '完成'"
      show-icon
      class="mb"
    >
      <template #description>
        新建/生成 {{ created.length || report_files.length }} · 覆盖 {{ updated.length }} · 编辑
        {{ edited.length }} · 跳过 {{ skipped.length }}
        <a-tag v-if="showCheck && checkPassed === true" color="success">cargo check 通过</a-tag>
        <a-tag v-else-if="showCheck && checkPassed === false" color="error">cargo check 未通过</a-tag>
      </template>
    </a-alert>

    <a-card v-if="created.length || report_files.length" size="small" class="mb">
      <template #title>新建/生成文件</template>
      <ul class="file-list">
        <li v-for="item in created.length ? created : report_files" :key="item" class="mono">+ {{ item }}</li>
      </ul>
    </a-card>
    <a-card v-if="updated.length" size="small" class="mb">
      <template #title>覆盖文件</template>
      <ul class="file-list">
        <li v-for="item in updated" :key="item" class="mono">↻ {{ item }}</li>
      </ul>
    </a-card>
    <a-card v-if="edited.length" size="small" class="mb">
      <template #title>编辑文件</template>
      <ul class="file-list">
        <li v-for="item in edited" :key="item" class="mono">~ {{ item }}</li>
      </ul>
    </a-card>
    <a-card v-if="diffs.length" size="small" class="mb">
      <template #title>变更预览</template>
      <pre v-for="(entry, index) in diffs" :key="index" class="diff">{{ entry[1] }}</pre>
    </a-card>
    <a-card v-if="skipped.length" size="small" class="mb">
      <template #title>跳过</template>
      <ul class="file-list">
        <li v-for="item in skipped" :key="item">= {{ item }}</li>
      </ul>
    </a-card>
    <a-card v-if="notes.length" size="small" class="mb">
      <template #title>注意事项</template>
      <ul class="note-list">
        <li v-for="item in notes" :key="item">{{ item }}</li>
      </ul>
    </a-card>

    <details class="raw">
      <summary>原始 JSON</summary>
      <pre>{{ raw }}</pre>
    </details>
  </div>
</template>

<style scoped>
.mb {
  margin-bottom: 10px;
}

.file-list {
  margin: 0;
  padding-left: 20px;
  font-size: 12px;
  word-break: break-all;
}

.note-list {
  margin: 0;
  padding-left: 20px;
  font-size: 13px;
}

.diff {
  background: #0f1115;
  color: #d6deeb;
  border-radius: 6px;
  padding: 10px;
  font-size: 12px;
  overflow: auto;
  max-height: 240px;
  white-space: pre-wrap;
  word-break: break-all;
}

.raw {
  margin-top: 4px;
}

.raw summary {
  cursor: pointer;
  color: rgba(0, 0, 0, 0.45);
  font-size: 12px;
}

.raw pre {
  background: #0f1115;
  color: #d6deeb;
  border-radius: 8px;
  padding: 12px;
  max-height: 260px;
  overflow: auto;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-all;
}
</style>
