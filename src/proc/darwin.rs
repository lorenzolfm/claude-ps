pub fn start_time(pid: u32) -> Option<String> {
    start_secs(pid).map(format_lstart)
}

pub fn environ(pid: u32) -> Option<Vec<u8>> {
    let raw = procargs(pid)?;
    split_procargs(&raw).map(|(_, environ)| environ.to_vec())
}

pub fn cmdline(pid: u32) -> Option<Vec<u8>> {
    let raw = procargs(pid)?;
    split_procargs(&raw).map(|(cmdline, _)| cmdline.to_vec())
}

pub fn pid_domain() -> Option<String> {
    Some("darwin".to_owned())
}

const CTL_KERN: i32 = 1;
const KERN_ARGMAX: i32 = 8;
const KERN_PROC: i32 = 14;
const KERN_PROC_PID: i32 = 1;
const KERN_PROCARGS2: i32 = 49;

unsafe extern "C" {
    fn sysctl(
        name: *const i32,
        namelen: u32,
        oldp: *mut u8,
        oldlenp: *mut usize,
        newp: *const u8,
        newlen: usize,
    ) -> i32;
}

fn start_secs(pid: u32) -> Option<i64> {
    let mib = [CTL_KERN, KERN_PROC, KERN_PROC_PID, i32::try_from(pid).ok()?];
    let mut info = [0u8; 1024];
    let mut len = info.len();
    let rc = unsafe {
        sysctl(
            mib.as_ptr(),
            4,
            info.as_mut_ptr(),
            &raw mut len,
            std::ptr::null(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let (secs, _) = info[..len].split_first_chunk::<8>()?;
    Some(i64::from_ne_bytes(*secs))
}

fn format_lstart(secs: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let days = secs.div_euclid(86_400);
    let clock = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{} {} {day:>2} {:02}:{:02}:{:02} {year}",
        WEEKDAYS[days.rem_euclid(7) as usize],
        MONTHS[month as usize - 1],
        clock / 3600,
        clock % 3600 / 60,
        clock % 60,
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn argmax() -> Option<usize> {
    let mib = [CTL_KERN, KERN_ARGMAX];
    let mut argmax: i32 = 0;
    let mut len = std::mem::size_of::<i32>();
    let rc = unsafe {
        sysctl(
            mib.as_ptr(),
            2,
            (&raw mut argmax).cast::<u8>(),
            &raw mut len,
            std::ptr::null(),
            0,
        )
    };
    (rc == 0 && argmax > 0).then_some(argmax as usize)
}

fn procargs(pid: u32) -> Option<Vec<u8>> {
    let mib = [CTL_KERN, KERN_PROCARGS2, i32::try_from(pid).ok()?];
    let mut len = argmax()?;
    let mut raw = vec![0u8; len];
    let rc = unsafe {
        sysctl(
            mib.as_ptr(),
            3,
            raw.as_mut_ptr(),
            &raw mut len,
            std::ptr::null(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    raw.truncate(len);
    Some(raw)
}

fn split_procargs(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    let (argc, rest) = raw.split_first_chunk::<4>()?;
    let argc = usize::try_from(i32::from_ne_bytes(*argc)).ok()?;
    let exec_path_end = rest.iter().position(|byte| *byte == 0)?;
    let args_start = rest[exec_path_end..]
        .iter()
        .position(|byte| *byte != 0)
        .map_or(rest.len(), |offset| exec_path_end + offset);
    let strings = &rest[args_start..];
    let mut cmdline_end = 0;
    for _ in 0..argc {
        let nul = strings[cmdline_end..].iter().position(|byte| *byte == 0)?;
        cmdline_end += nul + 1;
    }
    Some(strings.split_at(cmdline_end))
}

#[cfg(test)]
mod tests {
    fn procargs(argc: i32, body: &[u8]) -> Vec<u8> {
        let mut raw = argc.to_ne_bytes().to_vec();
        raw.extend_from_slice(body);
        raw
    }

    #[test]
    fn lstart_is_the_c_locale_ctime_shape_in_utc() {
        assert_eq!(super::format_lstart(0), "Thu Jan  1 00:00:00 1970");
        assert_eq!(
            super::format_lstart(1_789_300_185),
            "Sun Sep 13 11:49:45 2026"
        );
        assert_eq!(
            super::format_lstart(951_868_799),
            "Tue Feb 29 23:59:59 2000"
        );
        assert_eq!(
            super::format_lstart(1_709_251_200),
            "Fri Mar  1 00:00:00 2024"
        );
    }

    #[test]
    fn the_start_time_agrees_with_ps_when_ps_can_run() {
        let pid = std::process::id();
        let Ok(output) = std::process::Command::new("/bin/ps")
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .args(["-o", "lstart=", "-p"])
            .arg(pid.to_string())
            .output()
        else {
            return;
        };
        let from_ps = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(super::start_time(pid).as_deref(), Some(from_ps.as_str()));
    }

    #[test]
    fn a_pid_nothing_runs_under_has_no_start_time() {
        assert_eq!(super::start_time(99_999), None);
    }

    #[test]
    fn procargs_splits_at_the_argc_th_nul() {
        let raw = procargs(
            2,
            b"/usr/bin/claude\0\0\0claude\0--resume\0HOME=/Users/x\0ZELLIJ_PANE_ID=3\0",
        );
        let (cmdline, environ) = super::split_procargs(&raw).unwrap();
        assert_eq!(cmdline, b"claude\0--resume\0");
        assert_eq!(environ, b"HOME=/Users/x\0ZELLIJ_PANE_ID=3\0");
    }

    #[test]
    fn procargs_without_padding_still_splits() {
        let raw = procargs(1, b"/bin/x\0x\0K=v\0");
        let (cmdline, environ) = super::split_procargs(&raw).unwrap();
        assert_eq!(cmdline, b"x\0");
        assert_eq!(environ, b"K=v\0");
    }

    #[test]
    fn procargs_rejects_a_truncated_buffer() {
        assert_eq!(super::split_procargs(b"\x02\0"), None);
        assert_eq!(super::split_procargs(&procargs(2, b"/bin/x\0x\0")), None);
        assert_eq!(super::split_procargs(&procargs(-1, b"/bin/x\0x\0")), None);
    }

    #[test]
    fn the_pid_domain_is_the_bare_word_darwin() {
        assert_eq!(super::pid_domain().as_deref(), Some("darwin"));
    }

    #[test]
    fn the_own_process_has_arguments_and_an_environment() {
        let pid = std::process::id();
        let cmdline = super::cmdline(pid).unwrap();
        assert!(
            cmdline
                .split(|byte| *byte == 0)
                .next()
                .is_some_and(|arg| !arg.is_empty())
        );
        let environ = super::environ(pid).unwrap();
        assert!(
            environ
                .split(|byte| *byte == 0)
                .any(|entry| entry.starts_with(b"PATH="))
        );
    }
}
