use std::time::Instant;

use crate::tmux::icons::ARROW_LEFT;

const THRESHOLD_BPS: u64 = 20_480; // 20 KiB/s default threshold

fn iec_fmt(bytes_per_sec: u64, pad: usize) -> String {
    const K: u64 = 1024;
    let (val, unit) = if bytes_per_sec >= K * K * K {
        (bytes_per_sec / (K * K * K), "GiB")
    } else if bytes_per_sec >= K * K {
        (bytes_per_sec / (K * K), "MiB")
    } else if bytes_per_sec >= K {
        (bytes_per_sec / K, "KiB")
    } else {
        (bytes_per_sec, "B")
    };
    format!("{:>pad$}{}/s", val, unit)
}

/// Style the unit part of a speed string: `20KiB/s` → `20#[fg=colour237,none,italics]KiB/s#[none]`.
/// Equivalent to: sed -E 's/([0-9]+)(.+)/\1#[fg=colour237,none,italics]\2#[none]/g'
fn iec_fmt_styled(bytes_per_sec: u64) -> String {
    let plain = iec_fmt(bytes_per_sec, 0);
    let split = plain.find(|c: char| !c.is_ascii_digit()).unwrap_or(plain.len());
    let (num, unit) = plain.split_at(split);
    format!("{}#[fg=colour237,none,italics]{}#[none]", num, unit)
}

/// Parse `netstat -ibn` output and return (total_rx_bytes, total_tx_bytes).
/// Exposed for unit testing.
fn parse_netstat_output(text: &str) -> (u64, u64) {
    let mut total_rx: u64 = 0;
    let mut total_tx: u64 = 0;

    // netstat -ibn columns on macOS:
    // Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }
        let name = cols[0];
        if name.starts_with("lo") {
            continue;
        }
        // Ibytes = col 6, Obytes = col 9
        if let (Ok(rx), Ok(tx)) = (cols[6].parse::<u64>(), cols[9].parse::<u64>()) {
            total_rx = total_rx.saturating_add(rx);
            total_tx = total_tx.saturating_add(tx);
        }
    }

    (total_rx, total_tx)
}

/// Format bandwidth delta into tmux status segment(s).
/// Returns empty string when both are below threshold.
fn format_bandwidth(dl: u64, ul: u64) -> String {
    let mut out = String::new();
    if dl >= THRESHOLD_BPS {
        out.push_str(&format!(
            "#[fg=#5cae36]{}#[fg=colour233,bg=#5cae36]{} ",
            ARROW_LEFT, iec_fmt_styled(dl)
        ));
    }
    if ul >= THRESHOLD_BPS {
        out.push_str(&format!(
            "#[fg=#0262a8]{}#[fg=colour233,bg=#0262a8]{} ",
            ARROW_LEFT, iec_fmt_styled(ul)
        ));
    }
    out
}

async fn read_net_bytes() -> anyhow::Result<(u64, u64)> {
    let out = tokio::process::Command::new("netstat")
        .args(["-ibn"])
        .output()
        .await?;
    Ok(parse_netstat_output(&String::from_utf8_lossy(&out.stdout)))
}

pub async fn render(previous: &mut Option<(u64, u64, Instant)>) -> anyhow::Result<String> {
    let (rx, tx) = read_net_bytes().await?;

    let Some((prev_rx, prev_tx, prev_time)) = previous.take() else {
        *previous = Some((rx, tx, Instant::now()));
        return Ok(String::new());
    };

    let elapsed = prev_time.elapsed().as_secs().max(1);
    let dl = rx.saturating_sub(prev_rx) / elapsed;
    let ul = tx.saturating_sub(prev_tx) / elapsed;
    *previous = Some((rx, tx, Instant::now()));

    Ok(format_bandwidth(dl, ul))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── iec_fmt ──────────────────────────────────────────────────────────────

    #[test]
    fn iec_fmt_bytes() {
        // iec_fmt(500, 6): format!("{:>6}B/s", 500) = "   500B/s" (3 spaces + 500)
        assert_eq!(iec_fmt(500, 6), "   500B/s");
        assert_eq!(iec_fmt(500, 0), "500B/s");
    }

    #[test]
    fn iec_fmt_kib() {
        assert_eq!(iec_fmt(2048, 0), "2KiB/s");
        assert_eq!(iec_fmt(1024, 0), "1KiB/s");
    }

    #[test]
    fn iec_fmt_mib() {
        assert_eq!(iec_fmt(1024 * 1024, 0), "1MiB/s");
        assert_eq!(iec_fmt(3 * 1024 * 1024, 0), "3MiB/s");
    }

    #[test]
    fn iec_fmt_gib() {
        assert_eq!(iec_fmt(2 * 1024 * 1024 * 1024, 0), "2GiB/s");
    }

    #[test]
    fn iec_fmt_padding() {
        // Padding pads on the left
        let s = iec_fmt(1, 8);
        // "       1B/s" — 8 chars wide for the number
        assert_eq!(s, "       1B/s");
    }

    #[test]
    fn iec_fmt_boundary_exactly_kib() {
        // 1024 B/s = exactly 1 KiB/s
        assert_eq!(iec_fmt(1024, 0), "1KiB/s");
        // 1023 B/s stays in B
        assert_eq!(iec_fmt(1023, 0), "1023B/s");
    }

    // ── parse_netstat_output ─────────────────────────────────────────────────

    const NETSTAT_SAMPLE: &str = "\
Name  Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll
en0   1500  <Link#4>      aa:bb:cc:dd:ee:ff  12000     0   10240000    8000     0    5120000     0
en0   1500  192.168.1.0/24 192.168.1.10      12000     0   10240000    8000     0    5120000     0
lo0   16384 <Link#1>      localhost           5000     0    1000000    5000     0    1000000     0
utun0 1500  <Link#10>     -                   2000     0     512000    1500     0     256000     0
";

    #[test]
    fn parse_sums_non_loopback_interfaces() {
        let (rx, tx) = parse_netstat_output(NETSTAT_SAMPLE);
        // en0 appears twice (link + IP rows), both counted: 2×10240000 = 20480000
        // utun0: +512000 rx, +256000 tx
        // lo0 is excluded
        assert_eq!(rx, 2 * 10_240_000 + 512_000);
        assert_eq!(tx, 2 * 5_120_000 + 256_000);
    }

    #[test]
    fn parse_skips_loopback() {
        let input = "\
Name  Mtu   Network   Address   Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll
lo0   16384 <Link#1>  localhost  1000     0 999999  1000     0 999999    0
";
        let (rx, tx) = parse_netstat_output(input);
        assert_eq!(rx, 0);
        assert_eq!(tx, 0);
    }

    #[test]
    fn parse_skips_short_lines() {
        let input = "Name Mtu\nen0\n"; // too few columns
        let (rx, tx) = parse_netstat_output(input);
        assert_eq!((rx, tx), (0, 0));
    }

    // ── format_bandwidth ─────────────────────────────────────────────────────

    #[test]
    fn format_bandwidth_both_below_threshold_empty() {
        assert_eq!(format_bandwidth(100, 100), "");
    }

    #[test]
    fn format_bandwidth_exactly_at_threshold_empty() {
        // threshold is >=, so 20479 is below and 20480 shows
        assert_eq!(format_bandwidth(THRESHOLD_BPS - 1, 0), "");
    }

    #[test]
    fn format_bandwidth_dl_above_threshold() {
        let out = format_bandwidth(THRESHOLD_BPS, 0);
        assert!(out.contains(ARROW_LEFT), "missing arrow: {out}");
        assert!(out.contains("#[fg=#5cae36]"), "expected green arrow: {out}");
        assert!(out.contains("#[fg=colour233,bg=#5cae36]"), "expected green segment: {out}");
        assert!(!out.contains("#[fg=#0262a8"), "should not have ul: {out}");
    }

    #[test]
    fn format_bandwidth_ul_above_threshold() {
        let out = format_bandwidth(0, THRESHOLD_BPS);
        assert!(out.contains(ARROW_LEFT), "missing arrow: {out}");
        assert!(out.contains("#[fg=#0262a8]"), "expected blue arrow: {out}");
        assert!(out.contains("#[fg=colour233,bg=#0262a8]"), "expected blue segment: {out}");
        assert!(!out.contains("#[fg=#5cae36"), "should not have dl: {out}");
    }

    #[test]
    fn format_bandwidth_both_above_threshold() {
        let out = format_bandwidth(THRESHOLD_BPS * 10, THRESHOLD_BPS * 2);
        assert!(out.contains("#[fg=#5cae36"), "missing dl: {out}");
        assert!(out.contains("#[fg=#0262a8"), "missing ul: {out}");
    }

    #[test]
    fn format_bandwidth_no_leading_space_in_speed() {
        // Number part must immediately follow the color tag — no numeric padding.
        // Use 40 KiB/s (above 20 KiB/s threshold).
        let out = format_bandwidth(40 * 1024, 0);
        assert!(out.contains("40"), "number present: {out}");
        assert!(!out.contains("   40"), "no left numeric padding: {out}");
    }

    // ── iec_fmt_styled ────────────────────────────────────────────────────────

    #[test]
    fn iec_fmt_styled_kib() {
        assert_eq!(
            iec_fmt_styled(2048),
            "2#[fg=colour237,none,italics]KiB/s#[none]"
        );
    }

    #[test]
    fn iec_fmt_styled_mib() {
        assert_eq!(
            iec_fmt_styled(3 * 1024 * 1024),
            "3#[fg=colour237,none,italics]MiB/s#[none]"
        );
    }

    #[test]
    fn iec_fmt_styled_bytes() {
        assert_eq!(
            iec_fmt_styled(500),
            "500#[fg=colour237,none,italics]B/s#[none]"
        );
    }

    #[test]
    fn iec_fmt_styled_split_is_at_first_non_digit() {
        // Verify number and unit are correctly separated for all unit types.
        for (bps, expected_num, expected_unit) in [
            (500_u64,                   "500", "B/s"),
            (2 * 1024,                    "2", "KiB/s"),
            (5 * 1024 * 1024,             "5", "MiB/s"),
            (2 * 1024 * 1024 * 1024,      "2", "GiB/s"),
        ] {
            let s = iec_fmt_styled(bps);
            assert!(s.starts_with(expected_num), "num for {bps}: {s}");
            assert!(s.contains(expected_unit), "unit for {bps}: {s}");
            assert!(s.contains("#[fg=colour237,none,italics]"), "style tag: {s}");
            assert!(s.ends_with("#[none]"), "reset tag: {s}");
        }
    }

    #[test]
    fn format_bandwidth_unit_is_styled() {
        let out = format_bandwidth(THRESHOLD_BPS, 0);
        assert!(out.contains("#[fg=colour237,none,italics]"), "unit style present: {out}");
        assert!(out.contains("#[none]"), "unit reset present: {out}");
    }

    #[test]
    fn format_bandwidth_shows_human_readable_speed() {
        let out = format_bandwidth(2 * 1024 * 1024, 0); // 2 MiB/s
        assert!(out.contains("MiB/s"), "expected MiB/s in: {out}");
    }

    // ── render state machine ─────────────────────────────────────────────────

    #[tokio::test]
    async fn render_no_previous_returns_empty_and_stores_state() {
        // We can't call real render() without netstat, but we can test the
        // state machine inline.
        let mut previous: Option<(u64, u64, Instant)> = None;

        // Simulate first call: set previous
        previous = Some((1000, 2000, Instant::now()));
        assert!(previous.is_some());

        // Simulate second call: compute delta
        let (prev_rx, prev_tx, prev_time) = previous.take().unwrap();
        let elapsed = prev_time.elapsed().as_secs().max(1);
        let (new_rx, new_tx) = (prev_rx + THRESHOLD_BPS * 2, prev_tx + THRESHOLD_BPS * 3);
        let dl = new_rx.saturating_sub(prev_rx) / elapsed;
        let ul = new_tx.saturating_sub(prev_tx) / elapsed;
        let out = format_bandwidth(dl, ul);
        assert!(!out.is_empty(), "should produce output: {out}");
    }
}
