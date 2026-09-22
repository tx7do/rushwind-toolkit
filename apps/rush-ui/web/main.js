// rush-ui 前端逻辑：纯静态 + window.__TAURI__.core.invoke（withGlobalTauri）。
// 参数键与 rush-gen 结构体的 Rust 字段名一致（snake_case，serde 默认形态）。

const invoke = window.__TAURI__?.core?.invoke;

function $(id) {
  return document.getElementById(id);
}

// ---- 标签页切换 ----
for (const button of document.querySelectorAll('nav button')) {
  button.addEventListener('click', () => {
    for (const b of document.querySelectorAll('nav button')) b.classList.remove('active');
    for (const p of document.querySelectorAll('section.panel')) p.classList.remove('active');
    button.classList.add('active');
    $('tab-' + button.dataset.tab).classList.add('active');
  });
}

// ---- 工具 ----
function trimmed(id) {
  return $(id).value.trim();
}

function optional(id) {
  const value = trimmed(id);
  return value === '' ? null : value;
}

function fieldLines(textareaId) {
  const lines = $(textareaId).value
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '');
  if (lines.length === 0) return [];
  return lines.map((line) => {
    const at = line.lastIndexOf(':');
    if (at === -1) throw new Error(`字段格式应为 name:kind：${line}`);
    return { name: line.slice(0, at).trim(), kind: line.slice(at + 1).trim() };
  });
}

function renderReport(preId, statusId, report) {
  const pre = $(preId);
  pre.style.display = 'block';
  pre.textContent = JSON.stringify(report, (key, value) => (value === null ? undefined : value), 2);
  const parts = [];
  if (report.created?.length) parts.push(`新建 ${report.created.length}`);
  if (report.edited?.length) parts.push(`编辑 ${report.edited.length}`);
  if (report.files?.length) parts.push(`文件 ${report.files.length}`);
  if (report.skipped?.length) parts.push(`跳过 ${report.skipped.length}`);
  $(statusId).textContent = parts.length ? `完成：${parts.join('，')}` : '完成';
  $(statusId).className = 'status ok';
}

function renderError(statusId, error) {
  $(statusId).textContent = String(error);
  $(statusId).className = 'status err';
}

// invoke 的参数名 = Rust 命令函数的形参名：gen_* / new / adopt 的单参叫
// opts；manifest 两命令是 repo + flavor。
async function runCommand(buttonId, statusId, reportId, command, argsBuilder) {
  const button = $(buttonId);
  button.disabled = true;
  $(statusId).textContent = '运行中…';
  $(statusId).className = 'status';
  try {
    if (!invoke) throw new Error('未在 Tauri 环境内（请通过 rush-ui 应用打开）');
    const report = await invoke(command, argsBuilder());
    renderReport(reportId, statusId, report);
  } catch (error) {
    renderError(statusId, error);
  } finally {
    button.disabled = false;
  }
}

// ---- gen entity ----
$('e-run').addEventListener('click', () => {
  let fields;
  try {
    fields = fieldLines('e-fields');
  } catch (error) {
    return renderError('e-status', error);
  }
  if (fields.length === 0) return renderError('e-status', '至少一个业务字段（name:kind）');
  const opts = {
    repo_root: trimmed('e-repo'),
    name: trimmed('e-name'),
    table: optional('e-table'),
    package: optional('e-package'),
    route_prefix: optional('e-route'),
    fields,
    code_field: optional('e-code'),
    global: $('e-global').checked,
    check: $('e-check').checked,
    dry_run: $('e-dry').checked,
    skip_manifest: false,
  };
  if (!opts.repo_root) return renderError('e-status', '仓库根目录必填');
  if (!opts.name) return renderError('e-status', '实体名必填');
  runCommand('e-run', 'e-status', 'e-report', 'gen_entity', { opts });
});

// ---- gen pages ----
$('p-run').addEventListener('click', () => {
  let fields;
  try {
    fields = fieldLines('p-fields');
  } catch (error) {
    return renderError('p-status', error);
  }
  const opts = {
    repo_root: trimmed('p-repo'),
    name: trimmed('p-name'),
    group: optional('p-group'),
    route_prefix: optional('p-route'),
    fields,
    code_field: optional('p-code'),
    stack: $('p-stack').value,
    dry_run: $('p-dry').checked,
  };
  if (!opts.repo_root) return renderError('p-status', '仓库根目录必填');
  if (!opts.name) return renderError('p-status', '实体名必填');
  runCommand('p-run', 'p-status', 'p-report', 'gen_pages', { opts });
});

// ---- rush new ----
$('n-run').addEventListener('click', () => {
  const opts = {
    name: trimmed('n-name'),
    dest: trimmed('n-dir') || '.',
    storage: $('n-storage').value,
    template: optional('n-template'),
    git: $('n-git').checked,
    dry_run: $('n-dry').checked,
  };
  if (!opts.name) return renderError('n-status', '项目名必填');
  runCommand('n-run', 'n-status', 'n-report', 'new_project', { opts });
});

// ---- adopt ----
$('a-run').addEventListener('click', () => {
  const opts = {
    repo_root: trimmed('a-repo'),
    dry_run: $('a-dry').checked,
    keep_gates: $('a-gates').checked,
    skip_proto: false,
    skip_react: false,
    prune_upstream_baseline: $('a-prune').checked,
    keep_sync_scripts: $('a-scripts').checked,
  };
  if (!opts.repo_root) return renderError('a-status', '仓库根目录必填');
  runCommand('a-run', 'a-status', 'a-report', 'adopt', { opts });
});

// ---- manifest ----
$('m-check').addEventListener('click', () => {
  const repo = trimmed('m-repo');
  if (!repo) return renderError('m-status', '仓库根目录必填');
  runCommand('m-check', 'm-status', 'm-report', 'manifest_check', {
    repo,
    flavor: $('m-flavor').value,
  });
});

$('m-rebuild').addEventListener('click', () => {
  const repo = trimmed('m-repo');
  if (!repo) return renderError('m-status', '仓库根目录必填');
  runCommand('m-rebuild', 'm-status', 'm-report', 'manifest_rebuild', {
    repo,
    flavor: $('m-flavor').value,
  });
});
