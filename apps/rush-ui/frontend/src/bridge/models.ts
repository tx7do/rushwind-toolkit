// 与 gowind-uiapp wailsjs/go/models 同名同形状的类型定义（Tauri 侧由
// Rust 命令按相同字段名序列化）；rushwind 特有的报告类型放在 rush 命名空间。

export namespace ai {

  export interface AIProviderPreset {
    name: string;
    value: string;
    baseUrl: string;
  }

  export interface Config {
    provider: string;
    baseUrl: string;
    apiKey: string;
    azureApiVersion?: string;
    model: string;
    temperature: number;
    maxTokens: number;
  }

  export interface MicroservicePartition {
    serviceName: string;
    tables: string[];
    description: string;
  }

  export interface OpenAPIResult {
    success: boolean;
    files: string[];
    message?: string;
    error?: string;
  }

  export interface PartitionResult {
    success: boolean;
    partitions: MicroservicePartition[];
    error?: string;
  }

  export interface StepResult {
    success: boolean;
    content: string;
    error?: string;
  }
}

export namespace configexporter {

  export interface ExportResult {
    success: boolean;
    error?: string;
    service?: string;
    filesCount?: number;
  }

  export interface RemoteConfig {
    type: string;
    endpoint: string;
    projectName: string;
    group: string;
    env: string;
    namespaceId: string;
    username: string;
    password: string;
    caCertPem: string;
    clientCertPem: string;
    clientKeyPem: string;
  }

  export interface ServiceInfo {
    name: string;
    configFiles: string[];
    configFolder: string;
  }
}

export namespace database {

  export interface ColumnInfo {
    name: string;
    type: string;
    nullable: boolean;
    primaryKey: boolean;
    default: string;
    comment: string;
    extra: string;
  }

  export interface DBError {
    code: string;
    message: string;
    details: string;
  }

  export interface ConnectionResult {
    success: boolean;
    message: string;
    database: string;
    serverVer: string;
    duration: number;
    tables: number;
    connected: boolean;
    error?: string;
  }

  export interface DBConfig {
    type: string;
    host: string;
    port: number;
    database: string;
    username: string;
    password: string;
    ssl: boolean;
    dbPath: string;
    useDSN?: boolean;
    dsn?: string;
    sqlContent?: string;
    timeout?: number;
    maxOpenConns?: number;
  }

  export interface TableInfo {
    table_name: string;
    table_type: string;
    table_engine: string;
    table_rows: number;
    table_comment: string;
    table_columns: number;
    table_indexes: number;
    create_time: string;
  }
}

export namespace detect {

  export interface ModuleCandidate {
    Dir: string;
    ModPath: string;
    RelPath: string;
  }

  /** RushWind 仓库形状：与 Go 版 ProjectInfo 同名，字段按 rushwind 仓结构填充。 */
  export interface ProjectInfo {
    Root: string;
    /** workspace/包名（Go 版为 go module 路径）。 */
    ModPath: string;
    Version: string;
    /** backend/services/* 服务目录名。 */
    Services?: string[];
    /** backend/api 契约面在位。 */
    HasApi?: boolean;
    /** 前端栈在位情况。 */
    HasReact?: boolean;
    HasVben?: boolean;
    HasElement?: boolean;
    /** .rush/ 下的实体规格名。 */
    Specs?: string[];
  }
}

export namespace devtools {

  export interface CommandResult {
    success: boolean;
    output: string;
    error?: string;
    dir?: string;
  }

  export interface CreateProjectOptions {
    name: string;
    module: string;
    repoUrl: string;
    branch: string;
    parentDir: string;
  }

  export interface AddServiceOptions {
    serviceName: string;
    servers: string[];
    dbClients: string[];
  }

  /** RushWind 侧：backend/services/* 一览（camelCase 由 Rust 序列化保证）。 */
  export interface ServiceInfo {
    name: string;
    /** src/main.rs（可运行）在位。 */
    hasServer: boolean;
    /** 配置目录在位。 */
    hasConfig: boolean;
  }
}

export namespace frontendgen {

  export interface GeneratedFile {
    path: string;
    content: string;
    description: string;
    serviceName: string;
    type: string;
  }
}

export namespace generator {

  /** 生成器行选项：RushWind 侧一行 = 一个实体（camelCase 由 Rust 序列化保证）。 */
  export interface Option {
    id: number;
    tableName: string;
    service: string;
    exclude: boolean;
    protoPackage: string;
    // ---- rushwind 差异化字段 ----
    table?: string;
    routePrefix?: string;
    fields?: rush.SpecField[];
    codeField?: string;
    global?: boolean;
    authFree?: boolean;
  }
}

export namespace main {

  export interface OpenProjectResult {
    Status: string;
    Project?: detect.ProjectInfo;
    Candidates?: detect.ModuleCandidate[];
  }
}

/** rushwind 特有：rush-gen 报告与选项（serde 原样输出，snake_case）。 */
export namespace rush {

  export interface FieldDto {
    name: string;
    kind: string;
  }

  /** FieldEditor 的行内状态：enum 时 values/default 才有意义。 */
  export interface FieldRow {
    name: string;
    kind: string;
    values: { num: number; text: string }[];
    default: string;
  }

  export interface SpecField {
    name: string;
    kind: string;
  }

  export interface EntitySpecFile {
    schema: number;
    name: string;
    table: string;
    package: string;
    route_prefix: string;
    fields: SpecField[];
    code_field?: string | null;
    global: boolean;
    group?: string | null;
    /** 最近一次 gen pages 的栈（react/vben/element）。 */
    stack?: string | null;
  }

  export interface EntityOptionsDto {
    repo_root: string;
    name: string;
    table: string | null;
    package: string | null;
    route_prefix: string | null;
    fields: FieldDto[];
    code_field: string | null;
    global: boolean;
    check: boolean;
    dry_run: boolean;
    skip_manifest: boolean;
    overwrite: boolean;
    auth_free: boolean;
  }

  export interface PagesOptionsDto {
    repo_root: string;
    name: string;
    group: string | null;
    route_prefix: string | null;
    fields: FieldDto[];
    code_field: string | null;
    stack: 'react' | 'vben' | 'element';
    global: boolean | null;
    overwrite: boolean;
    dry_run: boolean;
  }

  export interface EntityReport {
    created: string[];
    updated: string[];
    diffs: Array<[string, string]>;
    edited: string[];
    skipped: string[];
    manifest_entries?: number | null;
    check_passed?: boolean | null;
    notes: string[];
  }

  export interface PagesReport {
    created: string[];
    updated: string[];
    diffs: Array<[string, string]>;
    edited: string[];
    skipped: string[];
    notes: string[];
  }

  export interface UndoOptions {
    repo_root: string;
    name: string;
    dry_run: boolean;
  }

  export interface UndoReport {
    removed_files: string[];
    removed_dirs: string[];
    edited_files: string[];
    skipped: string[];
    notes: string[];
  }

  export interface DoctorCheck {
    name: string;
    status: 'Ok' | 'Warn' | 'Fail' | string;
    detail: string;
    hint?: string | null;
  }

  export interface DoctorReport {
    checks: DoctorCheck[];
  }

  export interface NewOptions {
    name: string;
    dest: string;
    storage: 'Memory' | 'Sqlite' | 'Postgres' | string;
    template: string | null;
    git: boolean;
    dry_run: boolean;
  }

  export interface NewReport {
    project_dir: string;
    files: string[];
    template_source: string;
    renamed_from?: string | null;
    notes: string[];
  }

  export interface AdoptOptions {
    repo_root: string;
    dry_run: boolean;
    keep_gates: boolean;
    skip_proto: boolean;
    skip_react: boolean;
    prune_upstream_baseline: boolean;
    keep_sync_scripts: boolean;
  }

  export interface AdoptReport {
    proto_entries?: number | null;
    react_entries?: number | null;
    removed_ci_steps: string[];
    ci_gates_already_absent: boolean;
    ci_missing: boolean;
    retired_scripts: string[];
    upstream_baseline: 'Pruned' | 'Kept' | 'Absent' | string;
  }

  export interface ManifestCheckReport {
    added: string[];
    removed: string[];
    modified: string[];
  }
}
