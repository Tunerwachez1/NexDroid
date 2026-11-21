//! Limits are applied to the LXC payload, which lives outside the systemd service cgroup.
use std::path::Path;

pub fn apply(cgroup: &Path, count: u32, memory_mib: u32) -> anyhow::Result<()> {
    let parent = cgroup.parent().ok_or_else(|| anyhow::anyhow!("Missing cgroup parent"))?;
    let available = std::fs::read_to_string(parent.join("cpuset.cpus.effective"))?;
    let ids = cpu_ids(&available)?;
    anyhow::ensure!(count as usize <= ids.len(), "CPU allocation exceeds available CPUs");
    let memory_total = std::fs::read_to_string("/proc/meminfo")?;
    let total_kib: u64 = memory_total.lines().find(|l| l.starts_with("MemTotal:"))
        .and_then(|l| l.split_whitespace().nth(1)).unwrap_or("0").parse()?;
    anyhow::ensure!(memory_mib == 0 || (memory_mib >= 1024 && u64::from(memory_mib) * 1024 <= total_kib), "Invalid memory limit");
    let memory_bytes = u64::from(memory_mib) * 1048576;
    let used: u64 = std::fs::read_to_string(cgroup.join("memory.current"))?.trim().parse()?;
    anyhow::ensure!(memory_mib == 0 || memory_bytes >= used + 128 * 1048576, "RAM limit is below current usage plus 128 MiB. Close apps or choose a higher limit.");
    let cpu_path = cgroup.join("cpuset.cpus");
    let memory_path = cgroup.join("memory.max");
    let previous_cpu = std::fs::read_to_string(&cpu_path)?;
    let cpus = if count == 0 { String::new() } else {
        ids.iter().take(count as usize).map(ToString::to_string).collect::<Vec<_>>().join(",")
    };
    std::fs::write(&cpu_path, format!("{cpus}\n"))?;
    let memory = if memory_mib == 0 { "max".to_owned() } else { memory_bytes.to_string() };
    if let Err(error) = std::fs::write(&memory_path, memory) {
        std::fs::write(&cpu_path, previous_cpu).map_err(|rollback| anyhow::anyhow!("{error}; CPU rollback failed: {rollback}"))?;
        return Err(error.into());
    }
    Ok(())
}
fn cpu_ids(text: &str) -> anyhow::Result<Vec<u32>> {
    let mut ids = Vec::new();
    for part in text.trim().split(',') {
        let (first, last) = part.split_once('-').unwrap_or((part, part));
        let first: u32 = first.parse()?;
        let last: u32 = last.parse()?;
        anyhow::ensure!(first <= last && last < 65536, "Invalid CPU range");
        ids.extend(first..=last);
    }
    Ok(ids)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_cpu_ids_are_preserved() {
        assert_eq!(cpu_ids("2-3,8,10-11\n").unwrap(), [2,3,8,10,11]);
        assert!(cpu_ids("8-2").is_err());
        assert!(cpu_ids("0-9999999").is_err());
    }
}
