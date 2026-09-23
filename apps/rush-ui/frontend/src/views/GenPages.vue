<script setup>
import { ref, onMounted } from 'vue';
import { message } from 'ant-design-vue';
import FieldEditor from '../components/FieldEditor.vue';
import ReportView from '../components/ReportView.vue';
import { call, repoPath, SNAKE_RE } from '../api.js';

const fields = ref([]);
const editor = ref(null);
const name = ref(localStorage.getItem('rushui.pages.name') || '');
const stack = ref('react');
const group = ref('');
const codeField = ref('');
const routePrefix = ref('');
const explicit = ref(false);
const global = ref(false);
const regen = ref(false);
const dryRun = ref(true);
const running = ref(false);
const report = ref(null);
const lastDryRun = ref(true);
const hasHandoff = ref(!!localStorage.getItem('rushui.handoff'));

watch(name, (value) => localStorage.setItem('rushui.pages.name', value));
watch(stack, (value) => localStorage.setItem('rushui.pages.stack', value));
stack.value = localStorage.getItem('rushui.pages.stack') || 'react';

function loadHandoff() {
  try {
    const handoff = JSON.parse(localStorage.getItem('rushui.handoff') || 'null');
    if (!handoff) return;
    name.value = handoff.name;
    fields.value = editor.value.fromDto(handoff.fields);
    explicit.value = true;
    if (handoff.code_field) codeField.value = handoff.code_field;
    message.success(`已载入实体「${handoff.name}」的 ${handoff.fields.length} 个字段`);
  } catch {
    /* 无交接或损坏则忽略 */
  }
}

onMounted(() => {
  if (hasHandoff.value) loadHandoff();
});

function payload() {
  if (!repoPath.value) throw new Error('仓库根目录必填（右上角）');
  if (!SNAKE_RE.test(name.value)) throw new Error(`实体名须为 snake_case：${name.value || '（空）'}`);
  let fieldsDto = [];
  if (explicit.value) fieldsDto = editor.value.toDto(fields.value, '页面字段');
  return {
    repo_root: repoPath.value,
    name: name.value,
    group: group.value.trim() || null,
    route_prefix: routePrefix.value.trim() || null,
    fields: fieldsDto,
    code_field: codeField.value.trim() || null,
    stack: stack.value,
    global: global.value ? true : null,
    overwrite: regen.value,
    dry_run: dryRun.value,
  };
}

async function run() {
  let opts;
  try {
    opts = payload();
  } catch (error) {
    message.error(String(error.message || error));
    return;
  }
  running.value = true;
  try {
    report.value = await call('gen_pages', { opts });
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
    <a-form layout="vertical" style="max-width: 640px">
      <a-row :gutter="12">
        <a-col :span="12">
          <a-form-item label="实体名（须与 gen entity 一致）">
            <a-input v-model:value="name" class="mono" placeholder="widget" />
          </a-form-item>
        </a-col>
        <a-col :span="12">
          <a-form-item label="目标前端栈">
            <a-select v-model:value="stack">
              <a-select-option value="react">react（菜单走后端 seed）</a-select-option>
              <a-select-option value="vben">vue-vben（静态路由模块）</a-select-option>
              <a-select-option value="element">vue-element（静态路由模块）</a-select-option>
            </a-select>
          </a-form-item>
        </a-col>
        <a-col :span="12">
          <a-form-item label="页面分组（缺省 system）">
            <a-input v-model:value="group" class="mono" placeholder="system" />
          </a-form-item>
        </a-col>
        <a-col :span="12">
          <a-form-item label="唯一编码字段（可选，缺省继承规格）">
            <a-input v-model:value="codeField" class="mono" placeholder="code" />
          </a-form-item>
        </a-col>
        <a-col :span="24">
          <a-form-item label="路由前缀（可选，缺省继承规格）">
            <a-input v-model:value="routePrefix" class="mono" placeholder="/admin/v1/widgets" />
          </a-form-item>
        </a-col>
      </a-row>
    </a-form>

    <a-checkbox v-model:checked="explicit">显式指定字段（缺省从 .rush/&lt;name&gt;.json 规格继承，推荐）</a-checkbox>
    <a-card v-if="explicit" size="small" title="业务字段">
      <FieldEditor ref="editor" v-model:fields="fields" />
      <a-button type="dashed" block style="margin-top: 8px" @click="editor.addField()">
        <template #icon><PlusOutlined /></template>
        添加字段
      </a-button>
    </a-card>

    <a-space wrap :size="18">
      <a-checkbox v-model:checked="global">平台全局表（缺省继承规格）</a-checkbox>
      <a-checkbox v-model:checked="regen">重生成（覆盖既有页面组）</a-checkbox>
      <a-checkbox v-model:checked="dryRun">dry-run 预览</a-checkbox>
    </a-space>

    <div class="step-footer">
      <a-button type="primary" :loading="running" @click="run">生成</a-button>
      <span class="path-hint">react：页面 + seed 菜单；vben / element：composables + 静态路由模块</span>
    </div>

    <template v-if="report">
      <a-alert v-if="report.__error" type="error" show-icon :message="report.__error" style="white-space: pre-wrap" />
      <ReportView v-else :report="report" :dry-run="lastDryRun" />
    </template>
  </a-space>
</template>
