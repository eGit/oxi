//! Line-based three-way merge, so a tab with unsaved edits can take in what another program
//! (usually an agent) wrote to the same file instead of refusing to save.

/// Merge `ours` and `theirs`, both edited from `base`. `None` when both changed the same lines
/// differently, unless `union` asks to keep both sides (ours first), as the scratchpad does:
/// notes lose nothing that way, while code would end up with both versions of a line.
pub(crate) fn merge(base: &str, ours: &str, theirs: &str, union: bool) -> Option<String> {
    if ours == theirs || theirs == base {
        return Some(ours.to_string());
    }
    if ours == base {
        return Some(theirs.to_string());
    }
    let base_lines: Vec<&str> = base.split_inclusive('\n').collect();
    let ours_lines: Vec<&str> = ours.split_inclusive('\n').collect();
    let theirs_lines: Vec<&str> = theirs.split_inclusive('\n').collect();
    let to_ours = matching(&base_lines, &ours_lines)?;
    let to_theirs = matching(&base_lines, &theirs_lines)?;

    let mut out = String::with_capacity(ours.len().max(theirs.len()));
    let (mut b, mut o, mut t) = (0, 0, 0);
    loop {
        // The next base line both sides kept is where this chunk ends.
        let stable =
            (b..base_lines.len()).find(|&i| to_ours[i].is_some() && to_theirs[i].is_some());
        let (b_end, o_end, t_end) = match stable {
            Some(i) => (i, to_ours[i]?, to_theirs[i]?),
            None => (base_lines.len(), ours_lines.len(), theirs_lines.len()),
        };
        let chunk_base = &base_lines[b..b_end];
        let chunk_ours = &ours_lines[o..o_end];
        let chunk_theirs = &theirs_lines[t..t_end];
        if chunk_ours == chunk_base {
            out.extend(chunk_theirs.iter().copied());
        } else if chunk_theirs == chunk_base || chunk_ours == chunk_theirs {
            out.extend(chunk_ours.iter().copied());
        } else if union {
            out.extend(chunk_ours.iter().copied());
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.extend(chunk_theirs.iter().copied());
        } else {
            return None;
        }
        let Some(i) = stable else { break };
        out.push_str(base_lines[i]);
        (b, o, t) = (i + 1, o_end + 1, t_end + 1);
    }
    Some(out)
}

/// For each line of `a`, the line of `b` it is kept as in a longest common subsequence.
/// `None` when the differing middle is too big to diff in reasonable time.
fn matching(a: &[&str], b: &[&str]) -> Option<Vec<Option<usize>>> {
    const MAX_CELLS: usize = 4_000_000;
    let mut map = vec![None; a.len()];
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    for (i, slot) in map.iter_mut().enumerate().take(prefix) {
        *slot = Some(i);
    }
    for k in 0..suffix {
        map[a.len() - 1 - k] = Some(b.len() - 1 - k);
    }
    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];
    let (m, n) = (a_mid.len(), b_mid.len());
    if m == 0 || n == 0 {
        return Some(map);
    }
    if m.saturating_mul(n) > MAX_CELLS {
        return None;
    }
    // dp[i][j]: LCS length of a_mid[i..] and b_mid[j..].
    let width = n + 1;
    let mut dp = vec![0u32; (m + 1) * width];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i * width + j] = if a_mid[i] == b_mid[j] {
                dp[(i + 1) * width + j + 1] + 1
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < m && j < n {
        if a_mid[i] == b_mid[j] {
            map[prefix + i] = Some(prefix + j);
            i += 1;
            j += 1;
        } else if dp[(i + 1) * width + j] >= dp[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::merge;

    #[test]
    fn takes_in_edits_to_other_lines() {
        let base = "a\nb\nc\nd\n";
        let ours = "a\nB mine\nc\nd\n";
        let theirs = "a\nb\nc\nD theirs\ne\n";
        assert_eq!(
            merge(base, ours, theirs, false).as_deref(),
            Some("a\nB mine\nc\nD theirs\ne\n")
        );
    }

    #[test]
    fn trivial_sides() {
        assert_eq!(merge("x", "x", "y", false).as_deref(), Some("y"));
        assert_eq!(merge("x", "y", "x", false).as_deref(), Some("y"));
        assert_eq!(merge("x", "y", "y", false).as_deref(), Some("y"));
    }

    #[test]
    fn same_line_conflicts_unless_union() {
        assert_eq!(merge("a\nb\n", "a\nmine\n", "a\ntheirs\n", false), None);
        assert_eq!(
            merge("a\nb\n", "a\nmine\n", "a\ntheirs\n", true).as_deref(),
            Some("a\nmine\ntheirs\n")
        );
    }

    #[test]
    fn both_appending_keeps_both_in_union() {
        assert_eq!(
            merge("notes\n", "notes\ntyping", "notes\nagent line\n", true).as_deref(),
            Some("notes\ntyping\nagent line\n")
        );
        assert_eq!(
            merge("notes\n", "notes\ntyping", "notes\nagent\n", false),
            None
        );
    }

    #[test]
    fn deletions_and_insertions_interleave() {
        let base = "1\n2\n3\n4\n5\n";
        let ours = "1\n3\n4\n5\n";
        let theirs = "1\n2\n3\n4\n4.5\n5\n";
        assert_eq!(
            merge(base, ours, theirs, false).as_deref(),
            Some("1\n3\n4\n4.5\n5\n")
        );
        // Identical changes on both sides are not a conflict.
        assert_eq!(
            merge("a\nb\nc\n", "a\nX\nc\nmine\n", "a\nX\nc\n", false).as_deref(),
            Some("a\nX\nc\nmine\n")
        );
    }

    #[test]
    fn empty_base() {
        assert_eq!(merge("", "a\n", "b\n", false), None);
        assert_eq!(merge("", "a\n", "b\n", true).as_deref(), Some("a\nb\n"));
    }
}
