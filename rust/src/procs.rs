//! Same-user process snapshot used only as routing metadata (`everything/processes.py`).

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;

#[derive(Clone)]
pub struct Proc {
    pub pid: i64,
    pub ppid: i64,
    pub start_time: String,
    pub comm: String,
    pub argv: Vec<String>,
}

#[derive(Default, Clone)]
pub struct ProcTable {
    pub procs: HashMap<i64, Proc>,
}

impl ProcTable {
    pub fn read() -> Self {
        let uid = current_uid();
        let mut procs = HashMap::new();
        let Ok(entries) = std::fs::read_dir("/proc") else { return ProcTable { procs } };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.parse::<i64>().ok()) else { continue };
            let base = entry.path();
            match std::fs::metadata(&base) {
                Ok(meta) if meta.uid() == uid => {}
                _ => continue,
            }
            let Ok(stat) = std::fs::read_to_string(base.join("stat")) else { continue };
            let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else { continue };
            let comm = stat[open + 1..close].to_string();
            let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
            let (Some(ppid), Some(start_time)) = (fields.get(1).and_then(|f| f.parse().ok()), fields.get(19)) else {
                continue;
            };
            let start_time = start_time.to_string();
            let Ok(raw) = std::fs::read(base.join("cmdline")) else { continue };
            let argv = raw
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect();
            procs.insert(pid, Proc { pid, ppid, start_time, comm, argv });
        }
        ProcTable { procs }
    }

    pub fn ancestors(&self, pid: i64) -> Vec<&Proc> {
        let mut out: Vec<&Proc> = Vec::new();
        let mut current = self.procs.get(&pid);
        while let Some(process) = current {
            if out.len() >= 128 || out.iter().any(|seen| seen.pid == process.pid) {
                break;
            }
            out.push(process);
            current = self.procs.get(&process.ppid);
        }
        out
    }

    pub fn ancestor_pid_in(&self, pid: i64, candidates: &[i64]) -> Option<i64> {
        self.ancestors(pid).into_iter().map(|p| p.pid).find(|p| candidates.contains(p))
    }

    pub fn named(&self, name: &str) -> Vec<&Proc> {
        self.procs
            .values()
            .filter(|p| {
                p.comm.eq_ignore_ascii_case(name)
                    || p.argv.first().map(|a| basename(a).eq_ignore_ascii_case(name)).unwrap_or(false)
            })
            .collect()
    }
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn canonical_path(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let expanded = match (value.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => value.to_string(),
    };
    std::fs::canonicalize(&expanded)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or(expanded)
}

pub fn current_uid() -> u32 {
    // /proc/self is owned by the effective uid; avoids a libc dependency.
    std::fs::metadata("/proc/self").map(|meta| meta.uid()).unwrap_or(0)
}

impl ProcTable {
    pub fn get(&self, pid: i64) -> Option<&Proc> {
        self.procs.get(&pid)
    }

    pub fn is_descendant(&self, pid: i64, ancestor: i64) -> bool {
        self.ancestors(pid).iter().any(|p| p.pid == ancestor)
    }

    /// Only the requested variables of one same-user process, bounded in size.
    pub fn environment(&self, pid: i64, names: &[&str]) -> HashMap<String, String> {
        use std::io::Read;
        let mut values = HashMap::new();
        if self.get(pid).is_none() {
            return values;
        }
        let Ok(file) = std::fs::File::open(format!("/proc/{pid}/environ")) else { return values };
        let mut raw = Vec::new();
        if file.take(256 * 1024 + 1).read_to_end(&mut raw).is_err() || raw.len() > 256 * 1024 {
            return values;
        }
        for entry in raw.split(|b| *b == 0) {
            let text = String::from_utf8_lossy(entry);
            if let Some((key, value)) = text.split_once('=') {
                if names.contains(&key) {
                    values.insert(key.to_string(), value.to_string());
                }
            }
        }
        values
    }
}

pub fn proc_cwd(pid: i64) -> String {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn username() -> String {
    let uid = current_uid().to_string();
    std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|data| {
            data.lines().find_map(|line| {
                let f: Vec<&str> = line.split(':').collect();
                (f.len() > 2 && f[2] == uid).then(|| f[0].to_string())
            })
        })
        .unwrap_or(uid)
}
