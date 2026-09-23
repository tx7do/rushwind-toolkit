// Tauri invoke 封装：函数名与 gowind-uiapp 的 wailsjs/go/main/App 逐一对齐，
// 页面代码可按 Go 版复刻；rushwind 特有命令挂在下方 rush 段。
import {invoke} from '@tauri-apps/api/core'
import {open} from '@tauri-apps/plugin-dialog'
import {ai, configexporter, database, detect, devtools, generator, main, rush} from './models'

// ==================== 项目 ====================

export function OpenProject(path: string): Promise<main.OpenProjectResult> {
  return invoke('open_project', {path})
}

export function GetProjectInfo(): Promise<detect.ProjectInfo | null> {
  return invoke('get_project_info')
}

/** Go 版走后端；Tauri 侧直接调原生目录选择对话框。取消时抛错。 */
export async function SelectFolder(): Promise<string> {
  const dir = await open({directory: true, multiple: false})
  if (!dir) throw new Error('cancelled')
  return dir
}

export function CreateProject(opts: devtools.CreateProjectOptions & {storage?: string}): Promise<devtools.CommandResult> {
  return invoke('create_project', {opts})
}

export function CleanConfig(): Promise<void> {
  return invoke('clean_config')
}

export function ProbeRepo(repo: string): Promise<Record<string, any>> {
  return invoke('probe_repo', {repo})
}

// ==================== 后端代码生成（rushwind 实体链） ====================

export function GenEntity(opts: rush.EntityOptionsDto): Promise<rush.EntityReport> {
  return invoke('gen_entity', {opts})
}

export function GenPages(opts: rush.PagesOptionsDto): Promise<rush.PagesReport> {
  return invoke('gen_pages', {opts})
}

export function SpecLoad(repo: string, name: string): Promise<rush.EntitySpecFile> {
  return invoke('spec_load', {repo, name})
}

export function SpecSave(repo: string, spec: rush.EntitySpecFile): Promise<void> {
  return invoke('spec_save', {repo, spec})
}

export function GenUndo(opts: rush.UndoOptions): Promise<rush.UndoReport> {
  return invoke('gen_undo', {opts})
}

export function Doctor(repo: string | null): Promise<rush.DoctorReport> {
  return invoke('doctor', {repo})
}

export function NewProject(opts: rush.NewOptions): Promise<rush.NewReport> {
  return invoke('new_project', {opts})
}

export function Adopt(opts: rush.AdoptOptions): Promise<rush.AdoptReport> {
  return invoke('adopt', {opts})
}

export function ManifestCheck(repo: string, flavor: 'Proto' | 'React'): Promise<rush.ManifestCheckReport> {
  return invoke('manifest_check', {repo, flavor})
}

export function ManifestRebuild(repo: string, flavor: 'Proto' | 'React'): Promise<number> {
  return invoke('manifest_rebuild', {repo, flavor})
}

// ==================== 开发工具 ====================

export function GetDevServices(): Promise<devtools.ServiceInfo[]> {
  return invoke('get_dev_services')
}

export function DevRunService(name: string): Promise<devtools.CommandResult> {
  return invoke('dev_run_service', {name})
}

export function DevStopService(name: string): Promise<devtools.CommandResult> {
  return invoke('dev_stop_service', {name})
}

export function DevCargoCheck(scope: string): Promise<devtools.CommandResult> {
  return invoke('dev_cargo_check', {scope})
}

export function AddService(opts: devtools.AddServiceOptions): Promise<devtools.CommandResult> {
  return invoke('add_service', {opts})
}

// ==================== 生成器选项 ====================

export function GetGeneratorOptions(): Promise<generator.Option[]> {
  return invoke('get_generator_options')
}

export function ImportSpecTables(names: string[]): Promise<string> {
  return invoke('import_spec_tables', {names})
}

export function EditGeneratorOption(option: generator.Option): Promise<void> {
  return invoke('edit_generator_option', {option})
}

export function SetGeneratorOption(options: generator.Option[]): Promise<void> {
  return invoke('set_generator_option', {options})
}

// ==================== 数据库导入 ====================

export function GetDBConfig(): Promise<database.DBConfig> {
  return invoke('get_db_config')
}

export function SetDBConfig(cfg: database.DBConfig): Promise<void> {
  return invoke('set_db_config', {cfg})
}

export function TestDatabaseConnection(cfg: database.DBConfig): Promise<database.ConnectionResult> {
  return invoke('test_database_connection', {cfg})
}

export function GetDatabaseTables(cfg: database.DBConfig): Promise<database.TableInfo[]> {
  return invoke('get_database_tables', {cfg})
}

export function GetTableColumns(cfg: database.DBConfig, table: string): Promise<database.ColumnInfo[]> {
  return invoke('get_table_columns', {cfg, table})
}

export function ImportDatabaseTables(cfg: database.DBConfig): Promise<string> {
  return invoke('import_database_tables', {cfg})
}

export function ImportSqlTables(sql: string): Promise<string> {
  return invoke('import_sql_tables', {sql})
}

export function ImportGoSchemaTables(dir: string, service: string): Promise<string> {
  return invoke('import_go_schema_tables', {dir, service})
}

// ==================== 远程配置 ====================

export function ExportConfigToRemote(cfg: configexporter.RemoteConfig): Promise<configexporter.ExportResult> {
  return invoke('export_config_to_remote', {cfg})
}

export function ExportOneServiceConfig(cfg: configexporter.RemoteConfig, service: string): Promise<configexporter.ExportResult> {
  return invoke('export_one_service_config', {cfg, service})
}

export function GetConfigServices(): Promise<configexporter.ServiceInfo[]> {
  return invoke('get_config_services')
}

export function GetRemoteConfigTypes(): Promise<Array<Record<string, string>>> {
  return invoke('get_remote_config_types')
}

// ==================== AI 助手 ====================

export function GetAIConfig(): Promise<ai.Config> {
  return invoke('get_ai_config')
}

export function SetAIConfig(cfg: ai.Config): Promise<void> {
  return invoke('set_ai_config', {cfg})
}

export function GetAIProviderPresets(): Promise<ai.AIProviderPreset[]> {
  return invoke('get_ai_provider_presets')
}

export function TestAIConnection(): Promise<ai.StepResult> {
  return invoke('test_ai_connection')
}

export function AIFindOpenAPIFiles(): Promise<ai.OpenAPIResult> {
  return invoke('ai_find_openapi_files')
}

export function AIPartitionMicroservices(input: string): Promise<ai.PartitionResult> {
  return invoke('ai_partition_microservices', {input})
}

export function AIGenerateDDL(file: string): Promise<ai.StepResult> {
  return invoke('ai_generate_ddl', {file})
}

export function AIGenerateDDLStream(file: string): Promise<ai.StepResult> {
  return invoke('ai_generate_ddl_stream', {file})
}

export function AIGenerateBackendCode(openapi: string, framework: string, partitions: ai.MicroservicePartition[]): Promise<string> {
  return invoke('ai_generate_backend_code', {openapi, framework, partitions})
}

export function AIReviewCode(files: Record<string, string>): Promise<ai.StepResult> {
  return invoke('ai_review_code', {files})
}

export function AIReviewCodeStream(files: Record<string, string>): Promise<ai.StepResult> {
  return invoke('ai_review_code_stream', {files})
}

// ==================== 一键批量生成（实体链 / 页面链） ====================

/** proto package 策略：'per-table' | 'by-service' | 'custom'；servers 携 check/dry_run 开关。 */
export function GenerateGrpcCode(strategy: string, servers: string[]): Promise<string> {
  return invoke('generate_grpc_code', {strategy, servers})
}

/** stack=前端栈（react/vben/element），group=页面分组目录；servers 携 overwrite/dry_run 开关。 */
export function GenerateRestCode(stack: string, group: string, servers: string[]): Promise<string> {
  return invoke('generate_rest_code', {stack, group, servers})
}
