use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::pkg::read_pkg_manifest;
use crate::sha256::hash_file;

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub desc: String,
    pub depends: Vec<String>,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Default)]
pub struct IndexDb {
    pub entries: BTreeMap<String, IndexEntry>,
}

impl IndexDb {
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> io::Result<Self> {
        let mut entries = BTreeMap::new();
        for block in text.split("\n---\n") {
            let block = block.trim().trim_matches('-').trim();
            if block.is_empty() {
                continue;
            }
            let mut name = String::new();
            let mut version = String::new();
            let mut arch = String::from("x86_64");
            let mut desc = String::new();
            let mut depends = Vec::new();
            let mut filename = String::new();
            let mut size = 0u64;
            let mut sha256 = String::new();

            for line in block.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                let k = k.trim();
                let v = v.trim();
                match k {
                    "name" => name = v.to_string(),
                    "version" => version = v.to_string(),
                    "arch" => arch = v.to_string(),
                    "desc" => desc = v.to_string(),
                    "depends" => {
                        depends = v
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect();
                    }
                    "filename" => filename = v.to_string(),
                    "size" => size = v.parse().unwrap_or(0),
                    "sha256" => sha256 = v.to_string(),
                    _ => {}
                }
            }

            if !name.is_empty() && !version.is_empty() {
                entries.insert(
                    name.clone(),
                    IndexEntry {
                        name,
                        version,
                        arch,
                        desc,
                        depends,
                        filename,
                        size,
                        sha256,
                    },
                );
            }
        }
        Ok(Self { entries })
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for (idx, entry) in self.entries.values().enumerate() {
            if idx > 0 {
                out.push_str("---\n");
            }
            out.push_str(&format!(
                "name={}\nversion={}\narch={}\ndesc={}\ndepends={}\nfilename={}\nsize={}\nsha256={}\n",
                entry.name,
                entry.version,
                entry.arch,
                entry.desc,
                entry.depends.join(", "),
                entry.filename,
                entry.size,
                entry.sha256,
            ));
        }
        out
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.serialize())
    }

    pub fn generate_from_dir(repo_dir: &Path) -> io::Result<Self> {
        let mut entries = BTreeMap::new();
        for item in fs::read_dir(repo_dir)? {
            let item = item?;
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("piz") {
                continue;
            }
            let manifest = read_pkg_manifest(&path)?;
            let size = fs::metadata(&path)?.len();
            let sha256 = hash_file(&path)?;
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            entries.insert(
                manifest.name.clone(),
                IndexEntry {
                    name: manifest.name,
                    version: manifest.version,
                    arch: manifest.arch,
                    desc: manifest.desc,
                    depends: manifest.depends,
                    filename,
                    size,
                    sha256,
                },
            );
        }
        Ok(Self { entries })
    }
}

#[derive(Debug, Clone)]
pub struct InstalledPkg {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub desc: String,
    pub depends: Vec<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Default)]
pub struct InstalledDb {
    pub packages: BTreeMap<String, InstalledPkg>,
}

impl InstalledDb {
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        let mut packages = BTreeMap::new();

        for block in text.split("\n---\n") {
            let block = block.trim().trim_matches('-').trim();
            if block.is_empty() {
                continue;
            }
            let mut name = String::new();
            let mut version = String::new();
            let mut arch = String::from("x86_64");
            let mut desc = String::new();
            let mut depends = Vec::new();
            let mut files = Vec::new();

            for line in block.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                let k = k.trim();
                let v = v.trim();
                match k {
                    "name" => name = v.to_string(),
                    "version" => version = v.to_string(),
                    "arch" => arch = v.to_string(),
                    "desc" => desc = v.to_string(),
                    "depends" => {
                        depends = v
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect();
                    }
                    "file" if !v.is_empty() => {
                        files.push(v.to_string());
                    }
                    _ => {}
                }
            }

            if !name.is_empty() {
                packages.insert(
                    name.clone(),
                    InstalledPkg {
                        name,
                        version,
                        arch,
                        desc,
                        depends,
                        files,
                    },
                );
            }
        }

        Ok(Self { packages })
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = String::new();
        for (idx, pkg) in self.packages.values().enumerate() {
            if idx > 0 {
                out.push_str("---\n");
            }
            out.push_str(&format!(
                "name={}\nversion={}\narch={}\ndesc={}\ndepends={}\n",
                pkg.name,
                pkg.version,
                pkg.arch,
                pkg.desc,
                pkg.depends.join(", "),
            ));
            for f in &pkg.files {
                out.push_str(&format!("file={f}\n"));
            }
        }
        fs::write(path, out)
    }

    pub fn dependents_of(&self, target: &str) -> Vec<String> {
        self.packages
            .values()
            .filter(|p| p.name != target && p.depends.iter().any(|d| d == target))
            .map(|p| p.name.clone())
            .collect()
    }
}
