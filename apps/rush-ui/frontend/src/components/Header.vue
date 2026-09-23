<script setup>
import { onMounted } from 'vue';
import { repoPath, setRepoPath, repoProbe, probeRepo } from '../api.js';

function onChange(event) {
  setRepoPath(event.target.value);
  probeRepo();
}

onMounted(probeRepo);
</script>

<template>
  <div class="header-bar">
    <img src="/logo.png" class="logo" alt="logo" />
    <div class="title-block">
      <div class="title">RushWind Toolkit</div>
      <div class="subtitle">同进程直调 rush-gen —— 接管 · 清单 · 脚手架 · 实体链 · 三栈页面</div>
    </div>
    <div class="repo-box">
      <a-input
        :value="repoPath"
        placeholder="rushwind-admin 仓库根目录"
        @change="onChange"
        @press-enter="onChange"
        allow-clear
      />
      <div v-if="repoProbe" class="probe-row">
        <a-tag :color="repoProbe.backend ? 'success' : 'error'">backend</a-tag>
        <a-tag :color="repoProbe.react ? 'success' : 'error'">react</a-tag>
        <a-tag :color="repoProbe.vben ? 'success' : 'error'">vben</a-tag>
        <a-tag :color="repoProbe.element ? 'success' : 'error'">element</a-tag>
        <a-tag :color="repoProbe.seed_rs ? 'success' : 'error'">seed.rs</a-tag>
        <a-tag v-if="repoProbe.specs && repoProbe.specs.length" color="processing">
          规格: {{ repoProbe.specs.join('、') }}
        </a-tag>
      </div>
    </div>
  </div>
</template>

<style scoped>
.header-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 20px 10px;
  border-bottom: 1px solid #f0f0f0;
}

.logo {
  width: 34px;
  height: 34px;
  border-radius: 8px;
  flex: none;
}

.title {
  font-size: 16px;
  font-weight: 600;
}

.subtitle {
  color: rgba(0, 0, 0, 0.45);
  font-size: 12px;
}

.title-block {
  flex: none;
  margin-right: 8px;
}

.repo-box {
  flex: 1;
  min-width: 0;
  max-width: 520px;
}

.probe-row {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
  margin-top: 6px;
}

.probe-row :deep(.ant-tag) {
  margin-inline-end: 0;
  font-size: 12px;
  line-height: 18px;
}
</style>
