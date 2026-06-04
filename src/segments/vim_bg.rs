const VIM_OUTPUT: &str =
    "#[fg=#0262a8,bg=colour235,none] n#[fg=#539035]im#[fg=colour235,bg=colour233]";

/// Returns true if a single `ps -o stat=,comm=` output line represents a
/// stopped (state=T) nvim process.
fn is_suspended_nvim(ps_line: &str) -> bool {
    ps_line.starts_with('T') && ps_line.contains("nvim")
}

pub async fn has_suspended_nvim(pane_pid: u32) -> anyhow::Result<bool> {
    let child_out = tokio::process::Command::new("pgrep")
        .args(["-P", &pane_pid.to_string()])
        .output()
        .await?;

    let child_pids: Vec<u32> = String::from_utf8_lossy(&child_out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();

    for pid in child_pids {
        let ps_out = tokio::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat=,comm="])
            .output()
            .await?;
        if is_suspended_nvim(&String::from_utf8_lossy(&ps_out.stdout)) {
            return Ok(true)
        }
    }
    return Ok(false)
}

pub async fn render(pane_pid: u32) -> anyhow::Result<String> {
    if has_suspended_nvim(pane_pid).await? {
        return Ok(VIM_OUTPUT.to_string());
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_stopped_nvim() {
        assert!(is_suspended_nvim("T  nvim"));
        assert!(is_suspended_nvim("T+ nvim\n"));
        assert!(is_suspended_nvim("T  nvim --noplugin"));
    }

    #[test]
    fn ignores_running_nvim() {
        assert!(!is_suspended_nvim("S  nvim")); // sleeping, not stopped
        assert!(!is_suspended_nvim("R  nvim")); // running
    }

    #[test]
    fn ignores_stopped_non_nvim() {
        assert!(!is_suspended_nvim("T  zsh"));
        assert!(!is_suspended_nvim("T  vim")); // vim ≠ nvim
        assert!(!is_suspended_nvim("T  node"));
    }

    #[test]
    fn empty_line_is_not_match() {
        assert!(!is_suspended_nvim(""));
        assert!(!is_suspended_nvim("   "));
    }

    #[test]
    fn output_constant_contains_color_codes() {
        assert!(VIM_OUTPUT.contains("fg=#0262a8"));
        assert!(VIM_OUTPUT.contains("fg=#539035"));
        assert!(VIM_OUTPUT.contains("n"));
        assert!(VIM_OUTPUT.contains("im"));
    }
}
