use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const MAGIC: &[u8; 4] = b"PIZ1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub desc: String,
    pub depends: Vec<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> io::Result<Self> {
        let mut name = None;
        let mut version = None;
        let mut arch = String::from("x86_64");
        let mut desc = String::new();
        let mut depends = Vec::new();

        for (line_no, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("manifest line {}: expected key=value", line_no + 1),
                ));
            };
            let key = key.trim();
            let val = val.trim();
            match key {
                "name" => name = Some(val.to_string()),
                "version" => version = Some(val.to_string()),
                "arch" => arch = val.to_string(),
                "desc" => desc = val.to_string(),
                "depends" => {
                    depends = val
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                }
                _ => {}
            }
        }

        let name = name.filter(|s| !s.is_empty()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "manifest missing 'name'")
        })?;
        let version = version.filter(|s| !s.is_empty()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "manifest missing 'version'")
        })?;

        Ok(Self {
            name,
            version,
            arch,
            desc,
            depends,
        })
    }

    pub fn serialize(&self) -> String {
        let deps = self.depends.join(", ");
        format!(
            "name={}\nversion={}\narch={}\ndesc={}\ndepends={}\n",
            self.name, self.version, self.arch, self.desc, deps
        )
    }
}

fn validate_rel_path(rel: &str) -> io::Result<PathBuf> {
    if rel.is_empty() || rel.contains('\0') || rel.starts_with('/') || rel.starts_with('\\') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsafe package path: {rel:?}"),
        ));
    }
    let normalized = rel.replace('\\', "/");
    let mut out = PathBuf::new();
    for part in Path::new(&normalized).components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsafe package path component in {rel:?}"),
                ));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty package path",
        ));
    }
    Ok(out)
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(base, &path, out)?;
        } else if file_type.is_file() {
            let rel = path.strip_prefix(base).map_err(io::Error::other)?;
            if rel == Path::new("piz.manifest") {
                continue;
            }
            let rel_str = rel
                .iter()
                .map(|c| c.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel_str, path));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn file_mode(path: &Path) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> io::Result<u32> {
    Ok(0o755)
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

pub fn pack_dir(src_dir: &Path, out_path: &Path) -> io::Result<Manifest> {
    let manifest_path = src_dir.join("piz.manifest");
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("failed to read {}: {e}", manifest_path.display()),
        )
    })?;
    let manifest = Manifest::parse(&manifest_text)?;
    let serialized = manifest.serialize();
    let manifest_bytes = serialized.as_bytes();

    let mut files = Vec::new();
    collect_files(src_dir, src_dir, &mut files)?;

    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let mut writer = BufWriter::new(File::create(out_path)?);
    writer.write_all(MAGIC)?;
    writer.write_all(&(manifest_bytes.len() as u32).to_le_bytes())?;
    writer.write_all(manifest_bytes)?;
    writer.write_all(&(files.len() as u32).to_le_bytes())?;

    for (rel_str, abs_path) in files {
        let rel_bytes = rel_str.as_bytes();
        let rel_len = u16::try_from(rel_bytes.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "file path too long")
        })?;
        let mode = file_mode(&abs_path)?;
        let data = fs::read(&abs_path)?;

        writer.write_all(&rel_len.to_le_bytes())?;
        writer.write_all(rel_bytes)?;
        writer.write_all(&mode.to_le_bytes())?;
        writer.write_all(&(data.len() as u64).to_le_bytes())?;
        writer.write_all(&data)?;
    }

    writer.flush()?;
    Ok(manifest)
}

fn read_u16_le<R: Read>(r: &mut R) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32_le<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64_le<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_header<R: Read>(reader: &mut R) -> io::Result<(Manifest, u32)> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a valid .piz package (bad magic)",
        ));
    }

    let manifest_len = read_u32_le(reader)? as usize;
    if manifest_len > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "manifest too large",
        ));
    }
    let mut manifest_buf = vec![0u8; manifest_len];
    reader.read_exact(&mut manifest_buf)?;
    let manifest_str = String::from_utf8(manifest_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let manifest = Manifest::parse(&manifest_str)?;
    let file_count = read_u32_le(reader)?;
    Ok((manifest, file_count))
}

pub fn read_pkg_manifest(pkg_path: &Path) -> io::Result<Manifest> {
    let mut reader = BufReader::new(File::open(pkg_path)?);
    let (manifest, _) = read_header(&mut reader)?;
    Ok(manifest)
}

pub fn extract_pkg(pkg_path: &Path, root: &Path) -> io::Result<(Manifest, Vec<String>)> {
    let mut reader = BufReader::new(File::open(pkg_path)?);
    let (manifest, file_count) = read_header(&mut reader)?;

    let mut installed_paths = Vec::with_capacity(file_count as usize);
    let mut buf = [0u8; 16384];

    for _ in 0..file_count {
        let path_len = read_u16_le(&mut reader)? as usize;
        let mut path_bytes = vec![0u8; path_len];
        reader.read_exact(&mut path_bytes)?;
        let rel_str = String::from_utf8(path_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let rel_path = validate_rel_path(&rel_str)?;

        let mode = read_u32_le(&mut reader)?;
        let mut remaining = read_u64_le(&mut reader)?;

        let dest = root.join(&rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut out = BufWriter::new(File::create(&dest)?);
        while remaining > 0 {
            let to_read = (remaining as usize).min(buf.len());
            reader.read_exact(&mut buf[..to_read])?;
            out.write_all(&buf[..to_read])?;
            remaining -= to_read as u64;
        }
        out.flush()?;
        apply_mode(&dest, mode)?;

        installed_paths.push(rel_str.replace('\\', "/"));
    }

    Ok((manifest, installed_paths))
}
