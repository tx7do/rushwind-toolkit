<script setup lang="ts">
import {ref, computed, watch} from 'vue'
import {message} from 'ant-design-vue'
import {useI18n} from 'vue-i18n'
import {
  InboxOutlined,
  FolderOpenOutlined,
  EyeOutlined,
  UndoOutlined,
  RightOutlined,
} from '@ant-design/icons-vue'

import {GenPages, SpecLoad} from "../../bridge/App";
import {frontendgen, rush} from "../../bridge/models";
import {useProject} from "../../stores/project";

import MonacoEditor from "../backend/MonacoEditor.vue";

const {t} = useI18n()

const {projectInfo} = useProject()

const confirmLoading = ref(false)

// ==================== 步骤控制 ====================
const currentStep = ref(0)

// ==================== 目标框架 ====================
type TargetFramework = 'vue-element' | 'vue-vben' | 'react'
const targetFramework = ref<TargetFramework>('react')

const frameworkOptions = computed(() => [
  {label: 'React', value: 'react', available: projectInfo.value?.HasReact ?? false},
  {label: 'Vue3 Vben', value: 'vue-vben', available: projectInfo.value?.HasVben ?? false},
  {label: 'Vue3 Element Plus', value: 'vue-element', available: projectInfo.value?.HasElement ?? false},
])

function stackOf(framework: TargetFramework): rush.PagesOptionsDto['stack'] {
  return framework === 'vue-element' ? 'element' : framework === 'vue-vben' ? 'vben' : 'react'
}

// ==================== 规格导入方式 ====================
type ImportSource = 'project' | 'local' | 'remote' | 'paste'
const importSource = ref<ImportSource>('project')

// 项目规格（.rush/*.json）
const specNames = ref<string[]>([])

// 本地文件
const selectedFileName = ref('')
const fileInputRef = ref<HTMLInputElement | null>(null)

// 远程 URL
const remoteUrl = ref('')
const remoteLoading = ref(false)

// 粘贴内容
const jsonContent = ref('')

// ==================== 规格数据 ====================
const parsedSpecs = ref<rush.EntitySpecFile[]>([])
const selectedSpecKeys = ref<string[]>([])

// ==================== 生成选项 ====================
const generateOptions = ref({
  group: '',
  overwrite: false,
})

// ==================== 生成结果 ====================
const generatedFiles = ref<frontendgen.GeneratedFile[]>([])
const selectedFileIndex = ref(0)

const currentFileContent = ref('')

// ==================== 文件列表过滤 ====================
const activeFileType = ref<string>('all')
const fileTypeOptions = computed(() => [
  {label: t('frontend.fileType.all'), value: 'all'},
  {label: t('frontend.fileType.new'), value: 'created'},
  {label: t('frontend.fileType.changed'), value: 'diff'},
  {label: t('frontend.fileType.edited'), value: 'edited'},
])

const filteredFiles = ref<frontendgen.GeneratedFile[]>([])

function filterFiles() {
  if (activeFileType.value === 'all') {
    filteredFiles.value = generatedFiles.value
  } else {
    filteredFiles.value = generatedFiles.value.filter(f => f.type === activeFileType.value)
  }
  if (selectedFileIndex.value >= filteredFiles.value.length) {
    selectedFileIndex.value = 0
  }
  updatePreview()
}

function updatePreview() {
  if (filteredFiles.value.length === 0) {
    currentFileContent.value = ''
    return
  }
  const idx = Math.min(selectedFileIndex.value, filteredFiles.value.length - 1)
  currentFileContent.value = filteredFiles.value[idx]?.content || ''
}

function selectFile(index: number) {
  selectedFileIndex.value = index
  updatePreview()
}

// ==================== 本地文件选择 ====================
function handleChooseFile() {
  fileInputRef.value?.click()
}

// 拖拽状态
const fileDragging = ref(false)

function handleFileDragOver(e: DragEvent) {
  e.preventDefault()
  fileDragging.value = true
}

function handleFileDragLeave() {
  fileDragging.value = false
}

function handleFileDrop(e: DragEvent) {
  e.preventDefault()
  fileDragging.value = false

  const file = e.dataTransfer?.files?.[0]
  if (!file) return

  const ext = file.name.split('.').pop()?.toLowerCase()
  if (ext !== 'json') {
    message.warning(t('frontend.import.fileDragWarning'))
    return
  }

  processSpecFile(file)
}

function processSpecFile(file: File) {
  selectedFileName.value = file.name
  const reader = new FileReader()
  reader.onload = (e) => {
    jsonContent.value = e.target?.result as string || ''
    handleParse()
  }
  reader.readAsText(file)
}

function handleFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return

  processSpecFile(file)
  input.value = ''
}

// ==================== 远程 URL 拉取 ====================
async function handleFetchRemote() {
  if (!remoteUrl.value.trim()) {
    message.warning(t('frontend.import.remoteUrlRequired'))
    return
  }

  remoteLoading.value = true
  try {
    const response = await fetch(remoteUrl.value.trim())
    if (!response.ok) {
      message.error(t('frontend.import.requestFailed', {status: response.status, text: response.statusText}))
      return
    }

    const text = await response.text()
    if (!text.trim()) {
      message.error(t('frontend.import.remoteEmpty'))
      return
    }

    jsonContent.value = text
    message.success(t('frontend.import.remoteSuccess'))
    handleParse()
  } catch (e: any) {
    message.error(t('frontend.import.remoteFetchFailed', {error: e.message || e}))
  } finally {
    remoteLoading.value = false
  }
}

// ==================== 解析 ====================
function parseSpecJson(text: string, label: string): rush.EntitySpecFile {
  let spec: any
  try {
    spec = JSON.parse(text)
  } catch (e: any) {
    throw new Error(`${label}: ${e.message || e}`)
  }
  if (!spec || typeof spec.name !== 'string' || !Array.isArray(spec.fields)) {
    throw new Error(t('frontend.import.parseFailed', {msg: `${label} (need name + fields[])`}))
  }
  return spec as rush.EntitySpecFile
}

async function handleParse() {
  try {
    let specs: rush.EntitySpecFile[] = []
    if (importSource.value === 'project') {
      const repo = projectInfo.value?.Root
      if (!repo || specNames.value.length === 0) return
      for (const name of specNames.value) {
        specs.push(await SpecLoad(repo, name))
      }
    } else {
      if (!jsonContent.value.trim()) return
      specs = [parseSpecJson(jsonContent.value, importSource.value === 'local' ? selectedFileName.value : t('frontend.import.paste'))]
    }
    parsedSpecs.value = specs
    selectedSpecKeys.value = specs.map(s => s.name)
    currentStep.value = 1
  } catch (e: any) {
    message.error(String(e.message || e))
    console.error('解析实体规格失败:', e)
  }
}

// ==================== 生成参数 ====================
function pagesDto(name: string, dryRun: boolean): rush.PagesOptionsDto {
  const spec = parsedSpecs.value.find(s => s.name === name)
  return {
    repo_root: projectInfo.value?.Root ?? '',
    name,
    group: generateOptions.value.group.trim() || null,
    route_prefix: spec?.route_prefix || null,
    fields: (spec?.fields ?? []).map(f => ({name: f.name, kind: f.kind})),
    code_field: spec?.code_field ?? null,
    stack: stackOf(targetFramework.value),
    global: spec ? spec.global : null,
    overwrite: generateOptions.value.overwrite,
    dry_run: dryRun,
  }
}

function repoRelative(path: string): string {
  const root = projectInfo.value?.Root
  if (root && path.startsWith(root + '/')) return path.slice(root.length + 1)
  return path
}

/** 一个实体的 PagesReport → 预览文件列表。 */
function reportToFiles(name: string, report: rush.PagesReport): frontendgen.GeneratedFile[] {
  const files: frontendgen.GeneratedFile[] = []
  for (const path of report.created ?? []) {
    files.push({path: repoRelative(path), content: '', description: t('frontend.fileType.new'), serviceName: name, type: 'created'})
  }
  for (const [path, diff] of report.diffs ?? []) {
    files.push({path: repoRelative(path), content: diff, description: t('frontend.fileType.changed'), serviceName: name, type: 'diff'})
  }
  for (const path of report.edited ?? []) {
    files.push({path: repoRelative(path), content: '', description: t('frontend.fileType.edited'), serviceName: name, type: 'edited'})
  }
  return files
}

// ==================== 预览 ====================
async function handlePreview() {
  const selected = selectedSpecKeys.value
  if (selected.length === 0) return

  try {
    const files: frontendgen.GeneratedFile[] = []
    for (const name of selected) {
      const report = await GenPages(pagesDto(name, true))
      files.push(...reportToFiles(name, report))
    }
    generatedFiles.value = files
  } catch (e: any) {
    message.error(String(e.message || e))
    return
  }

  selectedFileIndex.value = 0
  activeFileType.value = 'all'
  filterFiles()
  currentStep.value = 2
}

// ==================== 选择 ====================
function handleSelectAll() {
  selectedSpecKeys.value = selectedSpecKeys.value.length === parsedSpecs.value.length
    ? [] : parsedSpecs.value.map(s => s.name)
}

function handleSelectUngenerated() {
  selectedSpecKeys.value = parsedSpecs.value
    .filter((s: rush.EntitySpecFile) => !s.stack)
    .map(s => s.name)
}

function toggleSpecSelection(name: string) {
  const idx = selectedSpecKeys.value.indexOf(name)
  if (idx >= 0) {
    selectedSpecKeys.value.splice(idx, 1)
  } else {
    selectedSpecKeys.value.push(name)
  }
}

// ==================== 确认生成（写盘） ====================
async function handleCommit() {
  try {
    confirmLoading.value = true
    let count = 0
    for (const name of selectedSpecKeys.value) {
      const report = await GenPages(pagesDto(name, false))
      count += (report.created?.length ?? 0) + (report.updated?.length ?? 0) + (report.edited?.length ?? 0)
    }
    message.success(t('frontend.config.generateSuccess', {count}))
    currentStep.value = 0
    resetState()
  } catch (error: any) {
    message.error(String(error.message || error))
    console.error('页面生成失败:', error)
  } finally {
    confirmLoading.value = false
  }
}

function resetState() {
  jsonContent.value = ''
  parsedSpecs.value = []
  selectedSpecKeys.value = []
  specNames.value = []
  generatedFiles.value = []
  filteredFiles.value = []
  selectedFileIndex.value = 0
  currentFileContent.value = ''
  selectedFileName.value = ''
  remoteUrl.value = ''
}

// 项目是全局状态:换项目后本页残留的旧规格会让"生成"仍然按上一个项目的
// 解析结果往上个项目目录里写文件,用户以为在操作当前项目。
watch(projectInfo, (pi, prev) => {
  if (pi?.ModPath === prev?.ModPath) return
  currentStep.value = 0
  resetState()
})

function specOperations(spec: rush.EntitySpecFile): string[] {
  const ops = ['list', 'get', 'create', 'update', 'delete']
  if (spec.code_field) ops.push('other')
  return ops
}

function getOperationTag(type: string) {
  const map: Record<string, { color: string; text: string }> = {
    list: {color: 'blue', text: t('frontend.operation.list')}, get: {color: 'cyan', text: t('frontend.operation.get')},
    create: {color: 'green', text: t('frontend.operation.create')}, update: {color: 'orange', text: t('frontend.operation.update')},
    delete: {color: 'red', text: t('frontend.operation.delete')}, other: {color: 'default', text: t('frontend.operation.other')},
  }
  return map[type] || map.other
}

function getFileTypeColor(type: string) {
  const map: Record<string, string> = {
    created: 'green', diff: 'orange', edited: 'geekblue',
  }
  return map[type] || 'default'
}

// 根据文件路径推断 Monaco 语言
function detectLanguage(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase() || ''
  const langMap: Record<string, string> = {
    ts: 'typescript',
    tsx: 'typescript',
    js: 'javascript',
    jsx: 'javascript',
    vue: 'html',
    html: 'html',
    css: 'css',
    scss: 'css',
    less: 'css',
    json: 'json',
    yaml: 'yaml',
    yml: 'yaml',
    sql: 'sql',
    md: 'markdown',
    xml: 'xml',
  }
  return langMap[ext] || 'plaintext'
}

const previewLanguage = computed(() => {
  if (filteredFiles.value.length === 0) return 'plaintext'
  const file = filteredFiles.value[selectedFileIndex.value]
  if (!file) return 'plaintext'
  if (file.type === 'diff') return 'diff'
  return detectLanguage(file.path)
})
</script>

<template>
  <div class="frontend-gen-container">
    <!-- 步骤条 -->
    <a-steps :current="currentStep" size="small" style="margin-bottom: 20px">
      <a-step :title="t('frontend.steps.importOpenApi')"/>
      <a-step :title="t('frontend.steps.genConfig')"/>
      <a-step :title="t('frontend.steps.previewGenerate')"/>
    </a-steps>

    <!-- ====== 步骤 0: 导入实体规格 ====== -->
    <div v-if="currentStep === 0" class="step-content">
      <!-- 目标框架选择 -->
      <a-card :title="t('frontend.framework.title')" size="small" style="margin-bottom: 16px">
        <a-radio-group v-model:value="targetFramework" button-style="solid">
          <a-radio-button v-for="opt in frameworkOptions" :key="opt.value" :value="opt.value" :disabled="!opt.available && !!projectInfo">
            {{ opt.label }}
            <a-tag v-if="projectInfo" :color="opt.available ? 'success' : 'error'" style="margin-left: 6px">
              {{ opt.available ? t('frontend.framework.inRepo') : t('frontend.framework.missing') }}
            </a-tag>
          </a-radio-button>
        </a-radio-group>
      </a-card>

      <!-- 导入方式切换 -->
      <a-card :title="t('frontend.import.title')" size="small">
        <a-radio-group v-model:value="importSource" style="margin-bottom: 16px">
          <a-radio-button value="project">{{ t('frontend.import.project') }}</a-radio-button>
          <a-radio-button value="local">{{ t('frontend.import.local') }}</a-radio-button>
          <a-radio-button value="remote">{{ t('frontend.import.remote') }}</a-radio-button>
          <a-radio-button value="paste">{{ t('frontend.import.paste') }}</a-radio-button>
        </a-radio-group>

        <!-- 项目规格（.rush/*.json） -->
        <div v-if="importSource === 'project'">
          <a-select
            v-model:value="specNames"
            mode="multiple"
            style="width: 100%; margin-bottom: 12px"
            :placeholder="t('frontend.import.specPlaceholder')"
            :options="(projectInfo?.Specs ?? []).map(n => ({label: n, value: n}))"
          />
          <a-alert v-if="!(projectInfo?.Specs?.length)" :message="t('frontend.import.noSpecs')" type="info" show-icon/>
        </div>

        <!-- 本地文件选择 -->
        <div v-if="importSource === 'local'">
          <input
            ref="fileInputRef"
            type="file"
            accept=".json"
            style="display: none"
            @change="handleFileChange"
          />
          <div
            class="file-drop-zone"
            :class="{ dragging: fileDragging }"
            @click="handleChooseFile"
            @dragover="handleFileDragOver"
            @dragleave="handleFileDragLeave"
            @drop="handleFileDrop"
          >
            <div class="drop-zone-content">
              <div style="font-size: 32px; color: #1890ff; margin-bottom: 8px"><InboxOutlined/></div>
              <div style="font-weight: 500; margin-bottom: 4px">
                {{ selectedFileName || t('frontend.import.fileDropHint') }}
              </div>
              <div style="color: #999; font-size: 12px">{{ t('frontend.import.fileFormatHint') }}</div>
            </div>
          </div>
        </div>

        <!-- 远程 URL -->
        <div v-if="importSource === 'remote'">
          <a-input-search
            v-model:value="remoteUrl"
            :placeholder="t('frontend.import.remotePlaceholder')"
            :enter-button="t('frontend.import.fetchBtn')"
            :loading="remoteLoading"
            @search="handleFetchRemote"
            style="margin-bottom: 12px"
          />
          <a-alert v-if="!remoteUrl" :message="t('frontend.import.remoteHint')" type="info" show-icon/>
        </div>

        <!-- 粘贴 JSON -->
        <div v-if="importSource === 'paste'">
          <a-textarea
            v-model:value="jsonContent"
            :placeholder="t('frontend.import.pastePlaceholder')"
            :auto-size="{ minRows: 12, maxRows: 22 }"
            style="font-family: 'Courier New', monospace; font-size: 12px;"
          />
        </div>
      </a-card>

      <div class="step-footer" style="justify-content: flex-end">
        <a-button
          type="primary"
          @click="handleParse"
          :disabled="importSource === 'project' ? specNames.length === 0 : !jsonContent.trim()"
        >
          <RightOutlined style="margin-right: 4px"/> {{ t('frontend.import.parseBtn') }}
        </a-button>
      </div>
    </div>

    <!-- ====== 步骤 1: 配置生成 ====== -->
    <div v-if="currentStep === 1" class="step-content">
      <!-- 生成配置 -->
      <a-card :title="t('frontend.config.title')" size="small" style="margin-bottom: 16px">
        <a-form layout="inline">
          <a-form-item :label="t('frontend.framework.selectFramework')">
            <a-select v-model:value="targetFramework" style="width: 180px">
              <a-select-option v-for="opt in frameworkOptions" :key="opt.value" :value="opt.value" :disabled="!opt.available">
                {{ opt.label }}
              </a-select-option>
            </a-select>
          </a-form-item>
          <a-form-item :label="t('frontend.config.repoRoot')">
            <a-input :value="projectInfo?.Root || '-'" class="mono" read-only style="width: 280px"/>
          </a-form-item>
          <a-form-item :label="t('frontend.config.group')">
            <a-input v-model:value="generateOptions.group" :placeholder="t('frontend.config.groupPlaceholder')" style="width: 180px"/>
          </a-form-item>
          <a-form-item>
            <a-checkbox v-model:checked="generateOptions.overwrite">{{ t('frontend.config.overwrite') }}</a-checkbox>
          </a-form-item>
        </a-form>
      </a-card>

      <!-- 实体列表 -->
      <a-card size="small">
        <template #title>
          <div style="display: flex; align-items: center; justify-content: space-between;">
            <span>{{ t('frontend.service.selectTitle', {selected: selectedSpecKeys.length, total: parsedSpecs.length}) }}</span>
            <a-space>
              <a-button size="small" @click="handleSelectAll">
                {{ selectedSpecKeys.length === parsedSpecs.length ? t('frontend.service.deselectAll') : t('frontend.service.selectAll') }}
              </a-button>
              <a-button size="small" type="dashed" @click="handleSelectUngenerated">{{ t('frontend.service.selectUngenerated') }}</a-button>
            </a-space>
          </div>
        </template>

        <a-list
          :data-source="parsedSpecs"
          size="small"
          :split="true"
          class="service-select-list"
        >
          <template #renderItem="{ item: spec }">
            <a-list-item
              class="service-list-item"
              :class="{ selected: selectedSpecKeys.includes(spec.name) }"
              @click="toggleSpecSelection(spec.name)"
            >
              <a-list-item-meta>
                <template #title>
                  <div class="service-item-title">
                    <span class="service-model-name">{{ spec.name }}</span>
                    <a-tag v-if="spec.stack" color="purple" size="small" style="margin-left: 6px">{{ spec.stack }}</a-tag>
                    <a-tag v-if="selectedSpecKeys.includes(spec.name)" color="blue" size="small" style="margin-left: 6px">{{ t('frontend.service.selected') }}</a-tag>
                  </div>
                </template>
                <template #description>
                  <div class="service-item-desc">
                    <span class="service-desc-text mono">{{ spec.route_prefix }} · {{ spec.table }}</span>
                    <div class="service-meta-row">
                      <a-tag v-for="op in specOperations(spec)" :key="op"
                             :color="getOperationTag(op).color" size="small">
                        {{ getOperationTag(op).text }}
                      </a-tag>
                      <a-tag v-if="spec.global" color="gold" size="small">{{ t('frontend.service.globalFlag') }}</a-tag>
                      <span class="service-field-count">{{ t('frontend.service.fields', {count: spec.fields.length}) }}</span>
                    </div>
                  </div>
                </template>
                <template #avatar>
                  <a-checkbox
                    :checked="selectedSpecKeys.includes(spec.name)"
                    @click.stop
                    @change="toggleSpecSelection(spec.name)"
                  />
                </template>
              </a-list-item-meta>
            </a-list-item>
          </template>
        </a-list>
      </a-card>

      <div class="step-footer">
        <a-button @click="currentStep = 0">{{ t('common.prevStep') }}</a-button>
        <a-button type="primary" @click="handlePreview" :disabled="selectedSpecKeys.length === 0">
          <EyeOutlined style="margin-right: 4px"/> {{ t('frontend.preview.previewBtn') }}
        </a-button>
      </div>
    </div>

    <!-- ====== 步骤 2: 预览 & 生成 ====== -->
    <div v-if="currentStep === 2" class="step-content">
      <div style="display: flex; gap: 16px; height: calc(100vh - 240px); min-height: 400px;">
        <!-- 左侧文件列表 -->
        <div class="file-list-panel">
          <div class="file-list-header">
            <span>{{ t('frontend.preview.files', {count: filteredFiles.length}) }}</span>
            <a-select v-model:value="activeFileType" :options="fileTypeOptions" size="small"
                      style="width: 110px" @change="filterFiles"/>
          </div>
          <div class="file-list-body">
            <div v-for="(file, index) in filteredFiles" :key="file.path"
                 :class="['file-item', { active: selectedFileIndex === index }]"
                 @click="selectFile(index)">
              <div class="file-path">{{ file.path }}</div>
              <div class="file-meta">
                <a-tag :color="getFileTypeColor(file.type)" size="small">{{ file.serviceName }}</a-tag>
                <span class="file-desc">{{ file.description }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- 右侧代码预览 -->
        <div class="code-preview-panel">
          <div v-if="currentFileContent" class="monaco-wrapper">
            <MonacoEditor
              :key="selectedFileIndex + '-' + previewLanguage"
              :model-value="currentFileContent"
              :language="previewLanguage"
              :read-only="true"
              height="100%"
            />
          </div>
          <a-empty v-else :description="t('frontend.preview.noContent')" style="margin-top: 100px"/>
        </div>
      </div>

      <div class="step-footer">
        <a-button @click="currentStep = 1">{{ t('common.prevStep') }}</a-button>
        <a-space>
          <a-button @click="currentStep = 0; resetState()"><UndoOutlined style="margin-right: 4px"/> {{ t('frontend.preview.resetBtn') }}</a-button>
          <a-button type="primary" :loading="confirmLoading" @click="handleCommit">
            {{ t('frontend.preview.confirmBtn') }}
          </a-button>
        </a-space>
      </div>
    </div>
  </div>
</template>

<style scoped>
.frontend-gen-container {
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
}

.step-content {
  flex: 1;
  overflow: auto;
}

/* 本地文件选择区域 */
.file-drop-zone {
  border: 2px dashed #d9d9d9;
  border-radius: 8px;
  padding: 32px 16px;
  text-align: center;
  cursor: pointer;
  transition: all 0.3s;
  background: #fafafa;
}

.file-drop-zone:hover {
  border-color: #1890ff;
  background: #f0f7ff;
}

.file-drop-zone.dragging {
  border-color: #1890ff;
  background: #e6f7ff;
  box-shadow: 0 0 0 3px rgba(24, 144, 255, 0.1);
}

.drop-zone-content {
  display: flex;
  flex-direction: column;
  align-items: center;
}

/* 文件列表面板 */
.file-list-panel {
  width: 320px;
  flex-shrink: 0;
  border: 1px solid #f0f0f0;
  border-radius: 6px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.file-list-header {
  padding: 8px 12px;
  background: #fafafa;
  border-bottom: 1px solid #f0f0f0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 500;
  font-size: 13px;
}

.file-list-body {
  flex: 1;
  overflow-y: auto;
}

.file-item {
  padding: 8px 12px;
  border-bottom: 1px solid #f0f0f0;
  border-left: 3px solid transparent;
  cursor: pointer;
  transition: background 0.15s;
}

.file-item:hover {
  background: #f5f5f5;
}

.file-item.active {
  background: #e6f7ff;
  border-left-color: #1890ff;
}

.file-path {
  font-size: 12px;
  font-family: 'Consolas', 'Courier New', monospace;
  word-break: break-all;
  color: #333;
}

.file-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 3px;
}

.file-desc {
  font-size: 11px;
  color: #999;
}

/* 代码预览面板 */
.code-preview-panel {
  flex: 1;
  min-width: 0;
  border: 1px solid #f0f0f0;
  border-radius: 6px;
  overflow: hidden;
  background: #fafafa;
  position: relative;
}

.monaco-wrapper {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
}

/* 实体选择列表 */
.service-select-list {
  max-height: calc(100vh - 420px);
  overflow-y: auto;
}

.service-list-item {
  cursor: pointer;
  padding: 10px 16px !important;
  transition: background 0.15s, border-color 0.15s;
  border-left: 3px solid transparent;
}

.service-list-item:hover {
  background: #f5f7fa;
}

.service-list-item.selected {
  background: #f0f7ff;
  border-left-color: #1890ff;
}

.service-item-title {
  display: flex;
  align-items: center;
}

.service-model-name {
  font-weight: 600;
  font-size: 14px;
  color: #262626;
}

.service-item-desc {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.service-desc-text {
  font-size: 12px;
  color: #8c8c8c;
}

.service-meta-row {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
}

.service-field-count {
  font-size: 12px;
  color: #8c8c8c;
  margin-left: 4px;
}
</style>
