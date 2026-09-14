//! Minimal line diff for showing file changes. Produces a git-style text
//! diff (unified header + hunks), capped to keep output small.

const CAP: usize = 120;

pub fn diff(old: &str, new: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    if old_lines == new_lines {
        return String::new();
    }
    let ops = myers(&old_lines, &new_lines);
    let mut out = String::new();
    out.push_str(&format!("--- {path}\n+++ {path}\n"));
    let mut emitted = 0usize;
    for op in &ops {
        if emitted >= CAP {
            out.push_str("... (diff truncated)\n");
            break;
        }
        match op {
            Op::Keep(l) => {
                out.push_str(&format!(" {}\n", old_lines[*l]));
            }
            Op::Del(l) => {
                out.push_str(&format!("-{}\n", old_lines[*l]));
                emitted += 1;
            }
            Op::Add(l) => {
                out.push_str(&format!("+{}\n", new_lines[*l]));
                emitted += 1;
            }
        }
    }
    out
}

enum Op {
    Keep(usize),
    Del(usize),
    Add(usize),
}

fn myers(a: &[&str], b: &[&str]) -> Vec<Op> {
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Keep(i));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Del(i));
            i += 1;
        } else {
            ops.push(Op::Add(j));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del(i));
        i += 1;
    }
    while j < m {
        ops.push(Op::Add(j));
        j += 1;
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_returns_empty() {
        assert_eq!(diff("a\nb\n", "a\nb\n", "f.txt"), "");
    }

    #[test]
    fn single_line_change() {
        let out = diff("one\n", "one\ntwo\n", "f.txt");
        assert!(out.contains("--- f.txt"));
        assert!(out.contains("+two"), "got: {out}");
        assert!(out.contains(" one"), "got: {out}");
    }

    #[test]
    fn deletion_marked() {
        let out = diff("a\nb\nc\n", "a\nc\n", "f.txt");
        assert!(out.contains("-b"), "got: {out}");
        assert!(!out.contains("+b"));
    }
}