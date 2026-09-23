<script setup lang="ts">
import {ref, watch, nextTick, computed} from "vue";
import {message} from "ant-design-vue";
import {
  DeleteOutlined,
  FormatPainterOutlined,
  FileTextOutlined,
  BarChartOutlined,
} from '@ant-design/icons-vue';

import {ImportSqlTables} from "../../bridge/App";

import MonacoEditor from './MonacoEditor.vue';

const props = defineProps<{
  open?: boolean
  modelValue?: string
  dbType?: 'mysql' | 'postgresql' | 'sqlite' | 'oracle' // 从父组件传入
}>()

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'update:modelValue', value: string): void
  (e: 'submit', value: string): void
}>()

const innerOpen = ref(false)
const sqlContent = ref(`SELECT *
                        FROM users
                        WHERE id = 1;`);

const editorRef = ref<InstanceType<typeof MonacoEditor>>()

// 同步外部的 open 状态
watch(() => props.open, async (val) => {
  innerOpen.value = val ?? false
  if (val) {
    // 打开时同步初始值
    sqlContent.value = props.modelValue || ''
    // 等待 DOM 更新后聚焦
    await nextTick()
    editorRef.value?.focus()
  }
}, {immediate: true})

// 同步 SQL 内容到外部
watch(() => sqlContent.value, (val) => {
  emit('update:modelValue', val)
})

// 关闭模态框
function handleClose() {
  emit('update:open', false)
}

// 提交表单
async function handleCommit() {
  const trimmed = sqlContent.value.trim()
  if (!trimmed) {
    message.warning('请输入 SQL 语句')
    return
  }

  // 后端导入时会设置好对应的数据库配置,此处不再覆写。
  const res = await ImportSqlTables(trimmed);
  if (res !== '') {
    message.error('SQL 导入失败，请检查语句是否正确')
    return
  }

  message.success('SQL导入成功！');

  emit('submit', trimmed)
  handleClose()
}

// 清空 SQL
function clearSQL() {
  sqlContent.value = ''
  message.success('已清空')
  editorRef.value?.focus()
}

// 格式化 SQL
function formatSQL() {
  editorRef.value?.formatDocument()
  message.success('格式化完成')
}

// 获取统计信息
const lineCount = computed(() => {
  return sqlContent.value.split('\n').filter(line => line.trim()).length
})

const charCount = computed(() => {
  return sqlContent.value.length
})

</script>

<template>
  <a-modal
      v-model:open="innerOpen"
      title="SQL 输入"
      :width="900"
      @ok="handleCommit"
      @cancel="handleClose"
      okText="导入"
      cancelText="取消"
      :okButtonProps="{ disabled: !sqlContent.trim() }"
      :bodyStyle="{ padding: '16px' }"
      :destroyOnClose="true"
  >
    <!-- 工具栏 -->
    <div class="toolbar">
      <div class="toolbar-left">
        <a-button
            size="small"
            @click="clearSQL"
            type="text"
            title="清空内容"
        >
          <template #icon>
            <DeleteOutlined/>
          </template>
          清空
        </a-button>
        <a-button
            size="small"
            @click="formatSQL"
            type="text"
            title="格式化 SQL"
        >
          <template #icon>
            <FormatPainterOutlined/>
          </template>
          格式化
        </a-button>
        <a-divider type="vertical"/>
        <a-tag color="blue">SQL</a-tag>
      </div>
      <div class="toolbar-right">
        <span class="stat-item"><FileTextOutlined style="margin-right: 4px"/> 行数: {{ lineCount }}</span>
        <span class="stat-item"><BarChartOutlined style="margin-right: 4px"/> 字符: {{ charCount }}</span>
      </div>
    </div>

    <!-- Monaco Editor -->
    <div class="editor-wrapper">
      <MonacoEditor
          ref="editorRef"
          v-model="sqlContent"
          :db-type="dbType"
          :height="400"
          @change="(val: any) => emit('update:modelValue', val)"
      />
    </div>
  </a-modal>
</template>

<style scoped>
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
  padding: 8px 0;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 16px;
  font-size: 12px;
  color: #8c8c8c;
}

.stat-item {
  display: flex;
  align-items: center;
  gap: 4px;
}

.icon-btn {
  font-size: 14px;
  margin-right: 4px;
}

.editor-wrapper {
  border: 1px solid #d9d9d9;
  border-radius: 4px;
  overflow: hidden;
  transition: border-color 0.3s;
  margin-bottom: 12px;
}

.editor-wrapper:hover {
  border-color: #4096ff;
}

:deep(.ant-modal-body) {
  padding: 16px;
}

:deep(.ant-divider-vertical) {
  height: 20px;
}

:deep(.ant-btn-loading-icon) {
  margin-right: 4px;
}
</style>
