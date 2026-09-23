// rush-ui 前端逻辑：纯静态 + window.__TAURI__.core.invoke（withGlobalTauri）。
// 命令参数经 DTO（字段 kind 用规格串），与 rush-gen 的 Options 形状一致。

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

// ---- 小工具 ----
function trimmed(id) {
  return $(id).value.trim();
}

function optional(id) {
  const value = trimmed(id);
  return value === '' ? null : value;
}

function esc(text) {
  return String(text).replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
}

const SNAKE_RE = /^[a-z][a-z0-9_]*$/;
const UPPER_RE = /^[A-Z][A-Z0-9_]*$/;

// ---- 本地持久化 ----
function remember(key, value) {
  try { localStorage.setItem('rushui.' + key, JSON.stringify(value)); } catch { /* 无痕模式等场景忽略 */ }
}

function recall(key, fallback) {
  try {
    const raw = localStorage.getItem('rushui.' + key);
    return raw === null ? fallback : JSON.parse(raw);
  } catch {
    return fallback;
  }
}

function bindPersist(id, key) {
  const el = $(id);
  const saved = recall(key);
  if (saved !== null && saved !== undefined && typeof saved === typeof el.value) el.value = saved;
  if (el.tagName === 'INPUT' && el.type === 'checkbox') {
    if (typeof saved === 'boolean') el.checked = saved;
    el.addEventListener('change', () => remember(key, el.checked));
  } else {
    el.addEventListener('change', () => remember(key, el.value));
  }
}

// ---- 仓库探测 ----
let probeTimer = null;
function probeDebounced(repoId, probeId) {
  clearTimeout(probeTimer);
  probeTimer = setTimeout(() => probeNow(repoId, probeId), 350);
}

async function probeNow(repoId, probeId) {
  const repo = trimmed(repoId);
  const box = $(probeId);
  if (!repo || !invoke) {
    box.innerHTML = '';
    return;
  }
  try {
    const probe = await invoke('probe_repo', { repo });
    const chip = (on, label) => `<span class="chip ${on ? 'on' : 'off'}">${on ? '✓' : '✗'} ${label}</span>`;
    let html = [
      chip(probe.backend, 'backend'),
      chip(probe.react, 'react'),
      chip(probe.vben, 'vben'),
      chip(probe.element, 'element'),
      chip(probe.seed_rs, 'seed.rs'),
    ].join('');
    if (probe.specs.length) {
      html += ` <span class="chip spec">规格: ${probe.specs.map(esc).join('、')}</span>`;
    }
    box.innerHTML = html;
  } catch {
    box.innerHTML = '';
  }
}

function wireRepo(repoId, probeId) {
  const el = $(repoId);
  const saved = recall('repo.' + repoId);
  if (saved) el.value = saved;
  el.addEventListener('change', () => {
    remember('repo.' + repoId, el.value.trim());
    probeDebounced(repoId, probeId);
  });
  probeDebounced(repoId, probeId);
}

// ---- 字段编辑器（结构化：标量/枚举取值集/默认值） ----

const KIND_CHOICES = [
  ['string', 'string'],
  ['i32', 'i32'],
  ['u32', 'u32'],
  ['bool', 'bool'],
  ['f64', 'f64'],
  ['enum', 'enum(…)'],
];

function newField(name = '') {
  return {
    name,
    kind: 'string',
    // 枚举取值集（kind === 'enum' 时使用）
    values: [
      { num: 0, text: 'OFF' },
      { num: 1, text: 'ON' },
    ],
    default: 'OFF',
  };
}

function createFieldEditor(prefix) {
  let fields = [newField()];
  const root = $(prefix + '-fields-editor');

  function enumRowsHtml(field) {
    return field.values
      .map(
        (value, index) => `
      <div class="enum-row">
        <input type="number" class="num" data-vi="${index}" value="${value.num}" />
        <input type="text" class="text mono" data-vi="${index}" value="${esc(value.text)}" placeholder="取值文本 (UPPER_SNAKE)" />
        <button class="v-del" data-vi="${index}" title="删除取值">✕</button>
      </div>`,
      )
      .join('');
  }

  function rowHtml(field, index) {
    const enumEditor =
      field.kind === 'enum'
        ? `<div class="enum-editor" data-enum="${index}">
            <div class="enum-head">取值集（数值必须含 0；未识别值回退到缺省文本）</div>
            ${enumRowsHtml(field)}
            <div class="enum-foot">
              <button class="linkish v-add" data-enum="${index}">＋ 取值</button>
              <label>缺省/回退
                <select class="def" data-enum="${index}">${field.values
                  .map((v) => `<option ${v.text === field.default ? 'selected' : ''}>${esc(v.text)}</option>`)
                  .join('')}</select>
              </label>
            </div>
          </div>`
        : '';
    return `
    <div class="field-block" data-fi="${index}">
      <div class="field-row">
        <input type="text" class="f-name mono" data-fi="${index}" value="${esc(field.name)}" placeholder="字段名 (snake_case)" />
        <select class="f-kind" data-fi="${index}">
          ${KIND_CHOICES.map(([v, label]) => `<option value="${v}" ${field.kind === v ? 'selected' : ''}>${label}</option>`).join('')}
        </select>
        <button class="f-del" data-fi="${index}" title="删除字段">✕</button>
      </div>
      ${enumEditor}
    </div>`;
  }

  function render() {
    root.innerHTML = fields.map((field, index) => rowHtml(field, index)).join('');
    onFieldsChanged();
  }

  function onFieldsChanged() {
    // code-field 下拉随 string 字段刷新（entity 侧）
    const codeSelect = $(prefix === 'e' ? 'e-code' : null);
    if (codeSelect) {
      const prev = codeSelect.value;
      const names = fields.filter((f) => f.kind === 'string' && f.name.trim()).map((f) => f.name.trim());
      codeSelect.innerHTML =
        '<option value="">（无）</option>' +
        names.map((name) => `<option ${name === prev ? 'selected' : ''}>${esc(name)}</option>`).join('');
    }
  }

  // 事件委托：状态更新不整树重绘（保住输入焦点）
  root.addEventListener('input', (event) => {
    const target = event.target;
    const block = target.closest('.field-block');
    if (!block) return;
    const index = Number(block.dataset.fi);
    if (target.classList.contains('f-name')) fields[index].name = target.value;
    else if (target.classList.contains('f-kind')) {
      fields[index].kind = target.value;
      render();
    } else if (target.classList.contains('num')) fields[index].values[Number(target.dataset.vi)].num = target.value;
    else if (target.classList.contains('text')) {
      const vi = Number(target.dataset.vi);
      const before = fields[index].values[vi].text;
      fields[index].values[vi].text = target.value;
      if (fields[index].default === before) fields[index].default = target.value;
    }
  });

  root.addEventListener('click', (event) => {
    const target = event.target.closest('button');
    if (!target) return;
    const block = target.closest('.field-block');
    if (target.classList.contains('f-del')) {
      fields.splice(Number(block.dataset.fi), 1);
      if (fields.length === 0) fields = [newField()];
      render();
    } else if (target.classList.contains('v-del')) {
      const field = fields[Number(block.dataset.fi)];
      const removed = field.values.splice(Number(target.dataset.vi), 1)[0];
      if (field.default === removed.text) field.default = field.values[0]?.text ?? '';
      render();
    } else if (target.classList.contains('v-add')) {
      const field = fields[Number(target.dataset.enum)];
      field.values.push({ num: field.values.length, text: '' });
      render();
    }
  });

  root.addEventListener('change', (event) => {
    const target = event.target;
    if (target.classList.contains('def')) {
      fields[Number(target.dataset.enum)].default = target.value;
    }
  });

  return {
    getFields: () => fields,
    setFields(next) {
      fields = next.length ? next : [newField()];
      render();
    },
    clear() {
      fields = [newField()];
      render();
    },
    render,
  };
}

/** 把编辑器状态折叠成 DTO 字段（kind 规格串）；非法即抛中文错误。 */
function fieldsToDto(list, label) {
  const seen = new Set();
  return list.map((field) => {
    const name = field.name.trim();
    if (!name) throw new Error(`${label}存在未命名的字段`);
    if (!SNAKE_RE.test(name)) throw new Error(`字段名须为 snake_case：${name}`);
    if (seen.has(name)) throw new Error(`字段重复：${name}`);
    seen.add(name);
    let kind = field.kind;
    if (kind === 'enum') {
      const seenNums = new Set();
      const seenTexts = new Set();
      const parts = [];
      let zeroText = null;
      for (const value of field.values) {
        const text = value.text.trim();
        const num = Number(value.num);
        if (!text) throw new Error(`字段 ${name} 的枚举取值缺文本`);
        if (!UPPER_RE.test(text)) throw new Error(`字段 ${name} 的枚举文本须为 UPPER_SNAKE：${text}`);
        if (!Number.isInteger(num)) throw new Error(`字段 ${name} 的枚举数值非法：${value.num}`);
        if (seenNums.has(num)) throw new Error(`字段 ${name} 的枚举数值重复：${num}`);
        if (seenTexts.has(text)) throw new Error(`字段 ${name} 的枚举文本重复：${text}`);
        if (num === 0) zeroText = text;
        seenNums.add(num);
        seenTexts.add(text);
        parts.push(`${num}=${text}`);
      }
      if (zeroText === null) throw new Error(`字段 ${name} 的枚举必须含 0 值项`);
      const fallback = field.default.trim() || zeroText;
      kind = `enum(${parts.join(',')})` + (fallback !== zeroText ? `@default=${fallback}` : '');
    }
    return { name, kind };
  });
}

/** 规格串 → 编辑器状态（gen pages 显式字段 / 实体交接用）。 */
function dtoToFields(dto) {
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

const entityEditor = createFieldEditor('e');
const pagesEditor = createFieldEditor('p');
entityEditor.render();
pagesEditor.render();
$('e-add-field').addEventListener('click', () => {
  const list = entityEditor.getFields();
  list.push(newField());
  entityEditor.setFields(list);
});
$('p-add-field').addEventListener('click', () => {
  const list = pagesEditor.getFields();
  list.push(newField());
  pagesEditor.setFields(list);
});

// ---- 结构化报告渲染 ----

function card(title, items, markerClass) {
  if (!items || items.length === 0) return '';
  return `
    <div class="card ${markerClass === 'note' ? 'notes' : ''}">
      <div class="head"><span>${title}</span><span class="count">${items.length}</span></div>
      <ul>${items.map((item) => `<li class="${markerClass}">${esc(item)}</li>`).join('')}</ul>
    </div>`;
}

function renderReport(containerId, report, opts) {
  const box = $(containerId);
  const sections = [
    card('新建文件', report.created, 'created'),
    card('编辑文件', report.edited, 'edited'),
    card('生成文件', report.files, 'created'),
    card('跳过', report.skipped, 'note'),
  ];
  let check = '';
  if (opts?.showCheck && report.check_passed !== null && report.check_passed !== undefined) {
    check =
      report.check_passed === true
        ? '<span class="badge ok">cargo check 通过</span>'
        : '<span class="badge err">cargo check 未通过</span>';
  }
  const notes = (report.notes || []).map((note) => esc(note));
  box.innerHTML = `
    <div class="banner ${opts?.dryRun ? 'dry' : 'ok'}">
      ${opts?.dryRun ? '🗒 dry-run 预览（未落盘）' : '✅ 完成'}
      <span style="font-weight: 400;">新建/生成 ${report.created?.length || report.files?.length || 0} · 编辑 ${report.edited?.length || 0} · 跳过 ${report.skipped?.length || 0}</span>
      ${check}
    </div>
    <div class="report" style="display: flex;">
      ${sections.join('')}
      ${card('注意事项', notes, 'note')}
    </div>
    <details class="raw"><summary>原始 JSON</summary><pre>${esc(JSON.stringify(report, null, 2))}</pre></details>`;
}

function renderError(containerId, error) {
  $(containerId).innerHTML = `<div class="banner err">❌ ${esc(String(error))}</div>`;
}

async function runCommand(buttonId, statusId, resultId, command, argsBuilder, opts) {
  const button = $(buttonId);
  button.disabled = true;
  $(statusId).textContent = '运行中…';
  $(statusId).className = 'status';
  try {
    if (!invoke) throw new Error('未在 Tauri 环境内（请通过 rush-ui 应用打开）');
    const report = await invoke(command, argsBuilder());
    $(statusId).textContent = '完成';
    $(statusId).className = 'status ok';
    renderReport(resultId, report, opts);
    return report;
  } catch (error) {
    $(statusId).textContent = '失败';
    $(statusId).className = 'status err';
    renderError(resultId, error);
    return null;
  } finally {
    button.disabled = false;
  }
}

// ---- 实体 → 页面 交接 ----
let handoff = recall('handoff', null);
function showHandoff() {
  if (!handoff) return;
  $('p-handoff').style.display = 'flex';
  $('p-handoff-text').textContent = `已就绪交接：实体「${handoff.name}」的 ${handoff.fields.length} 个字段`;
}
$('e-goto-pages').addEventListener('click', () => {
  document.querySelector('nav button[data-tab="pages"]').click();
});
$('p-handoff-load').addEventListener('click', () => {
  if (!handoff) return;
  $('p-name').value = handoff.name;
  pagesEditor.setFields(dtoToFields(handoff.fields));
  $('p-explicit').checked = true;
  $('p-fields-editor-wrap').style.display = 'block';
  if (handoff.code_field) $('p-code').value = handoff.code_field;
  $('p-handoff').style.display = 'none';
});

// ---- gen entity ----
$('e-run').addEventListener('click', () => {
  let fields;
  try {
    fields = fieldsToDto(entityEditor.getFields(), '实体字段');
  } catch (error) {
    return renderError('e-result', error);
  }
  if (fields.length === 0) return renderError('e-result', '至少一个业务字段');
  const codeSelect = $('e-code');
  const opts = {
    repo_root: trimmed('e-repo'),
    name: trimmed('e-name'),
    table: optional('e-table'),
    package: optional('e-package'),
    route_prefix: optional('e-route'),
    fields,
    code_field: codeSelect.value === '' ? null : codeSelect.value,
    global: $('e-global').checked,
    check: $('e-check').checked,
    overwrite: $('e-regen').checked,
    auth_free: $('e-authfree').checked,
    dry_run: $('e-dry').checked,
    skip_manifest: false,
  };
  if (!opts.repo_root) return renderError('e-result', '仓库根目录必填');
  if (!SNAKE_RE.test(opts.name)) return renderError('e-result', `实体名须为 snake_case：${opts.name || '（空）'}`);
  runCommand('e-run', 'e-status', 'e-result', 'gen_entity', { opts }, { dryRun: opts.dry_run, showCheck: true }).then(
    (report) => {
      if (report && !opts.dry_run) {
        // 实体成功落盘 → 准备交接 + 刷新探测
        handoff = { name: opts.name, fields, code_field: opts.code_field };
        remember('handoff', handoff);
        showHandoff();
        $('e-goto-pages').style.display = '';
        probeNow('e-repo', 'e-probe');
      }
    },
  );
});

// ---- gen pages ----
$('p-explicit').addEventListener('change', (event) => {
  $('p-fields-editor-wrap').style.display = event.target.checked ? 'block' : 'none';
});
bindPersist('p-stack', 'pages.stack');
$('p-run').addEventListener('click', () => {
  let fields = [];
  try {
    if ($('p-explicit').checked) fields = fieldsToDto(pagesEditor.getFields(), '页面字段');
  } catch (error) {
    return renderError('p-result', error);
  }
  const opts = {
    repo_root: trimmed('p-repo'),
    name: trimmed('p-name'),
    group: optional('p-group'),
    route_prefix: optional('p-route'),
    fields,
    code_field: optional('p-code'),
    stack: $('p-stack').value,
    global: $('p-global').checked ? true : null,
    overwrite: $('p-regen').checked,
    dry_run: $('p-dry').checked,
  };
  if (!opts.repo_root) return renderError('p-result', '仓库根目录必填');
  if (!SNAKE_RE.test(opts.name)) return renderError('p-result', `实体名须为 snake_case：${opts.name || '（空）'}`);
  runCommand('p-run', 'p-status', 'p-result', 'gen_pages', { opts }, { dryRun: opts.dry_run });
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
  if (!opts.name) return renderError('n-result', '项目名必填');
  runCommand('n-run', 'n-status', 'n-result', 'new_project', { opts }, { dryRun: opts.dry_run });
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
  if (!opts.repo_root) return renderError('a-result', '仓库根目录必填');
  runCommand('a-run', 'a-status', 'a-result', 'adopt', { opts }, { dryRun: opts.dry_run });
});

// ---- manifest ----
$('m-check').addEventListener('click', () => {
  const repo = trimmed('m-repo');
  if (!repo) return renderError('m-result', '仓库根目录必填');
  runCommand('m-check', 'm-status', 'm-result', 'manifest_check', { repo, flavor: $('m-flavor').value });
});

$('m-rebuild').addEventListener('click', () => {
  const repo = trimmed('m-repo');
  if (!repo) return renderError('m-result', '仓库根目录必填');
  runCommand('m-rebuild', 'm-status', 'm-result', 'manifest_rebuild', { repo, flavor: $('m-flavor').value });
});

// ---- gen undo ----
wireRepo('u-repo', 'u-probe');
$('u-run').addEventListener('click', () => {
  const opts = {
    repo_root: trimmed('u-repo'),
    name: trimmed('u-name'),
    dry_run: $('u-dry').checked,
  };
  if (!opts.repo_root) return renderError('u-result', '仓库根目录必填');
  if (!SNAKE_RE.test(opts.name)) return renderError('u-result', `实体名须为 snake_case：${opts.name || '（空）'}`);
  runCommand('u-run', 'u-status', 'u-result', 'gen_undo', { opts }, { dryRun: opts.dry_run });
});

// ---- doctor ----
$('d-run').addEventListener('click', async () => {
  const repo = trimmed('d-repo');
  const button = $('d-run');
  button.disabled = true;
  $('d-status').textContent = '体检中…';
  $('d-status').className = 'status';
  try {
    if (!invoke) throw new Error('未在 Tauri 环境内');
    const report = await invoke('doctor', { repo: repo === '' ? null : repo });
    const mark = { Ok: ['✓', 'ok'], Warn: ['⚠', ''], Fail: ['✗', 'err'] };
    const rows = report.checks
      .map((check) => {
        const [symbol, cls] = mark[check.status] || ['·', ''];
        const hint = check.hint ? `<div class="hint">↳ ${esc(check.hint)}</div>` : '';
        return `<div class="banner ${cls}" style="margin-top: 6px; align-items: flex-start;">
          <span>${symbol}</span>
          <div style="flex: 1;"><strong>${esc(check.name)}</strong> — ${esc(check.detail)}${hint}</div>
        </div>`;
      })
      .join('');
    const failures = report.checks.filter((check) => check.status === 'Fail').length;
    $('d-result').innerHTML =
      `<div class="banner ${failures ? 'err' : 'ok'}">${failures ? `❌ ${failures} 项失败` : '✅ 全部通过'}</div>${rows}`;
    $('d-status').textContent = failures ? `${failures} 项失败` : '全部通过';
    $('d-status').className = `status ${failures ? 'err' : 'ok'}`;
  } catch (error) {
    renderError('d-result', error);
    $('d-status').textContent = '失败';
    $('d-status').className = 'status err';
  } finally {
    button.disabled = false;
  }
});

// ---- 初始化：恢复持久化 ----
for (const id of ['e-repo', 'p-repo', 'm-repo', 'a-repo']) {
  wireRepo(id, id.charAt(0) + '-probe');
}
bindPersist('n-dir', 'new.dir');
showHandoff();
