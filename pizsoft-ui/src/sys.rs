use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct CpuSample {
    pub busy: u64,
    pub total: u64,
}

impl CpuSample {
    pub fn parse_proc_stat(text: &str) -> Option<Self> {
        let line = text.lines().find(|l| l.starts_with("cpu "))?;
        let nums: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        if nums.len() < 4 {
            return None;
        }
        let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
        let total: u64 = nums.iter().sum();
        let busy = total.saturating_sub(idle);
        Some(Self { busy, total })
    }

    pub fn percent_since(self, prev: Self) -> u8 {
        let dt = self.total.saturating_sub(prev.total);
        if dt == 0 {
            return 0;
        }
        let db = self.busy.saturating_sub(prev.busy);
        ((db.saturating_mul(100)) / dt).min(100) as u8
    }
}

pub fn read_cpu_sample() -> Option<CpuSample> {
    let text = fs::read_to_string("/proc/stat").ok()?;
    CpuSample::parse_proc_stat(&text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    pub total_mb: u64,
    pub used_mb: u64,
    pub percent: u8,
}

impl MemInfo {
    pub fn parse_proc_meminfo(text: &str) -> Self {
        let mut total_kb = 0u64;
        let mut avail_kb = None;
        let mut free_kb = 0u64;
        let mut buffers_kb = 0u64;
        let mut cached_kb = 0u64;

        for line in text.lines() {
            let Some((key, rest)) = line.split_once(':') else {
                continue;
            };
            let val = rest
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            match key.trim() {
                "MemTotal" => total_kb = val,
                "MemAvailable" => avail_kb = Some(val),
                "MemFree" => free_kb = val,
                "Buffers" => buffers_kb = val,
                "Cached" => cached_kb = val,
                _ => {}
            }
        }

        if total_kb == 0 {
            return Self {
                total_mb: 512,
                used_mb: 64,
                percent: 12,
            };
        }

        let avail = avail_kb.unwrap_or(free_kb + buffers_kb + cached_kb).min(total_kb);
        let used_kb = total_kb.saturating_sub(avail);
        let percent = ((used_kb.saturating_mul(100)) / total_kb).min(100) as u8;
        Self {
            total_mb: total_kb / 1024,
            used_mb: used_kb / 1024,
            percent,
        }
    }
}

pub fn read_meminfo() -> MemInfo {
    match fs::read_to_string("/proc/meminfo") {
        Ok(text) => MemInfo::parse_proc_meminfo(&text),
        Err(_) => MemInfo {
            total_mb: 512,
            used_mb: 64,
            percent: 12,
        },
    }
}

pub fn read_uptime_str() -> String {
    if let Ok(text) = fs::read_to_string("/proc/uptime")
        && let Some(secs_f) = text.split_whitespace().next().and_then(|s| s.parse::<f64>().ok())
    {
        let secs = secs_f as u64;
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        return format!("{h}h {m:02}m {s:02}s");
    }
    String::from("0h 04m 20s")
}

pub fn read_loadavg() -> String {
    if let Ok(text) = fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = text.split_whitespace().take(3).collect();
        if !parts.is_empty() {
            return parts.join(" ");
        }
    }
    String::from("0.08 0.03 0.01")
}

pub fn read_kernel_version() -> String {
    if let Ok(text) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return format!("Linux {trimmed}");
        }
    }
    String::from("Linux 6.12-pizdOS x86_64")
}

#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub state: char,
    pub mem_kb: u64,
}

pub fn list_processes() -> Vec<ProcInfo> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return vec![
            ProcInfo {
                pid: 1,
                name: String::from("init"),
                state: 'S',
                mem_kb: 1024,
            },
            ProcInfo {
                pid: 42,
                name: String::from("sh"),
                state: 'S',
                mem_kb: 2048,
            },
            ProcInfo {
                pid: 69,
                name: String::from("pizsoft"),
                state: 'R',
                mem_kb: 1536,
            },
        ];
    };

    let mut procs = Vec::new();
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let Ok(pid) = fname.to_string_lossy().parse::<u32>() else {
            continue;
        };
        let pdir = entry.path();
        let name = fs::read_to_string(pdir.join("comm"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| String::from("?"));
        let state = fs::read_to_string(pdir.join("stat"))
            .ok()
            .and_then(|s| {
                s.rfind(')')
                    .and_then(|idx| s[idx + 1..].trim_start().chars().next())
            })
            .unwrap_or('?');
        let mem_kb = fs::read_to_string(pdir.join("statm"))
            .ok()
            .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
            .map(|pages| pages * 4)
            .unwrap_or(0);

        procs.push(ProcInfo {
            pid,
            name,
            state,
            mem_kb,
        });
    }

    procs.sort_by_key(|p| p.pid);
    procs
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkgSummary {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub desc: String,
    pub installed: bool,
}

pub fn parse_pkg_blocks(text: &str, installed: bool) -> Vec<PkgSummary> {
    let mut list = Vec::new();
    for block in text.split("\n---\n") {
        let block = block.trim().trim_matches('-').trim();
        if block.is_empty() {
            continue;
        }
        let mut name = String::new();
        let mut version = String::new();
        let mut arch = String::from("x86_64");
        let mut desc = String::new();

        for line in block.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "name" => name = v.trim().to_string(),
                "version" => version = v.trim().to_string(),
                "arch" => arch = v.trim().to_string(),
                "desc" => desc = v.trim().to_string(),
                _ => {}
            }
        }

        if !name.is_empty() {
            list.push(PkgSummary {
                name,
                version,
                arch,
                desc,
                installed,
            });
        }
    }
    list
}

pub fn load_packages(root: &Path) -> Vec<PkgSummary> {
    let mut map: BTreeMap<String, PkgSummary> = BTreeMap::new();

    let idx_path = root.join("var/lib/piz-get/index.db");
    if let Ok(text) = fs::read_to_string(idx_path) {
        for pkg in parse_pkg_blocks(&text, false) {
            map.insert(pkg.name.clone(), pkg);
        }
    }

    let inst_path = root.join("var/lib/piz-get/installed.db");
    if let Ok(text) = fs::read_to_string(inst_path) {
        for pkg in parse_pkg_blocks(&text, true) {
            map.insert(pkg.name.clone(), pkg);
        }
    }

    map.into_values().collect()
}
