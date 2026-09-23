<script setup>
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons-vue';
import { SNAKE_RE, UPPER_RE } from '../bridge/constants';

const props = defineProps({
  fields: { type: Array, required: true }, // v-model:fields
});
const emit = defineEmits(['update:fields']);

function update(next) {
  emit('update:fields', next);
}

function addField() {
  update([...props.fields, { name: '', kind: 'string', values: [{ num: 0, text: 'OFF' }, { num: 1, text: 'ON' }], default: 'OFF' }]);
}

function removeField(index) {
  const next = props.fields.filter((_, i) => i !== index);
  update(next.length ? next : [{ name: '', kind: 'string', values: [{ num: 0, text: 'OFF' }, { num: 1, text: 'ON' }], default: 'OFF' }]);
}

function onKindChange(field, kind) {
  field.kind = kind;
}

function addValue(field) {
  field.values.push({ num: field.values.length, text: '' });
}

function removeValue(field, index) {
  const removed = field.values.splice(index, 1)[0];
  if (field.default === removed.text) field.default = field.values[0]?.text ?? '';
}

/** 折叠成 DTO（kind 规格串）；非法抛中文错误。 */
function toDto(list, label) {
  const seen = new Set();
  return list.map((field) => {
    const name = field.name.trim();
    if (!name) throw new Error(`${label}存在未命名的字段`);
    if (!SNAKE_RE.test(name)) throw new Error(`字段名须为 snake_case：${name}`);
    if (seen.has(name)) throw new Error(`字段重复：${name}`);
    seen.add(name);
    let kind = field.kind;
    if (kind === 'enum') {
      const nums = new Set();
      const texts = new Set();
      const parts = [];
      let zeroText = null;
      for (const value of field.values) {
        const text = value.text.trim();
        const num = Number(value.num);
        if (!text) throw new Error(`字段 ${name} 的枚举取值缺文本`);
        if (!UPPER_RE.test(text)) throw new Error(`字段 ${name} 的枚举文本须为 UPPER_SNAKE：${text}`);
        if (!Number.isInteger(num)) throw new Error(`字段 ${name} 的枚举数值非法：${value.num}`);
        if (nums.has(num)) throw new Error(`字段 ${name} 的枚举数值重复：${num}`);
        if (texts.has(text)) throw new Error(`字段 ${name} 的枚举文本重复：${text}`);
        if (num === 0) zeroText = text;
        nums.add(num);
        texts.add(text);
        parts.push(`${num}=${text}`);
      }
      if (zeroText === null) throw new Error(`字段 ${name} 的枚举必须含 0 值项`);
      const fallback = field.default.trim() || zeroText;
      kind = `enum(${parts.join(',')})` + (fallback !== zeroText ? `@default=${fallback}` : '');
    }
    return { name, kind };
  });
}

/** 规格串 → 编辑器状态。 */
function fromDto(dto) {
  return dto.map((field) => {
    const match = /^enum\((.*)\)(?:@default=([A-Z0-9_]+))?$/.exec(field.kind);
    if (!match) return { name: field.name, kind: field.kind, values: [], default: '' };
    const values = match[1].split(',').map((part) => {
      const [num, text] = part.split('=');
      return { num: Number(num), text };
    });
    const zero = values.find((value) => value.num === 0);
    return { name: field.name, kind: 'enum', values, default: match[2] || zero?.text || '' };
  });
}

defineExpose({ toDto, fromDto, addField });
</script>

<template>
  <div class="field-editor">
    <div v-for="(field, index) in fields" :key="index" class="field-block">
      <a-row :gutter="8" align="middle">
        <a-col :span="11">
          <a-input v-model:value="field.name" class="mono" placeholder="字段名 (snake_case)" />
        </a-col>
        <a-col :span="7">
          <a-select v-model:value="field.kind" style="width: 100%">
            <a-select-option value="string">string</a-select-option>
            <a-select-option value="i32">i32</a-select-option>
            <a-select-option value="u32">u32</a-select-option>
            <a-select-option value="bool">bool</a-select-option>
            <a-select-option value="f64">f64</a-select-option>
            <a-select-option value="enum">enum(…)</a-select-option>
          </a-select>
        </a-col>
        <a-col :span="6">
          <a-button block danger @click="removeField(index)">
            <template #icon><DeleteOutlined /></template>
          </a-button>
        </a-col>
      </a-row>
      <div v-if="field.kind === 'enum'" class="enum-editor">
        <div class="enum-head">取值集（数值必须含 0；未识别值回退到缺省文本）</div>
        <a-row v-for="(value, vi) in field.values" :key="vi" :gutter="8" align="middle" class="enum-row">
          <a-col :span="6">
            <a-input-number v-model:value="value.num" class="num" style="width: 100%" />
          </a-col>
          <a-col :span="14">
            <a-input v-model:value="value.text" class="mono" placeholder="取值文本 (UPPER_SNAKE)" />
          </a-col>
          <a-col :span="4">
            <a-button block danger size="small" @click="removeValue(field, vi)">
              <template #icon><DeleteOutlined /></template>
            </a-button>
          </a-col>
        </a-row>
        <a-space align="center">
          <a-button type="link" size="small" @click="addValue(field)">
            <template #icon><PlusOutlined /></template>
            取值
          </a-button>
          <span class="enum-head">缺省/回退</span>
          <a-select v-model:value="field.default" style="width: 140px">
            <a-select-option v-for="value in field.values" :key="value.text" :value="value.text">
              {{ value.text }}
            </a-select-option>
          </a-select>
        </a-space>
      </div>
      <a-divider v-if="index < fields.length - 1" style="margin: 4px 0" />
    </div>
  </div>
</template>

<style scoped>
.enum-editor {
  border: 1px dashed #d9d9d9;
  border-radius: 8px;
  padding: 10px 12px;
  background: #fafafa;
  margin-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.enum-head {
  font-size: 12px;
  color: rgba(0, 0, 0, 0.45);
}

.enum-row {
  margin: 2px 0;
}
</style>
