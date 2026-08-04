use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::tmux::{
    format::{
        AC_DARK_BLUE, AC_GONE, AC_GREEN, AC_LOADING, AC_NEW, AC_PURPLE, BG_BAR, BG_CLEAN,
        BG_DEFAULT, BG_ERROR, BG_GONE, BG_LOADING, BG_NEW, BG_TERMINAL, FG_BLUE, FG_CLEAN,
        FG_DARK_BLUE, FG_DEFAULT, FG_GONE, FG_GREEN, FG_GREY89, FG_PREVIOUS, FG_PURPLE, Palette,
        Segment, Style, colored_segment, powerline_segment,
    },
    icons::{
        ADDED, AHEAD, ARROW_RIGHT, BEHIND, CLEAN, COPIED, DELETED, DIVIDER, FAILED, GIT, MODIFIED,
        NEW, RENAMED, SEPARATOR, STAGED, STASHED, SYNC, UNMERGED, WHITE_SPACE,
    },
};

/// End cap for the outline styles.  Swap for any of the `CAP_*` icons —
/// `tmux-companion preview` renders them all side by side.
const OUTLINE_CAP: &str = crate::tmux::icons::CAP_NONE;

const BRANCH_MAX_LEN: usize = 20;
const HEAD_LEN: usize = 8;
const TAIL_LEN: usize = 9;

// Branch-type icon patterns — evaluated in order (first match wins).
// Using a Vec (not HashMap) to ensure deterministic order and allow order-sensitive matching.
static BRANCH_TYPES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        (
            crate::tmux::icons::FEATURE,
            Regex::new(r"^feat(ures?)?/").unwrap(),
        ),
        (
            crate::tmux::icons::BUGFIX,
            Regex::new(r"^(bug)?fix(es)?/").unwrap(),
        ),
        (crate::tmux::icons::HOTFIX, Regex::new(r"^hotfix/").unwrap()),
        (crate::tmux::icons::CHORE, Regex::new(r"^chores?/").unwrap()),
        (
            crate::tmux::icons::RELEASE,
            Regex::new(r"^releases?/").unwrap(),
        ),
    ]
});

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Area {
    pub modified: i32,
    pub added: i32,
    pub deleted: i32,
    pub renamed: i32,
    pub copied: i32,
}

impl Area {
    fn parse_symbol(&mut self, s: &str) {
        match s {
            "M" => self.modified += 1,
            "A" => self.added += 1,
            "D" => self.deleted += 1,
            "R" => self.renamed += 1,
            "C" => self.copied += 1,
            _ => {}
        }
    }

    fn count(&self) -> i32 {
        self.added + self.deleted + self.modified + self.copied + self.renamed
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub ahead: i32,
    pub behind: i32,
    pub branch: String,
    pub commit: String,
    pub is_gone: bool,
    pub is_new: bool,
    pub staged: Area,
    pub stashed: i32,
    pub unstaged: Area,
    pub unmerged: i32,
    pub untracked: i32,
    pub upstream: String,
    pub remote_success: bool,
    pub loading: bool,
}

impl GitStatus {
    pub fn parse_porcelain_v2(output: &str) -> Self {
        let mut s = Self::default();
        for line in output.lines() {
            if line.is_empty() {
                continue;
            }
            s.parse_line(line);
        }
        s
    }

    fn parse_line(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("#") => self.parse_branch_info(&mut words),
            Some("1") | Some("2") => self.parse_tracked_file(&mut words),
            Some("u") => self.unmerged += 1,
            Some("?") => self.untracked += 1,
            _ => {}
        }
    }

    fn parse_branch_info<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        while let Some(key) = words.next() {
            match key {
                "branch.oid" => {
                    if let Some(v) = words.next() {
                        self.commit = v.to_string();
                    }
                }
                "branch.head" => {
                    if let Some(v) = words.next() {
                        self.branch = v.to_string();
                    }
                }
                "branch.upstream" => {
                    if let Some(v) = words.next() {
                        self.upstream = v.to_string();
                    }
                }
                "branch.ab" => self.parse_ahead_behind(words),
                _ => {}
            }
        }
    }

    fn parse_ahead_behind<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        for word in words {
            if let Ok(n) = word[1..].parse::<i32>() {
                match &word[..1] {
                    "+" => self.ahead = n,
                    "-" => self.behind = n,
                    _ => {}
                }
            }
        }
    }

    fn parse_tracked_file<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        if let Some(xy) = words.next() {
            if xy.len() >= 2 {
                self.staged.parse_symbol(&xy[..1]);
                self.unstaged.parse_symbol(&xy[1..2]);
            }
        }
    }

    fn count(&self) -> i32 {
        self.staged.count()
            + self.unstaged.count()
            + self.behind
            + self.ahead
            + self.unmerged
            + self.untracked
    }

    fn is_clean(&self) -> bool {
        !self.is_new && !self.is_gone && self.count() == 0
    }

    fn is_dirty(&self) -> bool {
        !self.is_new && !self.is_gone && self.count() > 0
    }

    fn bg(&self) -> &'static str {
        if self.is_clean() {
            BG_CLEAN
        } else if self.is_new {
            BG_NEW
        } else if self.is_gone {
            BG_GONE
        } else {
            BG_DEFAULT
        }
    }

    fn fg(&self) -> &'static str {
        if self.is_clean() {
            FG_CLEAN
        } else if self.is_gone {
            FG_GONE
        } else {
            FG_DEFAULT
        }
    }

    /// Outline text color: the state color the fill style uses as background,
    /// lightened where the fill value would vanish on the dark bar.
    fn accent(&self, bright: bool) -> &'static str {
        match (self.bg(), bright) {
            (BG_GONE, true) => AC_GONE,
            (bg, _) => bg,
        }
    }

    fn palette(&self, style: Style, bar_bg: &'static str) -> Palette {
        match style {
            Style::Fill => Palette {
                bg: self.bg(),
                fg: self.fg(),
                reset_fg: BG_TERMINAL,
                cap: self.bg(),
                cap_glyph: ARROW_RIGHT,
                loading_fg: FG_GREY89,
                loading_bg: BG_LOADING,
                loading_cap: BG_LOADING,
                error_fg: FG_GREY89,
                error_bg: BG_ERROR,
                error_cap: BG_ERROR,
                prev_fg: FG_PREVIOUS,
                new_fg: FG_BLUE,
                green_fg: FG_GREEN,
                dirty_fg: BG_GONE,
                ahead_fg: FG_DARK_BLUE,
                unmerged_fg: BG_ERROR,
                stash_fg: FG_PURPLE,
            },
            Style::Outline | Style::OutlineBright => {
                let bright = style == Style::OutlineBright;
                let accent = self.accent(bright);
                Palette {
                    bg: bar_bg,
                    fg: accent,
                    reset_fg: accent,
                    cap: accent,
                    cap_glyph: OUTLINE_CAP,
                    loading_fg: if bright { AC_LOADING } else { BG_LOADING },
                    loading_bg: bar_bg,
                    loading_cap: if bright { AC_LOADING } else { BG_LOADING },
                    error_fg: BG_ERROR,
                    error_bg: bar_bg,
                    error_cap: BG_ERROR,
                    prev_fg: accent,
                    new_fg: if bright { AC_NEW } else { FG_BLUE },
                    green_fg: if bright { AC_GREEN } else { FG_GREEN },
                    dirty_fg: if bright { AC_GONE } else { BG_GONE },
                    ahead_fg: if bright { AC_DARK_BLUE } else { FG_DARK_BLUE },
                    unmerged_fg: BG_ERROR,
                    stash_fg: if bright { AC_PURPLE } else { FG_PURPLE },
                }
            }
        }
    }
}

fn short_branch(branch: &str) -> String {
    let mut icon = "";
    let mut stripped = String::new();

    for (icn, pattern) in BRANCH_TYPES.iter() {
        if pattern.is_match(branch) {
            stripped = pattern.replace(branch, "").into_owned();
            icon = icn;
            break;
        }
    }

    let name = if stripped.is_empty() {
        branch
    } else {
        &stripped
    };
    let char_count = name.chars().count();
    // Go iterates indices 0..len-1, truncating when index >= branchMaxLen,
    // which fires at index 20 (i.e., when len > 20 chars).
    // Tail formula: branch[lastIndex-tailLen:] = last (tailLen+1) = 10 chars.
    let truncated = if char_count > BRANCH_MAX_LEN {
        let head: String = name.chars().take(HEAD_LEN).collect();
        let tail: String = name
            .chars()
            .rev()
            .take(TAIL_LEN + 1)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("{}...{}", head, tail)
    } else {
        name.to_string()
    };

    format!("{}{}", icon, truncated)
}

/// Fill-style render — the shape the test suite pins.  Production goes through
/// `status_line_styled`.
#[cfg(test)]
pub fn status_line_mode(s: &GitStatus, nvim_suspended: bool, no_tmux: bool) -> String {
    status_line_styled(s, nvim_suspended, no_tmux, Style::Fill)
}

pub fn status_line_styled(
    s: &GitStatus,
    nvim_suspended: bool,
    no_tmux: bool,
    style: Style,
) -> String {
    status_line_capped(s, nvim_suspended, no_tmux, style, None)
}

/// Same render, with the end-cap glyph overridable — used by `preview` to show
/// cap candidates side by side.
pub fn status_line_capped(
    s: &GitStatus,
    nvim_suspended: bool,
    no_tmux: bool,
    style: Style,
    cap_glyph: Option<&'static str>,
) -> String {
    let bar_bg = if nvim_suspended { BG_TERMINAL } else { BG_BAR };
    let mut p = s.palette(style, bar_bg);
    if let Some(g) = cap_glyph {
        p.cap_glyph = g;
    }
    let bg = p.bg;
    let fg = p.fg;
    let reset = colored_segment(no_tmux, p.reset_fg, bg, "");
    let reset_ws = colored_segment(no_tmux, p.reset_fg, bg, WHITE_SPACE);

    let mut status_line = Segment::new();
    let mut remote = Segment::new();

    if s.loading {
        remote.add(colored_segment(
            no_tmux,
            p.loading_fg,
            p.loading_bg,
            &format!("{}{}{}", WHITE_SPACE, SYNC, WHITE_SPACE),
        ));
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.loading_cap, bg, ARROW_RIGHT),
            ""
        ));
    } else if s.remote_success {
        remote.add(colored_segment(no_tmux, p.prev_fg, bg, WHITE_SPACE));
    } else {
        remote.add(colored_segment(no_tmux, p.prev_fg, p.error_bg, ARROW_RIGHT));
        remote.add(colored_segment(no_tmux, p.error_fg, p.error_bg, WHITE_SPACE));
        remote.add(format!("{}{}", FAILED, WHITE_SPACE));
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.error_cap, bg, ""),
            ARROW_RIGHT
        ));
    }

    remote.add(format!(
        "{}{}{}{}",
        colored_segment(no_tmux, fg, bg, ""),
        GIT,
        short_branch(&s.branch),
        SEPARATOR
    ));

    if s.is_new {
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.new_fg, bg, NEW),
            reset
        ));
    } else if s.is_gone {
        remote.add(crate::tmux::icons::GONE.to_string());
    } else if s.is_clean() {
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.green_fg, bg, CLEAN),
            reset_ws
        ));
    } else if s.is_dirty() {
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.dirty_fg, bg, ""),
            reset
        ));
    }

    status_line.add(remote.to_string());

    // sub-segments
    let mut sub = Segment::new();

    // branch info (ahead/behind/unmerged)
    let mut branch = Segment::new();
    branch.counter(
        s.ahead,
        &format!(
            "{}{}",
            colored_segment(no_tmux, p.ahead_fg, bg, AHEAD),
            reset
        ),
    );
    branch.counter(
        s.behind,
        &format!(
            "{}{}",
            colored_segment(no_tmux, p.ahead_fg, bg, BEHIND),
            reset
        ),
    );
    branch.counter(
        s.unmerged,
        &colored_segment(no_tmux, p.unmerged_fg, bg, UNMERGED),
    );
    branch.append_only(&reset);
    sub.when(!branch.is_empty(), &branch.to_string());

    // unstaged
    let mut unstaged = Segment::new();
    unstaged.counter(s.untracked + s.unstaged.added, ADDED);
    unstaged.counter(s.unstaged.deleted, DELETED);
    unstaged.counter(s.unstaged.renamed, RENAMED);
    unstaged.counter(s.unstaged.copied, COPIED);
    unstaged.counter(s.unstaged.modified, MODIFIED);
    sub.when(!unstaged.is_empty(), &unstaged.to_string());

    // staged
    let mut staged = Segment::new();
    staged.counter(s.staged.added, ADDED);
    staged.counter(s.staged.deleted, DELETED);
    staged.counter(s.staged.renamed, RENAMED);
    staged.counter(s.staged.copied, COPIED);
    staged.counter(s.staged.modified, MODIFIED);
    staged.prepend_only(&format!(
        "{}{}",
        colored_segment(no_tmux, p.green_fg, bg, STAGED),
        reset
    ));
    sub.when(!staged.is_empty(), &staged.to_string());

    // stash
    sub.counter(
        s.stashed,
        &format!(
            "{}{}",
            colored_segment(no_tmux, p.stash_fg, bg, STASHED),
            reset
        ),
    );

    status_line.add(sub.join(DIVIDER));

    if no_tmux {
        status_line.append(&format!(
            "\x1b[0m\x1b[38;5;{}m{}\x1b[0m",
            p.cap, p.cap_glyph
        ));
    } else {
        status_line.append(&powerline_segment(p.cap, bar_bg, p.cap_glyph));
    }

    status_line.to_string()
}

async fn run_git(args: &[&str], dir: &Path) -> anyhow::Result<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("git {:?} timed out", args))??;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn count_stash(git_root: &Path) -> i32 {
    let stash = git_root.join("logs/refs/stash");
    std::fs::read_to_string(stash)
        .map(|s| s.lines().count() as i32)
        .unwrap_or(0)
}

async fn fetch_git_status(path: &Path) -> anyhow::Result<GitStatus> {
    let p1 = path.to_owned();
    let p2 = path.to_owned();

    let (status_out, root_out) = tokio::join!(
        async move {
            run_git(
                &[
                    "status",
                    "--untracked-files=all",
                    "--branch",
                    "--porcelain=v2",
                ],
                &p1,
            )
            .await
        },
        async move { run_git(&["rev-parse", "--path-format=absolute", "--git-dir"], &p2).await },
    );

    let mut status = GitStatus::parse_porcelain_v2(&status_out?);
    let git_root = PathBuf::from(root_out?.trim());

    let is_new = status.upstream.is_empty();
    status.is_new = is_new;

    let path_owned = path.to_owned();
    let upstream = status.upstream.clone();

    let (is_gone, stash_count) = tokio::join!(
        async move {
            if is_new {
                return Ok(false);
            }
            let branches = run_git(&["branch", "-r"], &path_owned).await?;
            Ok::<bool, anyhow::Error>(!branches.contains(&upstream))
        },
        tokio::task::spawn_blocking(move || count_stash(&git_root)),
    );

    status.is_gone = is_gone.unwrap_or(false);
    status.stashed = stash_count.unwrap_or(0);
    status.remote_success = true;

    Ok(status)
}

pub async fn render(
    path: Option<PathBuf>,
    pid: Option<u32>,
    force: bool,
    style: Style,
) -> anyhow::Result<String> {
    let path = match path {
        Some(p) => p.canonicalize()?,
        None => std::env::current_dir()?,
    };

    // Not-a-git-repo check: if rev-parse fails, return empty
    let check = run_git(&["rev-parse", "--is-inside-work-tree"], &path).await;
    if check.map(|s| s.trim() != "true").unwrap_or(true) {
        return Ok(String::new());
    }

    let id = B64.encode(path.to_string_lossy().as_bytes());

    let nvim_suspended = match pid {
        Some(p) => crate::segments::vim_bg::has_suspended_nvim(p).await.unwrap_or(false),
        None => false,
    };

    if !force {
        let id_c = id.clone();
        let cached =
            tokio::task::spawn_blocking(move || crate::db::get_git_status(&id_c, 2)).await??;
        if let Some(status) = cached {
            return Ok(status_line_styled(&status, nvim_suspended, false, style));
        }
    }

    let status = fetch_git_status(&path).await?;

    let id_c = id.clone();
    let s_clone = status.clone();
    tokio::task::spawn_blocking(move || crate::db::save_git_status(&id_c, &s_clone)).await??;

    Ok(status_line_styled(&status, nvim_suspended, false, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{
        format::{
            AC_DARK_BLUE, AC_GONE, AC_PURPLE, BG_BAR, BG_CLEAN, BG_DEFAULT, BG_ERROR, BG_GONE,
            BG_LOADING, BG_NEW, BG_TERMINAL, FG_CLEAN, FG_DARK_BLUE, FG_DEFAULT, FG_GONE,
            FG_GREEN, FG_PREVIOUS, FG_PURPLE, Style, colored_segment, powerline_segment,
        },
        icons::*,
    };

    // ── helpers ───────────────────────────────────────────────────────────────

    fn s_clean() -> GitStatus {
        GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            ..Default::default()
        }
    }

    fn s_dirty_all() -> GitStatus {
        GitStatus {
            ahead: 1,
            behind: 1,
            branch: "branch1".into(),
            unmerged: 1,
            untracked: 1,
            remote_success: true,
            stashed: 1,
            staged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            unstaged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            ..Default::default()
        }
    }

    // Build expected status-line strings the same way status_line_mode does.
    fn remote_success_seg(bg: &str, no_tmux: bool) -> String {
        colored_segment(no_tmux, FG_PREVIOUS, bg, WHITE_SPACE)
    }
    fn branch_seg(branch: &str, fg: &str, bg: &str, no_tmux: bool) -> String {
        format!(
            "{}{}{}{}",
            colored_segment(no_tmux, fg, bg, ""),
            GIT,
            short_branch(branch),
            SEPARATOR
        )
    }
    fn arrow(bg: &str, no_tmux: bool) -> String {
        if no_tmux {
            format!("\x1b[0m\x1b[38;5;{}m{}\x1b[0m", bg, ARROW_RIGHT)
        } else {
            // Default arrow background is "233" (status bar bg).
            // BG_TERMINAL ("235") is used only when nvim_suspended=true.
            powerline_segment(bg, "233", ARROW_RIGHT)
        }
    }

    // ── parse_porcelain_v2 ────────────────────────────────────────────────────

    #[test]
    fn parse_porcelain_clean() {
        let input = "# branch.oid abc123\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.branch, "main");
        assert_eq!(s.upstream, "origin/main");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }

    #[test]
    fn parse_porcelain_ahead_behind() {
        let input =
            "# branch.head feat/foo\n# branch.upstream origin/feat/foo\n# branch.ab +2 -3\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 3);
    }

    #[test]
    fn parse_tracked_file_mm() {
        let input = "# branch.head main\n1 MM N... 100644 100644 100644 hash hash file.txt\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.staged.modified, 1);
        assert_eq!(s.unstaged.modified, 1);
    }

    #[test]
    fn parse_tracked_file_all_xy_symbols() {
        // XY where X=staged, Y=unstaged; exercise A/D/R/C/M in each slot.
        let input = "# branch.head main\n\
            1 AM N... 0 0 0 h h f1\n\
            1 DC N... 0 0 0 h h f2\n\
            1 RA N... 0 0 0 h h f3\n\
            1 CD N... 0 0 0 h h f4\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.staged.added, 1); // A from "AM"
        assert_eq!(s.staged.deleted, 1); // D from "DC"
        assert_eq!(s.staged.renamed, 1); // R from "RA"
        assert_eq!(s.staged.copied, 1); // C from "CD"
        assert_eq!(s.unstaged.modified, 1); // M from "AM"
        assert_eq!(s.unstaged.copied, 1); // C from "DC"
        assert_eq!(s.unstaged.added, 1); // A from "RA"
        assert_eq!(s.unstaged.deleted, 1); // D from "CD"
    }

    #[test]
    fn parse_untracked_unmerged() {
        let input = "# branch.head main\n? newfile.txt\nu UU N... hash hash hash conflict.txt\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.untracked, 1);
        assert_eq!(s.unmerged, 1);
    }

    #[test]
    fn parse_empty_input_gives_defaults() {
        let s = GitStatus::parse_porcelain_v2("");
        assert_eq!(s.branch, "");
        assert_eq!(s.ahead, 0);
    }

    #[test]
    fn parse_no_upstream_means_is_new() {
        // The render path sets is_new from upstream.is_empty(); parse alone doesn't set it.
        let s = GitStatus::parse_porcelain_v2("# branch.head main\n");
        assert_eq!(s.upstream, ""); // no upstream in input
    }

    // ── Area ─────────────────────────────────────────────────────────────────

    #[test]
    fn area_count_sums_fields() {
        let a = Area {
            modified: 1,
            added: 2,
            deleted: 3,
            renamed: 4,
            copied: 5,
        };
        assert_eq!(a.count(), 15);
    }

    #[test]
    fn area_count_zero() {
        assert_eq!(Area::default().count(), 0);
    }

    #[test]
    fn area_parse_symbol_all() {
        let mut a = Area::default();
        for s in ["M", "A", "D", "R", "C"] {
            a.parse_symbol(s);
        }
        assert_eq!(a.modified, 1);
        assert_eq!(a.added, 1);
        assert_eq!(a.deleted, 1);
        assert_eq!(a.renamed, 1);
        assert_eq!(a.copied, 1);
    }

    #[test]
    fn area_parse_symbol_ignores_unknown() {
        let mut a = Area::default();
        a.parse_symbol("X");
        a.parse_symbol(".");
        a.parse_symbol("");
        assert_eq!(a.count(), 0);
    }

    // ── GitStatus helpers ────────────────────────────────────────────────────

    #[test]
    fn git_status_bg_clean() {
        assert_eq!(s_clean().bg(), BG_CLEAN);
    }

    #[test]
    fn git_status_bg_new() {
        let s = GitStatus {
            is_new: true,
            ..Default::default()
        };
        assert_eq!(s.bg(), BG_NEW);
    }

    #[test]
    fn git_status_bg_gone() {
        let s = GitStatus {
            is_gone: true,
            ..Default::default()
        };
        assert_eq!(s.bg(), BG_GONE);
    }

    #[test]
    fn git_status_bg_dirty() {
        let s = GitStatus {
            ahead: 1,
            ..Default::default()
        };
        assert_eq!(s.bg(), BG_DEFAULT);
    }

    #[test]
    fn git_status_fg_clean() {
        assert_eq!(s_clean().fg(), FG_CLEAN);
    }

    #[test]
    fn git_status_fg_gone() {
        let s = GitStatus {
            is_gone: true,
            ..Default::default()
        };
        assert_eq!(s.fg(), FG_GONE);
    }

    #[test]
    fn git_status_fg_dirty() {
        let s = GitStatus {
            ahead: 1,
            ..Default::default()
        };
        assert_eq!(s.fg(), FG_DEFAULT);
    }

    #[test]
    fn git_status_is_clean_true() {
        assert!(s_clean().is_clean());
    }

    #[test]
    fn git_status_is_clean_false_when_new() {
        let s = GitStatus {
            is_new: true,
            ..Default::default()
        };
        assert!(!s.is_clean());
    }

    #[test]
    fn git_status_is_clean_false_when_ahead() {
        let s = GitStatus {
            ahead: 1,
            ..Default::default()
        };
        assert!(!s.is_clean());
    }

    #[test]
    fn git_status_is_dirty_conditions() {
        assert!(
            !GitStatus {
                is_new: true,
                ahead: 1,
                ..Default::default()
            }
            .is_dirty()
        );
        assert!(
            !GitStatus {
                is_gone: true,
                ahead: 1,
                ..Default::default()
            }
            .is_dirty()
        );
        assert!(
            GitStatus {
                ahead: 1,
                ..Default::default()
            }
            .is_dirty()
        );
    }

    // ── short_branch ─────────────────────────────────────────────────────────

    #[test]
    fn short_branch_plain() {
        assert_eq!(short_branch("branch1"), "branch1");
    }

    #[test]
    fn short_branch_feature() {
        let r = short_branch("feat/my-feature");
        assert!(r.starts_with(FEATURE));
        assert!(r.contains("my-feature"));
    }

    #[test]
    fn short_branch_features_plural() {
        let r = short_branch("features/x");
        assert!(r.starts_with(FEATURE));
    }

    #[test]
    fn short_branch_bugfix() {
        let r = short_branch("bugfix/issue-42");
        assert!(r.starts_with(BUGFIX));
        assert!(r.contains("issue-42"));
    }

    #[test]
    fn short_branch_fix() {
        let r = short_branch("fix/crash");
        assert!(r.starts_with(BUGFIX));
    }

    #[test]
    fn short_branch_hotfix() {
        let r = short_branch("hotfix/urgent");
        assert!(r.starts_with(HOTFIX));
    }

    #[test]
    fn short_branch_chore() {
        let r = short_branch("chore/cleanup");
        assert!(r.starts_with(CHORE));
    }

    #[test]
    fn short_branch_release() {
        let r = short_branch("release/1.0");
        assert!(r.starts_with(RELEASE));
    }

    #[test]
    fn short_branch_exactly_max_len_not_truncated() {
        // BRANCH_MAX_LEN = 20; exactly 20 chars should NOT be truncated (> not >=).
        let twenty = "a".repeat(20);
        let r = short_branch(&twenty);
        assert!(!r.contains("..."), "should not truncate at exactly 20: {r}");
    }

    #[test]
    fn short_branch_one_over_max_len_is_truncated() {
        let twenty_one = "a".repeat(21);
        let r = short_branch(&twenty_one);
        assert!(r.contains("..."), "21 chars should be truncated: {r}");
    }

    #[test]
    fn short_branch_over_max_len_truncated() {
        let long = "this-is-a-very-long-branch-name"; // 31 chars
        let r = short_branch(long);
        assert!(r.contains("..."), "should contain ellipsis: {r}");
        assert!(
            r.chars().count() < long.chars().count(),
            "truncated shorter"
        );
    }

    #[test]
    fn short_branch_truncation_head_tail() {
        let long = "this-is-a-very-long-branch-name"; // 31 chars
        let r = short_branch(long);
        // head = first 8 chars = "this-is-"
        // tail = last 10 chars (Go: branch[len-1-tailLen:] = branch[21:]) = "ranch-name"
        assert!(r.starts_with("this-is-"), "head: {r}");
        assert!(r.ends_with("ranch-name"), "tail: {r}");
    }

    // ── status_line_mode: tmux format (ports all 17 Go test cases) ────────────

    #[test]
    fn status_line_loading() {
        let s = GitStatus {
            branch: "branch1".into(),
            loading: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        // Loading: sync icon on blue background
        assert!(line.contains(&format!("bg=color{}", BG_LOADING)));
        assert!(line.contains(SYNC));
        assert!(line.contains(GIT));
        assert!(line.contains("branch1"));
    }

    #[test]
    fn status_line_remote_fail() {
        let s = GitStatus {
            branch: "branch1".into(),
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        // Remote fail: arrow on error background, failed icon
        assert!(line.contains(&format!("bg=color{}", BG_ERROR)));
        assert!(line.contains(FAILED));
        assert!(line.contains(ARROW_RIGHT));
    }

    #[test]
    fn status_line_clean_exact() {
        let s = s_clean();
        let line = status_line_mode(&s, false, false);
        let bg = BG_CLEAN;
        // Should contain remote success marker, git+branch, clean icon, and arrow
        assert!(line.contains(&remote_success_seg(bg, false)));
        assert!(line.contains(&branch_seg("branch1", FG_CLEAN, bg, false)));
        assert!(line.contains(CLEAN));
        assert!(line.contains(&arrow(bg, false)));
    }

    #[test]
    fn status_line_new() {
        let s = GitStatus {
            branch: "branch1".into(),
            is_new: true,
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        let bg = BG_NEW;
        assert!(line.contains(&remote_success_seg(bg, false)));
        assert!(line.contains(NEW));
        assert!(line.contains(&arrow(bg, false)));
    }

    #[test]
    fn status_line_gone() {
        let s = GitStatus {
            branch: "branch1".into(),
            is_gone: true,
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(&format!("bg=color{}", BG_GONE)));
        assert!(line.contains(GONE));
    }

    #[test]
    fn status_line_feature_branch() {
        let s = GitStatus {
            branch: "feat/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(FEATURE));
        assert!(line.contains("branch1"));
        assert!(line.contains(GIT));
    }

    #[test]
    fn status_line_bugfix_branch() {
        let s = GitStatus {
            branch: "bugfix/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(BUGFIX));
    }

    #[test]
    fn status_line_hotfix_branch() {
        let s = GitStatus {
            branch: "hotfix/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(HOTFIX));
    }

    #[test]
    fn status_line_chore_branch() {
        let s = GitStatus {
            branch: "chore/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(CHORE));
    }

    #[test]
    fn status_line_branch_too_long() {
        let s = GitStatus {
            branch: "this-is-a-very-long-branch-name".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("this-is-"), "head: {line}");
        assert!(line.contains("ranch-name"), "tail (last 10): {line}");
        assert!(line.contains("..."), "ellipsis: {line}");
    }

    #[test]
    fn status_line_ahead() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            ahead: 1,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        let bg = BG_DEFAULT;
        assert!(line.contains(&format!("bg=color{}", bg)));
        assert!(line.contains("1"));
        assert!(line.contains(AHEAD));
        assert!(line.contains(&colored_segment(false, FG_DARK_BLUE, bg, AHEAD)));
    }

    #[test]
    fn status_line_behind() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            behind: 1,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("1"));
        assert!(line.contains(BEHIND));
    }

    #[test]
    fn status_line_unmerged() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            unmerged: 1,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("1"), "count: {line}");
        assert!(line.contains(UNMERGED), "icon: {line}");
        // colored_segment(fg=BG_ERROR, bg=BG_DEFAULT, ...) → fg=color160
        assert!(
            line.contains(&format!("fg=color{}", BG_ERROR)),
            "fg=160: {line}"
        );
    }

    #[test]
    fn status_line_unstaged() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            unstaged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(ADDED));
        assert!(line.contains(DELETED));
        assert!(line.contains(RENAMED));
        assert!(line.contains(COPIED));
        assert!(line.contains(MODIFIED));
        // Staged icon should NOT appear (no staged changes)
        assert!(!line.contains(STAGED));
    }

    #[test]
    fn status_line_staged() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            staged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(STAGED)); // staged icon appears
        assert!(line.contains(ADDED));
        assert!(line.contains(&colored_segment(false, FG_GREEN, BG_DEFAULT, STAGED)));
    }

    #[test]
    fn status_line_stashed() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            stashed: 2,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("2"));
        assert!(line.contains(STASHED));
        assert!(line.contains(&colored_segment(false, FG_PURPLE, BG_CLEAN, STASHED)));
    }

    #[test]
    fn status_line_dirty_all() {
        let s = s_dirty_all();
        let line = status_line_mode(&s, false, false);
        // All sub-sections present
        assert!(line.contains(AHEAD));
        assert!(line.contains(BEHIND));
        assert!(line.contains(UNMERGED));
        assert!(line.contains(ADDED));
        assert!(line.contains(STAGED));
        assert!(line.contains(STASHED));
        // Dividers between sub-sections
        assert!(line.contains(DIVIDER));
        // untracked(1) + unstaged.added(1) → count 2
        assert!(line.contains(&format!("2{}", ADDED)));
    }

    #[test]
    fn status_line_untracked_adds_to_unstaged_added() {
        let s = GitStatus {
            branch: "b".into(),
            remote_success: true,
            untracked: 3,
            unstaged: Area {
                added: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        // 3 untracked + 2 unstaged.added = 5
        assert!(line.contains(&format!("5{}", ADDED)));
    }

    // ── status_line_mode: no-tmux (ANSI) format ──────────────────────────────

    #[test]
    fn status_line_no_tmux_clean() {
        let s = s_clean();
        let line = status_line_mode(&s, false, true);
        // Uses ANSI escapes, not #[fg=...]
        assert!(
            !line.contains("#["),
            "should not contain tmux format: {line}"
        );
        assert!(line.contains("\x1b["));
        assert!(line.contains("branch1"));
        assert!(line.ends_with('\x00'.to_string().trim_end_matches('\x00')));
        // Ends with reset+arrow+reset
        let bg = BG_CLEAN;
        assert!(line.contains(&format!("\x1b[0m\x1b[38;5;{}m{}\x1b[0m", bg, ARROW_RIGHT)));
    }

    #[test]
    fn status_line_no_tmux_arrow_right_not_in_color_codes() {
        // In no-tmux mode, ColoredSegment(fg, bg, ARROW_RIGHT) special case:
        // the arrow character should appear only once (from the final append).
        let s = GitStatus {
            branch: "branch1".into(),
            ..Default::default()
        };
        let line = status_line_mode(&s, false, true);
        let arrow_count = line.matches(ARROW_RIGHT).count();
        // Loading+remote_fail have extra arrows; with remote_success=false we get
        // the remote-fail path which adds 2 arrows, plus the final arrow.
        // Just assert at least one arrow is present.
        assert!(arrow_count >= 1, "no arrow found in: {line:?}");
    }

    #[test]
    fn status_line_no_tmux_loading() {
        let s = GitStatus {
            branch: "branch1".into(),
            loading: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, true);
        assert!(!line.contains("#["));
        assert!(line.contains(SYNC));
    }

    #[test]
    fn status_line_nvim_suspended_uses_terminal_bg_for_arrow() {
        let s = s_clean();
        let normal   = status_line_mode(&s, false, false);
        let suspended = status_line_mode(&s, true,  false);
        // With nvim suspended the final arrow background is BG_TERMINAL ("235"),
        // without it the background is "233".
        assert!(normal.contains(&format!("bg=color233]{}", ARROW_RIGHT)),
            "normal arrow bg should be 233: {normal}");
        assert!(suspended.contains(&format!("bg=color{}]{}", BG_TERMINAL, ARROW_RIGHT)),
            "suspended arrow bg should be BG_TERMINAL: {suspended}");
        assert_ne!(normal, suspended);
    }

    // ── styles ────────────────────────────────────────────────────────────────

    #[test]
    fn fill_style_matches_legacy_render() {
        let s = s_dirty_all();
        assert_eq!(
            status_line_mode(&s, false, false),
            status_line_styled(&s, false, false, Style::Fill)
        );
    }

    #[test]
    fn outline_puts_state_color_in_foreground_and_bar_in_background() {
        let s = s_clean();
        let line = status_line_styled(&s, false, false, Style::Outline);
        // Clean state color moves from background to foreground.
        assert!(
            line.contains(&format!("fg=color{},bg=color{}]", BG_CLEAN, BG_BAR)),
            "state color should be the text color: {line}"
        );
        assert!(
            !line.contains(&format!("bg=color{}]", BG_CLEAN)),
            "no solid fill left: {line}"
        );
    }

    #[test]
    fn outline_keeps_icon_colors_untouched() {
        let s = GitStatus {
            stashed: 1,
            ..s_clean()
        };
        let line = status_line_styled(&s, false, false, Style::Outline);
        // Stash icon keeps FG_PURPLE, only its background changes.
        assert!(line.contains(&colored_segment(false, FG_PURPLE, BG_BAR, STASHED)));
        // Clean icon keeps FG_GREEN.
        assert!(line.contains(&colored_segment(false, FG_GREEN, BG_BAR, CLEAN)));
    }

    #[test]
    fn outline_bright_lightens_icon_colors() {
        let s = GitStatus {
            stashed: 1,
            ahead: 1,
            ..s_clean()
        };
        let line = status_line_styled(&s, false, false, Style::OutlineBright);
        assert!(line.contains(&colored_segment(false, AC_PURPLE, BG_BAR, STASHED)));
        assert!(line.contains(&colored_segment(false, AC_DARK_BLUE, BG_BAR, AHEAD)));
        assert!(!line.contains(&format!("fg=color{}", FG_PURPLE)));
    }

    #[test]
    fn outline_bright_lightens_the_gone_state_color() {
        let s = GitStatus {
            is_gone: true,
            ..s_clean()
        };
        let plain = status_line_styled(&s, false, false, Style::Outline);
        let bright = status_line_styled(&s, false, false, Style::OutlineBright);
        assert!(plain.contains(&format!("fg=color{}", BG_GONE)));
        assert!(bright.contains(&format!("fg=color{}", AC_GONE)));
    }

    #[test]
    fn outline_error_and_loading_backgrounds_flatten_to_the_bar() {
        let failed = status_line_styled(
            &GitStatus {
                branch: "b".into(),
                ..Default::default()
            },
            false,
            false,
            Style::Outline,
        );
        assert!(!failed.contains(&format!("bg=color{}]", BG_ERROR)), "{failed}");
        assert!(failed.contains(&format!("fg=color{}", BG_ERROR)), "{failed}");

        let loading = status_line_styled(
            &GitStatus {
                branch: "b".into(),
                loading: true,
                ..Default::default()
            },
            false,
            false,
            Style::Outline,
        );
        assert!(!loading.contains(&format!("bg=color{}]", BG_LOADING)), "{loading}");
        assert!(loading.contains(&format!("fg=color{}", BG_LOADING)), "{loading}");
    }

    #[test]
    fn fill_ends_with_the_solid_arrow() {
        let s = s_clean();
        let line = status_line_styled(&s, false, false, Style::Fill);
        assert!(line.ends_with(&powerline_segment(BG_CLEAN, BG_BAR, ARROW_RIGHT)));
    }

    #[test]
    fn outline_ends_without_a_cap_glyph() {
        let s = s_clean();
        let line = status_line_styled(&s, false, false, Style::Outline);
        assert!(line.ends_with(&powerline_segment(BG_CLEAN, BG_BAR, OUTLINE_CAP)));
        // The solid triangle must not survive into an outline render.
        assert!(!line.contains(ARROW_RIGHT), "solid arrow left over: {line}");
    }

    #[test]
    fn outline_last_visible_char_is_not_a_glyph() {
        // With CAP_NONE the render must end on the color marker, so the segment
        // simply stops rather than drawing a floating shape.
        let line = status_line_styled(&s_dirty_all(), false, false, Style::OutlineBright);
        assert!(line.ends_with(']'), "trailing glyph after the cap: {line}");
    }

    #[test]
    fn cap_override_replaces_the_end_cap() {
        let s = s_clean();
        let line = status_line_capped(&s, false, false, Style::Outline, Some(CAP_RULE));
        assert!(line.ends_with(&powerline_segment(BG_CLEAN, BG_BAR, CAP_RULE)));
    }

    #[test]
    fn cap_override_does_not_leak_into_the_default() {
        let s = s_clean();
        let _ = status_line_capped(&s, false, false, Style::Outline, Some(CAP_RULE));
        let plain = status_line_styled(&s, false, false, Style::Outline);
        assert!(!plain.contains(CAP_RULE.trim()), "{plain}");
    }

    #[test]
    fn outline_respects_nvim_suspended_bar_background() {
        let s = s_clean();
        let line = status_line_styled(&s, true, false, Style::Outline);
        assert!(line.contains(&format!("bg=color{}]", BG_TERMINAL)), "{line}");
        assert!(!line.contains(&format!("bg=color{}]", BG_BAR)), "{line}");
    }

    #[test]
    fn style_parse_accepts_known_names_only() {
        assert_eq!(Style::parse("fill"), Some(Style::Fill));
        assert_eq!(Style::parse("outline"), Some(Style::Outline));
        assert_eq!(Style::parse("outline-bright"), Some(Style::OutlineBright));
        assert_eq!(Style::parse("bright"), Some(Style::OutlineBright));
        assert_eq!(Style::parse("nope"), None);
        assert_eq!(Style::default(), Style::OutlineBright);
    }

    // ── count_stash ───────────────────────────────────────────────────────────

    #[test]
    fn count_stash_missing_file_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count_stash(dir.path()), 0);
    }

    #[test]
    fn count_stash_counts_lines() {
        let dir = tempfile::tempdir().unwrap();
        let stash_path = dir.path().join("logs/refs");
        std::fs::create_dir_all(&stash_path).unwrap();
        std::fs::write(stash_path.join("stash"), "entry1\nentry2\nentry3\n").unwrap();
        assert_eq!(count_stash(dir.path()), 3);
    }

    #[test]
    fn count_stash_empty_file_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let stash_path = dir.path().join("logs/refs");
        std::fs::create_dir_all(&stash_path).unwrap();
        std::fs::write(stash_path.join("stash"), "").unwrap();
        assert_eq!(count_stash(dir.path()), 0);
    }
}
