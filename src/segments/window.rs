use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

// Unselected/selected number circle icons (indices 1–10).
// Codepoints extracted directly from window-status.zsh.
const UNSELECTED: [&str; 10] = [
    "\u{f03a5}", // 󰎥
    "\u{f03a8}", // 󰎨
    "\u{f03ab}", // 󰎫
    "\u{f03b2}", // 󰎲
    "\u{f03af}", // 󰎯
    "\u{f03b4}", // 󰎴
    "\u{f03b7}", // 󰎷
    "\u{f03ba}", // 󰎺
    "\u{f03bd}", // 󰎽
    "\u{f0feb}", // 󰿫
];

const SELECTED: [&str; 10] = [
    "\u{f0f0f}", // 󰼏
    "\u{f0f10}", // 󰼐
    "\u{f0f11}", // 󰼑
    "\u{f0f12}", // 󰼒
    "\u{f0f13}", // 󰼓
    "\u{f0f14}", // 󰼔
    "\u{f0f15}", // 󰼕
    "\u{f0f16}", // 󰼖
    "\u{f0f17}", // 󰼗
    "\u{f0fea}", // 󰿪
];

// 20-color cycle for running-process animation (seconds % 20)
const PS_COLORS: [&str; 20] = [
    "#54b435", "#5cae36", "#63a837", "#6ba338", "#739d39", "#7b9739", "#82913a", "#8a8b3b",
    "#92863c", "#9a803d", "#a17a3e", "#a9743f", "#b16f40", "#b96941", "#c06342", "#c85d42",
    "#d05743", "#d85244", "#df4c45", "#e74646",
];

// Dir logos — applied in priority order (most specific first)
struct DirLogo {
    prefix: &'static str,
    logo: &'static str,
}

// Codepoints extracted directly from window-status.zsh dir_logos array.
const DIR_LOGOS: &[DirLogo] = &[
    DirLogo { prefix: "~/g/mysetup", logo: "\u{f0630} " },  // 󰘰 mysetup icon
    DirLogo { prefix: "~/g/", logo: "\u{f1d3} /" },         // git repos
    DirLogo { prefix: "~/b/", logo: "\u{f0bee} /" },        // 󰯮 bin/scripts
    DirLogo { prefix: "~/", logo: "\u{f0827} " },            // 󰠧 home sub
    DirLogo { prefix: "~", logo: "\u{f0827}" },              // 󰠧 home
    DirLogo { prefix: "/", logo: "\u{eb45} " },              //  root
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowArgs {
    #[serde(default)]
    pub current: bool,
    pub index: u32,
    #[serde(default)]
    pub window_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub process: Option<String>,
    #[serde(default)]
    pub start_path: Option<PathBuf>,
    #[serde(default)]
    pub flags: Option<String>,
    #[serde(default)]
    pub last: u32,
}

pub fn render(args: &WindowArgs, dir_aliases: &HashMap<PathBuf, String>) -> String {
    let home = dirs_home();
    let path = args.path.as_deref().unwrap_or(Path::new("/"));
    let process = args.process.as_deref().unwrap_or("zsh");
    let flags = args.flags.as_deref().unwrap_or("");

    // Process animation: non-zsh process in non-current window cycles colors
    let (running, index_color) = if process != "zsh" && !args.current {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ci = (secs % 20) as usize;
        (true, format!("#[fg={},none]", PS_COLORS[ci]))
    } else {
        (false, String::new())
    };

    // Title: check override name first, then dir aliases, then abbreviate path
    let name_override = args
        .name
        .as_deref()
        .filter(|n| !n.is_empty() && *n != "zsh" && Some(*n) != args.process.as_deref());

    let title = if let Some(name) = name_override {
        name.to_string()
    } else if let Some(alias) = dir_aliases.get(path) {
        alias.clone()
    } else {
        abbreviate_path(path, &home)
    };

    // Index icon
    let idx = args.index as usize;
    let icon = if args.current {
        SELECTED.get(idx.wrapping_sub(1)).copied().unwrap_or("?")
    } else {
        UNSELECTED.get(idx.wrapping_sub(1)).copied().unwrap_or("?")
    };

    // Colors
    let (final_index_color, title_color) = if args.current {
        (
            "#[fg=#ffffff,bg=color233]".to_string(),
            "#[fg=#e3f2fd,none]".to_string(),
        )
    } else if args.last == 1 {
        let ic = if running { index_color } else { "#[fg=#adb5bd,none]".to_string() };
        (ic, "#[fg=#adb5bd,none]".to_string())
    } else {
        let ic = if running { index_color } else { "#[fg=color242,none]".to_string() };
        (ic, "#[fg=color242,none]".to_string())
    };

    // Alert indicator
    let alert = if flags.contains('#') {
        if flags.contains('!') {
            " #[fg=#ff0000]\u{f0027}".to_string() // alert icon
        } else if !running {
            " #[fg=#5fd700]\u{f063e}".to_string() // activity icon 󰘾
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    format!(" {}{}{} {}{}", final_index_color, icon, title_color, title, alert)
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn abbreviate_path(path: &Path, home: &Path) -> String {
    use std::path::Component;

    // Strip home prefix and replace with ~, otherwise use the path directly.
    // We separate the string prefix ("~" or "") from the remaining components.
    let (str_prefix, rest): (&str, &Path) = match path.strip_prefix(home) {
        Ok(rel) => ("~", rel),
        Err(_) => ("", path),
    };

    // Collect only Normal/CurDir/ParentDir components (skip RootDir).
    let comps: Vec<String> = rest
        .components()
        .filter_map(|c| match c {
            Component::RootDir => None,
            other => Some(other.as_os_str().to_string_lossy().into_owned()),
        })
        .collect();

    let is_abs = str_prefix.is_empty() && path.is_absolute();

    // If there are no path components, the path is the prefix alone (e.g. home or /).
    if comps.is_empty() {
        let base = if is_abs { "/".to_string() } else { str_prefix.to_string() };
        for logo in DIR_LOGOS {
            if base.starts_with(logo.prefix) {
                return base.replacen(logo.prefix, logo.logo, 1);
            }
        }
        return base;
    }

    // Abbreviate all components except the last.
    let last = comps.len() - 1;
    let mut parts: Vec<String> = comps
        .iter()
        .enumerate()
        .map(|(i, comp)| {
            if i == last {
                comp.clone()
            } else if comp.starts_with('.') {
                // Dot-prefixed: keep first two visible chars.
                comp.chars().take(2).collect()
            } else {
                comp.chars().next().map(|c| c.to_string()).unwrap_or_default()
            }
        })
        .collect();

    // Ellipsize basename if longer than 17 chars.
    if let Some(base) = parts.last_mut() {
        let len = base.chars().count();
        if len > 17 {
            let head: String = base.chars().take(7).collect();
            let tail: String =
                base.chars().rev().take(7).collect::<String>().chars().rev().collect();
            *base = format!("{}...{}", head, tail);
        }
    }

    let inner = parts.join("/");

    // Reconstruct with the correct prefix (no double slashes).
    let abbreviated = if !str_prefix.is_empty() {
        format!("{}/{}", str_prefix, inner)
    } else if is_abs {
        format!("/{}", inner)
    } else {
        inner
    };

    // Apply dir logo substitutions in priority order.
    for logo in DIR_LOGOS {
        if abbreviated.starts_with(logo.prefix) {
            return abbreviated.replacen(logo.prefix, logo.logo, 1);
        }
    }

    abbreviated
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── icon codepoints ───────────────────────────────────────────────────────

    #[test]
    fn selected_icon_codepoints() {
        // U+F0F0F = f3 b0 bc 8f
        assert_eq!(SELECTED[0].as_bytes(), &[0xf3, 0xb0, 0xbc, 0x8f]);
    }

    #[test]
    fn unselected_icon_codepoints() {
        // U+F03A5 = f3 b0 8e a5
        assert_eq!(UNSELECTED[0].as_bytes(), &[0xf3, 0xb0, 0x8e, 0xa5]);
    }

    #[test]
    fn icon_arrays_have_ten_entries() {
        assert_eq!(SELECTED.len(), 10);
        assert_eq!(UNSELECTED.len(), 10);
        for (s, u) in SELECTED.iter().zip(UNSELECTED.iter()) {
            assert!(!s.is_empty());
            assert!(!u.is_empty());
            // selected and unselected icons at same index must differ
            assert_ne!(s, u);
        }
    }

    // ── abbreviate_path ───────────────────────────────────────────────────────

    fn home() -> PathBuf {
        PathBuf::from("/Users/test")
    }

    #[test]
    fn abbrev_home_subdir_shows_git_logo() {
        let path = PathBuf::from("/Users/test/git-repos/myproject");
        let result = abbreviate_path(&path, &home());
        // ~/git-repos/myproject → ~/g/myproject → git-logo /myproject
        assert!(result.contains("myproject"), "result: {result}");
    }

    #[test]
    fn abbrev_exact_home_uses_home_logo() {
        let result = abbreviate_path(&home(), &home());
        // "~" → home logo (no trailing slash)
        let logo = "\u{f0827}";
        assert!(result.contains(logo), "expected home logo in: {result}");
    }

    #[test]
    fn abbrev_home_slash_uses_tilde_slash_logo() {
        let path = PathBuf::from("/Users/test/Documents");
        let result = abbreviate_path(&path, &home());
        // ~/Documents → ~/D → tilde+slash logo + D
        let logo = "\u{f0827} "; // ~/  logo
        assert!(result.contains(logo), "expected ~/ logo in: {result}");
        assert!(result.contains("Documents"), "result: {result}");
    }

    #[test]
    fn abbrev_absolute_root_path() {
        let path = PathBuf::from("/tmp");
        let result = abbreviate_path(&path, &home());
        // /tmp → root logo + tmp
        let root_logo = "\u{eb45} ";
        assert!(result.contains(root_logo), "expected root logo in: {result}");
        assert!(result.contains("tmp"), "result: {result}");
    }

    #[test]
    fn abbrev_abbreviates_intermediate_components() {
        // /Users/test/a/b/c/d/myproject → ~/a/b/c/d/myproject
        // intermediate: a→a, b→b, c→c, d→d  (first char only)
        let path = PathBuf::from("/Users/test/alpha/beta/gamma/myproject");
        let result = abbreviate_path(&path, &home());
        // Should contain abbreviated intermediates and full basename
        assert!(result.contains("myproject"), "basename: {result}");
        assert!(!result.contains("alpha"), "alpha abbreviated: {result}");
        assert!(!result.contains("beta"), "beta abbreviated: {result}");
        assert!(!result.contains("gamma"), "gamma abbreviated: {result}");
    }

    #[test]
    fn abbrev_dotfile_component_keeps_two_chars() {
        let path = PathBuf::from("/Users/test/.config/nvim");
        let result = abbreviate_path(&path, &home());
        // .config → .c (two chars)
        assert!(result.contains(".c"), "dotfile abbreviated: {result}");
        assert!(result.contains("nvim"), "basename: {result}");
    }

    #[test]
    fn abbrev_long_basename_ellipsized() {
        let path = PathBuf::from("/Users/test/a-very-long-directory-name");
        let result = abbreviate_path(&path, &home());
        assert!(result.contains("..."), "expected ellipsis: {result}");
        // head=7 + "..." + tail=7
        let basename_part: String = result.chars().skip_while(|&c| c != 'a').collect();
        // Just verify head and tail are present
        let orig = "a-very-long-directory-name";
        let head: String = orig.chars().take(7).collect();
        let tail: String = orig.chars().rev().take(7).collect::<String>().chars().rev().collect();
        assert!(result.contains(&head), "head: {result}");
        assert!(result.contains(&tail), "tail: {result}");
        let _ = basename_part;
    }

    #[test]
    fn abbrev_short_basename_not_ellipsized() {
        let path = PathBuf::from("/Users/test/shortname");
        let result = abbreviate_path(&path, &home());
        assert!(!result.contains("..."), "should not ellipsize: {result}");
        assert!(result.contains("shortname"), "result: {result}");
    }

    #[test]
    fn abbrev_exactly_17_char_basename_not_ellipsized() {
        let seventeen = "a".repeat(17);
        let path = PathBuf::from(format!("/Users/test/{}", seventeen));
        let result = abbreviate_path(&path, &home());
        assert!(!result.contains("..."), "17 chars should not ellipsize: {result}");
    }

    #[test]
    fn abbrev_mysetup_gets_special_logo() {
        let path = PathBuf::from("/Users/test/git-repos/mysetup");
        let result = abbreviate_path(&path, &home());
        let logo = "\u{f0630} ";
        assert!(result.starts_with(logo), "expected mysetup logo: {result}");
    }

    // ── render ────────────────────────────────────────────────────────────────

    fn args_base() -> WindowArgs {
        WindowArgs {
            current: false,
            index: 1,
            window_id: Some("@1".into()),
            name: None,
            path: Some(PathBuf::from("/tmp")),
            process: Some("zsh".into()),
            start_path: None,
            flags: Some(String::new()),
            last: 0,
        }
    }

    fn no_aliases() -> HashMap<PathBuf, String> {
        HashMap::new()
    }

    #[test]
    fn render_current_uses_selected_icon() {
        let args = WindowArgs { current: true, index: 1, ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains(SELECTED[0]), "expected selected[0] in: {out}");
        assert!(!out.contains(UNSELECTED[0]), "should not use unselected: {out}");
    }

    #[test]
    fn render_non_current_uses_unselected_icon() {
        let args = WindowArgs { current: false, index: 1, ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains(UNSELECTED[0]), "expected unselected[0]: {out}");
    }

    #[test]
    fn render_index_2() {
        let args = WindowArgs { index: 2, ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains(UNSELECTED[1]));
    }

    #[test]
    fn render_index_10_valid() {
        let args = WindowArgs { index: 10, ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains(UNSELECTED[9]));
    }

    #[test]
    fn render_current_window_white_title_color() {
        let args = WindowArgs { current: true, index: 1, ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains("#ffffff"), "expected white fg: {out}");
        assert!(out.contains("#e3f2fd"), "expected light blue title: {out}");
    }

    #[test]
    fn render_last_window_grey_color() {
        let args = WindowArgs { last: 1, index: 1, ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains("#adb5bd"), "expected grey for last window: {out}");
    }

    #[test]
    fn render_other_window_dim_color() {
        let args = WindowArgs { index: 1, ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains("color242"), "expected dim for other window: {out}");
    }

    #[test]
    fn render_name_override_used_as_title() {
        let args = WindowArgs {
            name: Some("my-session".into()),
            process: Some("zsh".into()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains("my-session"), "override name: {out}");
    }

    #[test]
    fn render_zsh_override_not_used() {
        // name="zsh" is treated the same as no override (falls through to path)
        let args = WindowArgs { name: Some("zsh".into()), ..args_base() };
        let out = render(&args, &no_aliases());
        // should show path-based title, not "zsh"
        assert!(out.contains("tmp"), "should use path: {out}");
    }

    #[test]
    fn render_dir_alias_used() {
        let mut aliases = HashMap::new();
        aliases.insert(PathBuf::from("/tmp"), "my-alias".to_string());
        let args = WindowArgs { path: Some(PathBuf::from("/tmp")), ..args_base() };
        let out = render(&args, &aliases);
        assert!(out.contains("my-alias"), "alias used: {out}");
    }

    #[test]
    fn render_activity_flag_shows_indicator() {
        let args = WindowArgs { flags: Some("#".into()), ..args_base() };
        let out = render(&args, &no_aliases());
        // Activity flag (#) → green indicator
        assert!(out.contains("#5fd700") || out.contains("\u{f063e}"), "activity: {out}");
    }

    #[test]
    fn render_bell_flag_shows_red() {
        let args = WindowArgs { flags: Some("#!".into()), ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(out.contains("#ff0000"), "bell red: {out}");
    }

    #[test]
    fn render_no_flag_no_alert() {
        let args = WindowArgs { flags: Some(String::new()), ..args_base() };
        let out = render(&args, &no_aliases());
        assert!(!out.contains("#ff0000"), "no bell: {out}");
        assert!(!out.contains("#5fd700"), "no activity: {out}");
    }

    #[test]
    fn render_running_process_shows_color_cycle() {
        let args = WindowArgs {
            current: false,
            process: Some("cargo".into()), // non-zsh
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        // Should contain one of the PS_COLORS entries (hex color)
        let has_ps_color = PS_COLORS.iter().any(|c| out.contains(c));
        assert!(has_ps_color, "expected a process color: {out}");
    }

    #[test]
    fn render_output_starts_with_space() {
        // All window status outputs lead with a space (matches zsh script).
        let args = args_base();
        let out = render(&args, &no_aliases());
        assert!(out.starts_with(' '), "should start with space: {out:?}");
    }
}
