<script setup>
import { ref, computed, watch } from 'vue';
import { message } from 'ant-design-vue';
import { PlusOutlined } from '@ant-design/icons-vue';
import FieldEditor from '../components/FieldEditor.vue';
import ReportView from '../components/ReportView.vue';
import { call, repoPath, SNAKE_RE } from '../api.js';

const fields = ref([
  { name: '', kind: 'string', values: [{ num: 0, text: 'OFF' }, { num: 1, text: 'ON' }], default: 'OFF' },
]);
const editor = ref(null);
const name = ref(localStorage.getItem('rushui.entity.name') || '');
const codeField = ref('');
const global = ref(false);
const check = ref(false);
const regen = ref(false);
const authFree = ref(false);
const dryRun = ref(true);
const table = ref('');
const pkg = ref('');
const routePrefix = ref('');
const running = ref(false);
const report = ref(null);
const lastDryRun = ref(true);
const canHandoff = ref(false);

watch(name, (value) => localStorage.setItem('rushui.entity.name', value));

const stringNames = computed(() =>
  fields.value.filter((field) => field.kind === 'string' && field.name.trim()).map((field) => field.name.trim()),
);
watch(
  stringNames,
  (names) => {
    if (codeField.value && !names.includes(codeField.value)) codeField.value = undefined;
  },
);

function payload() {
  const dto = editor.value.toDto(fields.value, '实体字段');
  if (dto.length === 0) throw new Error('至少一个业务字段（重生成模式可留空，按规格整体重写）');
  if (!repoPath.value) throw new Error('仓库根目录必填（右上角）');
  if (!SNAKE_RE.test(name.value)) throw new Error(`实体名须为 snake_case：${name.value || '（空）'}`);
  return {
    repo_root: repoPath.value,
    name: name.value,
    table: table.value.trim() || null,
    package: pkg.value.trim() || null,
    route_prefix: routePrefix.value.trim() || null,
    fields: regen.value && dto.length === 0 ? [] : dto,
    code_field: codeField.value || null,
    global: global.value,
    check: check.value,
    dry_run: dryRun.value,
    skip_manifest: false,
    overwrite: regen.value,
    auth_free: authFree.value,
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
    report.value = await call('gen_entity', { opts });
    lastDryRun.value = dryRun.value;
    if (!dryRun.value) {
      canHandoff.value = true;
      localStorage.setItem(
        'rushui.handoff',
        JSON.stringify({ name: name.value, fields: opts.fields, code_field: opts.code_field }),
      );
      message.success(`已就绪交接：实体「${name.value}」——去「页面生成」页签点「载入实体」`);
    }
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
      <a-form-item label="实体名（snake_case 单数）">
        <a-input v-model:value="name" class="mono" placeholder="widget" />
      </a-form-item>
      <a-form-item label="唯一编码字段（code 臂 + /code/{code} 路由，须为 string 字段）">
        <a-select v-model:value="codeField" allow-clear placeholder="（无）">
          <a-select-option v-for="option in stringNames" :key="option" :value="option">{{ option }}</a-select-option>
        </a-select>
      </a-form-item>
    </a-form>

    <a-card size="small" title="业务字段">
      <FieldEditor ref="editor" v-model:fields="fields" />
      <a-button type="dashed" block style="margin-top: 8px" @click="editor.addField()">
        <template #icon><PlusOutlined /></template>
        添加字段
      </a-button>
      <div class="path-hint" style="margin-top: 6px">
        生成成功后字段真相落成 <code>.rush/&lt;name&gt;.json</code>，gen pages 免重输
      </div>
    </a-card>

    <a-collapse>
      <a-collapse-panel key="adv" header="高级选项（表名 / package / 路由前缀，缺省自动）">
        <a-form layout="vertical" style="max-width: 640px">
          <a-form-item label="表名（缺省 sys_&lt;复数&gt;）">
            <a-input v-model:value="table" class="mono" placeholder="sys_widgets" />
          </a-form-item>
          <a-form-item label="消息面 package（缺省 &lt;name&gt;.service.v1）">
            <a-input v-model:value="pkg" class="mono" placeholder="widget.service.v1" />
          </a-form-item>
          <a-form-item label="路由前缀（缺省 /admin/v1/&lt;复数&gt;）">
            <a-input v-model:value="routePrefix" class="mono" placeholder="/admin/v1/widgets" />
          </a-form-item>
        </a-form>
      </a-collapse-panel>
    </a-collapse>

    <a-space wrap :size="18">
      <a-checkbox v-model:checked="global">平台全局表（--global）</a-checkbox>
      <a-checkbox v-model:checked="check">生成后 cargo check</a-checkbox>
      <a-checkbox v-model:checked="regen">重生成（覆盖既有生成物）</a-checkbox>
      <a-checkbox v-model:checked="authFree">免鉴权白名单（auth_free）</a-checkbox>
      <a-checkbox v-model:checked="dryRun">dry-run 预览</a-checkbox>
    </a-space>

    <div class="step-footer">
      <a-space>
        <a-button type="primary" :loading="running" @click="run">生成</a-button>
      </a-space>
      <span class="path-hint">字段留空 + 重生成 = 整体按 .rush/&lt;name&gt;.json 重写</span>
    </div>

    <template v-if="report">
      <a-alert
        v-if="report.__error"
        type="error"
        show-icon
        :message="report.__error"
        style="white-space: pre-wrap"
      />
      <ReportView v-else :report="report" :dry-run="lastDryRun" show-check />
    </template>
  </a-space>
</template>
