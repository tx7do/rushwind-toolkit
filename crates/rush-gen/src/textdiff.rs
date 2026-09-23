//! 极简行级 diff（`--regen` 的 dry-run 预览用）：掐掉公共前后缀后，把
//! 中间变更区以 `-`/`+` 呈现，两侧各带少量上下文。刻意不做标准 unified
//! diff 的 hunk 头——预览要的是"改了什么"，不是补丁可用性。

/// 变更超过这个行数时截断展示（预览不是全量打印）。
const MAX_SHOWN: usize = 120;

/// 有变更返回 Some（多行文本），无变更返回 None。
pub fn unified_ish(path: &str, old: &str, new: &str) -> Option<String> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // 公共前缀
    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    // 公共后缀（不与前缀重叠）
    let mut suffix = 0usize;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let removed = &old_lines[prefix..old_lines.len() - suffix];
    let added = &new_lines[prefix..new_lines.len() - suffix];
    if removed.is_empty() && added.is_empty() {
        return None;
    }

    const CTX: usize = 3;
    let mut out = String::new();
    out.push_str(&format!("--- {path}（--regen 将更新）\n"));
    let ctx_before = &old_lines[prefix.saturating_sub(CTX)..prefix];
    for line in ctx_before {
        out.push_str(&format!("  {line}\n"));
    }
    for (label, lines) in [("-", removed), ("+", added)] {
        if lines.is_empty() {
            continue;
        }
        let shown = lines.len().min(MAX_SHOWN);
        for line in &lines[..shown] {
            out.push_str(&format!("{label}{line}\n"));
        }
        if lines.len() > shown {
            out.push_str(&format!(
                "{label} …（其余 {} 行省略）\n",
                lines.len() - shown
            ));
        }
    }
    let ctx_after =
        &new_lines[new_lines.len() - suffix..(new_lines.len() - suffix + CTX).min(new_lines.len())];
    for line in ctx_after {
        out.push_str(&format!("  {line}\n"));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_change_with_context() {
        let old = "a\nb\nc\nd\ne\nf\n";
        let new = "a\nb\nC!\nd\ne\nf\n";
        let diff = unified_ish("x.rs", old, new).unwrap();
        assert!(diff.contains("-c"), "{diff}");
        assert!(diff.contains("+C!"), "{diff}");
        assert!(diff.contains("  b"), "上下文：{diff}");

        assert!(unified_ish("x.rs", old, old).is_none(), "无变更返回 None");
    }

    #[test]
    fn truncates_large_changes() {
        let old = "x\n";
        let new = String::new() + &"line\n".repeat(300);
        let diff = unified_ish("x.rs", old, &new).unwrap();
        assert!(diff.contains("省略"), "{diff}");
    }
}
