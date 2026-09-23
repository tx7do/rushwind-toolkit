<script setup lang="ts">
import {ref, reactive, computed, watch, onMounted, onUnmounted} from 'vue'
import {message} from 'ant-design-vue'
import {useI18n} from 'vue-i18n'
import {
  FolderOpenOutlined,
  PlusOutlined,
  ReloadOutlined,
  PlayCircleOutlined,
  PauseCircleOutlined,
  ThunderboltOutlined,
  ToolOutlined,
  ClearOutlined,
  StopOutlined,
} from '@ant-design/icons-vue'
import {
  AddService,
  CleanConfig,
  DevCargoCheck,
  DevRunService,
  DevStopService,
  Doctor,
  GetDevServices,
  ManifestCheck,
  ManifestRebuild,
} from "../../bridge/App";
import {EventsOn, EventsOff} from "../../bridge/runtime";
import {useProject} from "../../stores/project";

const {t} = useI18n()

const {projectInfo, hasProject, selectAndOpenProject} = useProject()

const services = ref<any[]>([])
const selectedRowKeys = ref<string[]>([])
const loading = ref(false)
const outputText = ref('')

// 运行中的服务（cargo run 子进程由 Rust 侧托管，状态经 dev-log/dev-exit 事件回写）。
const runningServices = ref<string[]>([])

// ==================== 输出管理 ====================
function appendOutput(cmd: string, result: any) {
  const now = new Date().toLocaleTimeString()
  const success = result.success
  let text = `> ${cmd}\n`
  text += `  [${now}] ${success ? 'OK' : 'FAIL'}\n`
  if (result.output) text += result.output + '\n'
  if (result.error) text += 'Error: ' + result.error + '\n'
  text += '\n'
  outputText.value = text + outputText.value
}

function appendLine(line: string) {
  outputText.value = line + '\n' + outputText.value
}

// ==================== 项目管理 ====================
async function loadServices() {
  try {
    const list = await GetDevServices()
    services.value = list || []
  } catch (e) {
    services.value = []
  }
}

// ==================== 添加服务 Modal（复用 rush new 内嵌模板，落到 backend/services/<name>） ====================
const addVisible = ref(false)
const adding = ref(false)
const addForm = reactive({
  serviceName: '',
  servers: ['rest'] as string[],
  dbClients: ['sqlx'] as string[],
})

const serverOptions = [
  {label: 'REST (axum)', value: 'rest'},
  {label: 'gRPC', value: 'grpc'},
]
const dbClientOptions = [
  {label: 'SQLx', value: 'sqlx'},
  {label: 'SeaORM', value: 'seaorm'},
  {label: 'Redis', value: 'redis'},
]

function openAddModal() {
  addForm.serviceName = ''
  addForm.servers = ['rest']
  addForm.dbClients = ['sqlx']
  addVisible.value = true
}

async function handleAddService() {
  if (!addForm.serviceName.trim()) {
    message.warning(t('devTools.addService.nameRequired'))
    return
  }
  adding.value = true
  try {
    const result = await AddService({
      serviceName: addForm.serviceName,
      servers: addForm.servers,
      dbClients: addForm.dbClients,
    })
    if (result.success) {
      message.success(t('devTools.addService.success'))
      addVisible.value = false
      await loadServices()
    } else {
      message.error(result.error || t('devTools.addService.failed'))
    }
  } catch (e: any) {
    message.error(String(e.message || e))
  } finally {
    adding.value = false
  }
}

// ==================== 命令执行 ====================
async function execCommand(cmdLabel: string, fn: () => Promise<any>) {
  loading.value = true
  try {
    const result = await fn()
    appendOutput(cmdLabel, result)
    if (result.success) {
      message.success(t('devTools.commands.success'))
    } else {
      message.error(result.error || t('devTools.commands.failed'))
    }
  } catch (e: any) {
    appendOutput(cmdLabel, {success: false, error: e.toString()})
    message.error(String(e.message || e))
  } finally {
    loading.value = false
  }
}

// -- 单点 --
function handleRunService(name: string) {
  execCommand(`cargo run -p ${name}`, () => DevRunService(name).then(r => {
    if (r.success) runningServices.value = [...runningServices.value, name]
    return r
  }))
}
function handleStopService(name: string) {
  execCommand(`stop ${name}`, () => DevStopService(name).then(r => {
    if (r.success) runningServices.value = runningServices.value.filter(n => n !== name)
    return r
  }))
}
function handleCargoCheck(name: string) {
  execCommand(`cargo check -p ${name}`, () => DevCargoCheck(name))
}

// -- 群控 --
function handleDoctor() {
  execCommand('rush doctor', async () => {
    const report = await Doctor(projectInfo.value?.Root ?? null)
    const lines = (report.checks ?? [])
      .map(c => `[${c.status}] ${c.name}: ${c.detail}${c.hint ? ` — ${c.hint}` : ''}`)
      .join('\n')
    return {success: !lines.includes('[Fail]'), output: lines}
  })
}
function handleManifestCheck() {
  execCommand('manifest check', async () => {
    const root = projectInfo.value?.Root ?? ''
    const parts: string[] = []
    let ok = true
    for (const flavor of ['Proto', 'React'] as const) {
      try {
        const r = await ManifestCheck(root, flavor)
        const n = (r.added?.length ?? 0) + (r.removed?.length ?? 0) + (r.modified?.length ?? 0)
        ok = ok && n === 0
        parts.push(`${flavor}: ${n === 0 ? 'OK' : `${n} 项漂移`}`)
        if (n > 0) {
          parts.push(JSON.stringify(r))
        }
      } catch (e: any) {
        ok = false
        parts.push(`${flavor}: ${e.message || e}`)
      }
    }
    return {success: ok, output: parts.join('\n')}
  })
}
function handleManifestRebuild() {
  execCommand('manifest rebuild', async () => {
    const root = projectInfo.value?.Root ?? ''
    const parts: string[] = []
    for (const flavor of ['Proto', 'React'] as const) {
      try {
        const n = await ManifestRebuild(root, flavor)
        parts.push(`${flavor}: ${n} 条目`)
      } catch (e: any) {
        parts.push(`${flavor}: ${e.message || e}`)
      }
    }
    return {success: true, output: parts.join('\n')}
  })
}
function handleCargoCheckAll() {
  execCommand('cargo check --workspace', () => DevCargoCheck(''))
}
function handleCleanConfig() {
  execCommand('clean config', async () => {
    await CleanConfig()
    return {success: true, output: ''}
  })
}

// -- 批量 --
const hasSelection = computed(() => selectedRowKeys.value.length > 0)

async function batchExec(cmdLabel: string, fn: (name: string) => Promise<any>) {
  if (selectedRowKeys.value.length === 0) {
    message.warning(t('devTools.commands.noSelection'))
    return
  }
  loading.value = true
  for (const name of selectedRowKeys.value) {
    try {
      const result = await fn(name)
      appendOutput(`${cmdLabel} ${name}`, result)
    } catch (e: any) {
      appendOutput(`${cmdLabel} ${name}`, {success: false, error: e.toString()})
    }
  }
  loading.value = false
  message.success(t('devTools.commands.batchDone'))
}

function handleBatchRun() { batchExec('run', (n) => DevRunService(n)) }
function handleBatchStop() { batchExec('stop', (n) => DevStopService(n)) }
function handleBatchCheck() { batchExec('cargo check', (n) => DevCargoCheck(n)) }

// ==================== 表格选择 ====================
const rowSelection = computed(() => ({
  selectedRowKeys: selectedRowKeys.value,
  onChange: (keys: string[]) => { selectedRowKeys.value = keys },
}))

// ==================== 工具 ====================
function clearOutput() { outputText.value = '' }

// cargo run 子进程的日志流：逐行落到输出面板。
function onDevLog(data: any) {
  if (!data || typeof data.line !== 'string') return
  appendLine(`[${data.name}] ${data.line}`)
}

function onDevExit(data: any) {
  if (!data || typeof data.name !== 'string') return
  runningServices.value = runningServices.value.filter(n => n !== data.name)
  appendLine(`[${data.name}] exited (code ${data.code ?? '?'})`)
}

onMounted(() => {
  EventsOn('dev-log', onDevLog)
  EventsOn('dev-exit', onDevExit)
})

onUnmounted(() => {
  EventsOff('dev-log')
  EventsOff('dev-exit')
})

// 项目为全局状态：本页首次挂载时 immediate 拉一次服务列表，之后切换项目自动刷新。
watch(projectInfo, () => {
  selectedRowKeys.value = []
  services.value = []
  runningServices.value = []
  if (hasProject.value) loadServices()
}, {immediate: true})
</script>

<template>
  <div class="devtools-page">
    <!-- 顶部：未打开项目时给出入口；已打开项目由顶栏全局展示，本页不重复 -->
    <div class="top-bar">
      <template v-if="!hasProject">
        <a-button type="primary" @click="selectAndOpenProject">
          <FolderOpenOutlined style="margin-right: 4px"/> {{ t('backend.project.clickToOpen') }}
        </a-button>
        <span class="project-path project-path--empty">{{ t('app.noProject') }}</span>
      </template>

      <div class="spacer"/>

      <a-button size="small" type="primary" ghost :disabled="!hasProject" @click="openAddModal"><PlusOutlined style="margin-right: 4px"/> {{ t('devTools.addService.btn') }}</a-button>
      <a-button size="small" @click="loadServices" :disabled="!hasProject"><ReloadOutlined style="margin-right: 4px"/> {{ t('common.refresh') }}</a-button>
    </div>

    <!-- 群控 + 批量按钮栏 -->
    <div class="global-actions" v-if="hasProject">
      <span class="action-group-label">{{ t('devTools.commands.globalActions') }}</span>
      <a-button size="small" type="primary" ghost :loading="loading" @click="handleDoctor"><ToolOutlined style="margin-right: 4px"/> {{ t('devTools.commands.doctor') }}</a-button>
      <a-button size="small" ghost :loading="loading" @click="handleManifestCheck">{{ t('devTools.commands.manifestCheck') }}</a-button>
      <a-button size="small" ghost :loading="loading" @click="handleManifestRebuild">{{ t('devTools.commands.manifestRebuild') }}</a-button>
      <a-button size="small" type="dashed" :loading="loading" @click="handleCargoCheckAll"><ThunderboltOutlined style="margin-right: 4px"/> {{ t('devTools.commands.cargoCheck') }}</a-button>
      <a-button size="small" danger ghost :loading="loading" @click="handleCleanConfig"><StopOutlined style="margin-right: 4px"/> {{ t('devTools.commands.cleanConfig') }}</a-button>

      <a-divider type="vertical" style="height: 24px; margin: 0 8px"/>

      <span class="action-group-label">
        {{ t('devTools.commands.batchActions') }}
        <a-tag v-if="hasSelection" color="blue" style="margin-left: 4px">{{ selectedRowKeys.length }}</a-tag>
      </span>
      <a-button size="small" type="primary" ghost :loading="loading" :disabled="!hasSelection" @click="handleBatchRun">{{ t('devTools.commands.runService') }}</a-button>
      <a-button size="small" ghost :loading="loading" :disabled="!hasSelection" @click="handleBatchStop">{{ t('devTools.commands.stopService') }}</a-button>
      <a-button size="small" ghost :loading="loading" :disabled="!hasSelection" @click="handleBatchCheck">{{ t('devTools.commands.cargoCheck') }}</a-button>
    </div>

    <!-- 服务表格 -->
    <div class="table-section" v-if="hasProject">
      <a-table
          :data-source="services"
          :row-selection="rowSelection"
          :row-key="(record: any) => record.name"
          :pagination="false"
          :scroll="{ x: 800 }"
          size="small"
          bordered
      >
        <a-table-column dataIndex="name" :title="t('devTools.addService.serviceName')" :width="140"/>
        <a-table-column :title="t('projectManager.overview.hasServer')" :width="70" align="center">
          <template #default="{record}">
            <a-tag v-if="record.hasServer" color="green">OK</a-tag>
            <span v-else class="muted">-</span>
          </template>
        </a-table-column>
        <a-table-column :title="t('projectManager.overview.hasConfig')" :width="70" align="center">
          <template #default="{record}">
            <a-tag v-if="record.hasConfig" color="blue">OK</a-tag>
            <span v-else class="muted">-</span>
          </template>
        </a-table-column>
        <a-table-column :title="t('devTools.commands.running')" :width="80" align="center">
          <template #default="{record}">
            <a-tag v-if="runningServices.includes(record.name)" color="processing"><PlayCircleOutlined style="margin-right: 4px"/> {{ t('devTools.commands.running') }}</a-tag>
            <span v-else class="muted">-</span>
          </template>
        </a-table-column>
        <a-table-column :title="t('devTools.commands.title')" :width="260">
          <template #default="{record}">
            <div class="row-actions">
              <a-button size="small" type="primary" :loading="loading" :disabled="!record.hasServer || runningServices.includes(record.name)" @click="handleRunService(record.name)"><PlayCircleOutlined style="margin-right: 4px"/> {{ t('devTools.commands.runService') }}</a-button>
              <a-button size="small" danger :loading="loading" :disabled="!runningServices.includes(record.name)" @click="handleStopService(record.name)"><PauseCircleOutlined style="margin-right: 4px"/> {{ t('devTools.commands.stopService') }}</a-button>
              <a-button size="small" type="dashed" :loading="loading" @click="handleCargoCheck(record.name)">{{ t('devTools.commands.cargoCheck') }}</a-button>
            </div>
          </template>
        </a-table-column>
      </a-table>
    </div>

    <!-- 无项目提示 -->
    <div v-else class="empty-state">
      <a-empty :description="t('devTools.service.noServices')"/>
    </div>

    <!-- 输出面板 -->
    <div class="output-panel" v-if="hasProject">
      <div class="output-header">
        <span class="panel-title">{{ t('devTools.output.title') }}</span>
        <a-button size="small" type="link" @click="clearOutput" :disabled="!outputText"><ClearOutlined style="margin-right: 4px"/> {{ t('devTools.output.clear') }}</a-button>
      </div>
      <div class="output-content" v-if="outputText"><pre>{{ outputText }}</pre></div>
      <div class="output-content output-empty" v-else>{{ t('devTools.output.empty') }}</div>
    </div>

    <!-- ==================== 添加服务 Modal ==================== -->
    <a-modal v-model:open="addVisible" :title="t('devTools.addService.title')" :confirm-loading="adding" @ok="handleAddService" :width="480">
      <a-form layout="vertical" style="margin-top: 16px">
        <a-form-item :label="t('devTools.addService.serviceName')" required>
          <a-input v-model:value="addForm.serviceName" :placeholder="t('devTools.addService.namePlaceholder')"/>
        </a-form-item>
        <a-form-item :label="t('devTools.addService.servers')">
          <a-checkbox-group v-model:value="addForm.servers">
            <a-checkbox v-for="opt in serverOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</a-checkbox>
          </a-checkbox-group>
        </a-form-item>
        <a-form-item :label="t('devTools.addService.dbClients')">
          <a-checkbox-group v-model:value="addForm.dbClients">
            <a-checkbox v-for="opt in dbClientOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</a-checkbox>
          </a-checkbox-group>
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<style scoped>
.devtools-page {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.top-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid #f0f0f0;
  flex-shrink: 0;
}

.project-path { font-size: 13px; color: #595959; }
.project-path--empty { color: #bfbfbf; }
.spacer { flex: 1; }

.global-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 0;
  flex-shrink: 0;
  flex-wrap: wrap;
  border-bottom: 1px solid #f0f0f0;
}

.action-group-label {
  font-size: 12px;
  font-weight: 600;
  color: #8c8c8c;
  margin-right: 2px;
  white-space: nowrap;
}

.table-section { flex-shrink: 0; overflow: hidden; }
.table-section :deep(.ant-table-wrapper) { width: 100%; }
.table-section :deep(.ant-table) { font-size: 13px; }
.table-section :deep(.ant-table-cell) { padding: 6px 8px !important; }

.row-actions { display: flex; gap: 4px; flex-wrap: nowrap; }
.muted { color: #d9d9d9; }

.empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.output-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 120px;
  border-top: 1px solid #f0f0f0;
  margin-top: 8px;
}

.output-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 0;
  flex-shrink: 0;
}

.panel-title { font-weight: 600; font-size: 13px; color: #262626; }

.output-content {
  flex: 1;
  overflow: auto;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  font-size: 12px;
  line-height: 1.6;
}
.output-content pre { margin: 0; white-space: pre-wrap; word-break: break-all; color: #262626; }
.output-empty { display: flex; align-items: center; justify-content: center; color: #bfbfbf; font-size: 13px; }
</style>
