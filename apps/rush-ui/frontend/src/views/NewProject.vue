<script setup>
import { ref } from 'vue';
import ReportView from '../components/ReportView.vue';
import { call } from '../api.js';

const name = ref(localStorage.getItem('rushui.new.name') || '');
const dir = ref(localStorage.getItem('rushui.new.dir') || '.');
const storage = ref('Memory');
const template = ref('');
const git = ref(true);
const dryRun = ref(true);
const running = ref(false);
const report = ref(null);
const lastDryRun = ref(true);

function payload() {
  if (!name.value.trim()) throw new Error('项目名必填');
  return {
    name: name.value.trim(),
    dest: dir.value.trim() || '.',
    storage: storage.value,
    template: template.value.trim() || null,
    git: git.value,
    dry_run: dryRun.value,
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
    report.value = await call('new_project', { opts });
    lastDryRun.value = dryRun.value;
    localStorage.setItem('rushui.new.name', name.value);
    localStorage.setItem('rushui.new.dir', dir.value);
  } catch (error) {
    report.value = { __error: String(error) };
  } finally {
    running.value = false;
  }
}
</script>

<template>
  <a-space direction="vertical" style="width: 100%" :size="14">
    <a-form layout="vertical" style="max-width: 640px">
      <a-row :gutter="12">
        <a-col :span="12">
          <a-form-item label="项目名（= crate 名 + 目录名）">
            <a-input v-model:value="name" class="mono" placeholder="my-service" />
          </a-form-item>
        </a-col>
        <a-col :span="12">
          <a-form-item label="目标父目录">
            <a-input v-model:value="dir" class="mono" placeholder="." />
          </a-form-item>
        </a-col>
        <a-col :span="12">
          <a-form-item label="存储变体">
            <a-select v-model:value="storage">
              <a-select-option value="Memory">内存（开箱即跑）</a-select-option>
              <a-select-option value="Sqlite">内嵌 SQLite（零安装）</a-select-option>
              <a-select-option value="Postgres">PostgreSQL</a-select-option>
            </a-select>
          </a-form-item>
        </a-col>
        <a-col :span="12">
          <a-form-item label="外部模板目录（可选）">
            <a-input v-model:value="template" class="mono" placeholder="留空用内嵌模板" />
          </a-form-item>
        </a-col>
      </a-row>
      <a-space wrap :size="18">
        <a-checkbox v-model:checked="git">git init</a-checkbox>
        <a-checkbox v-model:checked="dryRun">dry-run 预览</a-checkbox>
      </a-space>
    </a-form>
    <div class="step-footer">
      <a-button type="primary" :loading="running" @click="run">创建</a-button>
    </div>
    <ReportView v-if="report" :report="report" :dry-run="lastDryRun" />
  </a-space>
</template>
