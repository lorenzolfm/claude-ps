fn proc_path(pid: u32, leaf: &str) -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from("/proc");
    path.push(pid.to_string());
    path.push(leaf);
    path
}

pub fn start_time(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(proc_path(pid, "stat")).ok()?;
    parse_start_time(&stat).map(str::to_owned)
}

pub fn parse_start_time(stat: &str) -> Option<&str> {
    let close = stat.rfind(')')?;
    stat[close + 1..].split_whitespace().nth(19)
}

pub fn environ(pid: u32) -> Option<Vec<u8>> {
    std::fs::read(proc_path(pid, "environ")).ok()
}

pub fn cmdline(pid: u32) -> Option<Vec<u8>> {
    std::fs::read(proc_path(pid, "cmdline")).ok()
}

pub fn pid_domain() -> Option<String> {
    let machine = machine_id()?;
    let namespace = std::fs::read_link("/proc/self/ns/pid").ok()?;
    Some(format!("linux:{machine}:{}", namespace.to_string_lossy()))
}

fn machine_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    const FIELDS_3_TO_21: &str = "R 1 1 1 0 -1 4194304 328 0 0 0 0 0 0 0 20 0 1 0";

    #[test]
    fn the_pid_domain_names_this_machine_and_this_namespace() {
        let Some(domain) = super::pid_domain() else {
            return;
        };
        assert!(domain.starts_with("linux:"), "{domain}");
        assert!(domain.contains(":pid:["), "{domain}");
    }

    #[test]
    fn start_time_is_field_22() {
        let stat = format!("3520542 (cat) {FIELDS_3_TO_21} 41288167 17489920 1165");
        assert_eq!(super::parse_start_time(&stat), Some("41288167"));
    }

    #[test]
    fn start_time_survives_parens_and_spaces_in_comm() {
        let stat = format!("1 ((sd pam)) {FIELDS_3_TO_21} 555 17489920 1165");
        assert_eq!(super::parse_start_time(&stat), Some("555"));
    }

    #[test]
    fn start_time_rejects_a_truncated_line() {
        assert_eq!(super::parse_start_time("42 (claude) S 0 0"), None);
        assert_eq!(super::parse_start_time("no parens here"), None);
    }
}
