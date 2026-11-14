mod db;
mod net;
mod pkg;
mod sha256;

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use db::{IndexDb, InstalledDb, InstalledPkg};
use net::fetch_to_file;
use pkg::{extract_pkg, pack_dir};
use sha256::hash_file;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_REPO: &str = "http://repo.pizdos.local/x86_64";

struct Config {
    root: PathBuf,
    repo: String,
    force: bool,
    available: bool,
}

impl Config {
    fn index_path(&self) -> PathBuf {
        self.root.join("var/lib/piz-get/index.db")
    }

    fn installed_path(&self) -> PathBuf {
        self.root.join("var/lib/piz-get/installed.db")
    }

    fn cache_dir(&self) -> PathBuf {
        self.root.join("var/cache/piz-get")
    }
}

fn print_help() {
    println!(
        "piz-get {VERSION} — zero-dependency package manager for pizdOS 2.0\n\
         \n\
         USAGE:\n\
           piz-get [OPTIONS] <COMMAND> [ARGS...]\n\
         \n\
         OPTIONS:\n\
           --root <DIR>       Target root directory (default: / or $PIZ_ROOT)\n\
           --repo <URL|DIR>   Repository URL or local path (default: $PIZ_REPO)\n\
           -f, --force        Force install/remove ignoring conflicts or deps\n\
           -a, --available    Show available packages in 'list'\n\
           -h, --help         Show this help\n\
           -V, --version      Show version\n\
         \n\
         COMMANDS (Eng / Po-pacanski):\n\
           update  | obnovi             Fetch package index from repository\n\
           install | stav | ebash <pkg> Install package(s) and dependencies\n\
           remove  | snosi | nahuy <pkg> Remove installed package(s)\n\
           upgrade | apgreyd            Upgrade all installed packages\n\
           list    | spisok             List installed (or -a available) packages\n\
           search  | ishi <query>       Search packages in index\n\
           info    | poyasni <pkg>      Show package details\n\
           pack    | pakuy <dir> [out]  Pack directory with piz.manifest into .piz\n\
           index   <dir>                Generate index.db from .piz files in <dir>\n\
           help    | huelp              Show this help"
    );
}

fn resolve_deps(
    target: &str,
    index: &IndexDb,
    installed: &InstalledDb,
    force_target: bool,
    visiting: &mut HashSet<String>,
    seen: &mut HashSet<String>,
    order: &mut Vec<String>,
) -> io::Result<()> {
    if seen.contains(target) {
        return Ok(());
    }
    if !force_target && installed.packages.contains_key(target) {
        seen.insert(target.to_string());
        return Ok(());
    }
    if !visiting.insert(target.to_string()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("dependency cycle detected involving '{target}'"),
        ));
    }

    let entry = index.entries.get(target).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("package '{target}' not found in index (run 'piz-get update'?)"),
        )
    })?;

    for dep in &entry.depends {
        resolve_deps(dep, index, installed, false, visiting, seen, order)?;
    }

    visiting.remove(target);
    seen.insert(target.to_string());
    order.push(target.to_string());
    Ok(())
}

fn cmd_update(cfg: &Config) -> io::Result<()> {
    let idx_path = cfg.index_path();
    println!("Updating index from {}...", cfg.repo);
    fetch_to_file(&cfg.repo, "index.db", &idx_path)?;
    let db = IndexDb::load(&idx_path)?;
    println!("Index updated: {} package(s) available.", db.entries.len());
    Ok(())
}

fn cmd_install(cfg: &Config, targets: &[String]) -> io::Result<()> {
    if targets.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "specify at least one package to install",
        ));
    }

    let index = IndexDb::load(&cfg.index_path())?;
    let mut installed = InstalledDb::load(&cfg.installed_path())?;
    let mut visiting = HashSet::new();
    let mut seen = HashSet::new();
    let mut order = Vec::new();

    for t in targets {
        resolve_deps(
            t,
            &index,
            &installed,
            cfg.force,
            &mut visiting,
            &mut seen,
            &mut order,
        )?;
    }

    if order.is_empty() {
        println!("Everything is already installed.");
        return Ok(());
    }

    let cache_dir = cfg.cache_dir();
    fs::create_dir_all(&cache_dir)?;

    for pkg_name in &order {
        let entry = &index.entries[pkg_name];
        let pkg_file = cache_dir.join(&entry.filename);

        println!("Fetching {} {}...", entry.name, entry.version);
        fetch_to_file(&cfg.repo, &entry.filename, &pkg_file)?;

        if !entry.sha256.is_empty() {
            let actual = hash_file(&pkg_file)?;
            if actual != entry.sha256 {
                let _ = fs::remove_file(&pkg_file);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "SHA-256 mismatch for {}: expected {}, got {}",
                        entry.name, entry.sha256, actual
                    ),
                ));
            }
        }

        println!("Installing {} {}...", entry.name, entry.version);
        let (manifest, files) = extract_pkg(&pkg_file, &cfg.root)?;
        installed.packages.insert(
            manifest.name.clone(),
            InstalledPkg {
                name: manifest.name,
                version: manifest.version,
                arch: manifest.arch,
                desc: manifest.desc,
                depends: manifest.depends,
                files,
            },
        );
    }

    installed.save(&cfg.installed_path())?;
    println!("Done. Installed {} package(s).", order.len());
    Ok(())
}

fn cleanup_empty_parents(root: &Path, file_path: &Path) {
    let mut cur = file_path.parent();
    while let Some(dir) = cur {
        if dir == root {
            break;
        }
        if fs::remove_dir(dir).is_err() {
            break;
        }
        cur = dir.parent();
    }
}

fn cmd_remove(cfg: &Config, targets: &[String]) -> io::Result<()> {
    if targets.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "specify at least one package to remove",
        ));
    }

    let mut installed = InstalledDb::load(&cfg.installed_path())?;

    for t in targets {
        if !installed.packages.contains_key(t) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("package '{t}' is not installed"),
            ));
        }

        if !cfg.force {
            let deps = installed.dependents_of(t);
            let remaining_deps: Vec<_> = deps
                .into_iter()
                .filter(|d| !targets.contains(d))
                .collect();
            if !remaining_deps.is_empty() {
                return Err(io::Error::other(format!(
                    "cannot remove '{t}': required by {} (use --force to override)",
                    remaining_deps.join(", ")
                )));
            }
        }
    }

    for t in targets {
        let Some(pkg) = installed.packages.remove(t) else {
            continue;
        };
        println!("Removing {} {}...", pkg.name, pkg.version);
        for rel in &pkg.files {
            let abs = cfg.root.join(rel);
            if abs.exists() {
                let _ = fs::remove_file(&abs);
                cleanup_empty_parents(&cfg.root, &abs);
            }
        }
    }

    installed.save(&cfg.installed_path())?;
    println!("Done.");
    Ok(())
}

fn cmd_upgrade(cfg: &Config) -> io::Result<()> {
    let index = IndexDb::load(&cfg.index_path())?;
    let installed = InstalledDb::load(&cfg.installed_path())?;

    let mut to_upgrade = Vec::new();
    for (name, inst) in &installed.packages {
        if let Some(avail) = index.entries.get(name)
            && avail.version != inst.version
        {
            println!("Upgrade: {} {} -> {}", name, inst.version, avail.version);
            to_upgrade.push(name.clone());
        }
    }

    if to_upgrade.is_empty() {
        println!("All packages are up to date.");
        return Ok(());
    }

    let force_cfg = Config {
        root: cfg.root.clone(),
        repo: cfg.repo.clone(),
        force: true,
        available: cfg.available,
    };
    cmd_install(&force_cfg, &to_upgrade)
}

fn cmd_list(cfg: &Config) -> io::Result<()> {
    if cfg.available {
        let index = IndexDb::load(&cfg.index_path())?;
        if index.entries.is_empty() {
            println!("Index is empty. Run 'piz-get update' first.");
            return Ok(());
        }
        for e in index.entries.values() {
            println!("{:<18} {:<10} [{}] {}", e.name, e.version, e.arch, e.desc);
        }
    } else {
        let installed = InstalledDb::load(&cfg.installed_path())?;
        if installed.packages.is_empty() {
            println!("No packages installed.");
            return Ok(());
        }
        for p in installed.packages.values() {
            println!("{:<18} {:<10} [{}] {}", p.name, p.version, p.arch, p.desc);
        }
    }
    Ok(())
}

fn cmd_search(cfg: &Config, query: Option<&str>) -> io::Result<()> {
    let q = query.unwrap_or("").to_ascii_lowercase();
    let index = IndexDb::load(&cfg.index_path())?;
    let mut found = 0usize;
    for e in index.entries.values() {
        if e.name.to_ascii_lowercase().contains(&q) || e.desc.to_ascii_lowercase().contains(&q) {
            println!("{:<18} {:<10} {}", e.name, e.version, e.desc);
            found += 1;
        }
    }
    if found == 0 {
        println!("No matching packages found.");
    }
    Ok(())
}

fn cmd_info(cfg: &Config, name: Option<&str>) -> io::Result<()> {
    let Some(name) = name else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "specify a package name",
        ));
    };

    let installed = InstalledDb::load(&cfg.installed_path())?;
    let index = IndexDb::load(&cfg.index_path())?;

    let mut printed = false;
    if let Some(p) = installed.packages.get(name) {
        printed = true;
        println!("Status:      installed");
        println!("Name:        {}", p.name);
        println!("Version:     {}", p.version);
        println!("Arch:        {}", p.arch);
        println!("Description: {}", p.desc);
        println!("Depends:     {}", p.depends.join(", "));
        println!("Files:       {}", p.files.len());
    }
    if let Some(e) = index.entries.get(name) {
        if printed {
            println!("---");
        }
        printed = true;
        println!("Status:      available in repo");
        println!("Name:        {}", e.name);
        println!("Version:     {}", e.version);
        println!("Arch:        {}", e.arch);
        println!("Description: {}", e.desc);
        println!("Depends:     {}", e.depends.join(", "));
        println!("Filename:    {}", e.filename);
        println!("Size:        {} bytes", e.size);
        println!("SHA-256:     {}", e.sha256);
    }

    if !printed {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("package '{name}' not found"),
        ));
    }
    Ok(())
}

fn cmd_pack(args: &[String]) -> io::Result<()> {
    let Some(dir) = args.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: piz-get pack <dir> [out.piz]",
        ));
    };
    let src_dir = Path::new(dir);
    let manifest_text = fs::read_to_string(src_dir.join("piz.manifest"))?;
    let manifest = pkg::Manifest::parse(&manifest_text)?;

    let out_path = if let Some(out) = args.get(1) {
        PathBuf::from(out)
    } else {
        PathBuf::from(format!(
            "{}-{}-{}.piz",
            manifest.name, manifest.version, manifest.arch
        ))
    };

    let packed = pack_dir(src_dir, &out_path)?;
    let sum = hash_file(&out_path)?;
    println!(
        "Packed {} {} -> {} (sha256: {})",
        packed.name,
        packed.version,
        out_path.display(),
        sum
    );
    Ok(())
}

fn cmd_index(args: &[String]) -> io::Result<()> {
    let Some(dir) = args.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: piz-get index <repo_dir>",
        ));
    };
    let repo_dir = Path::new(dir);
    let db = IndexDb::generate_from_dir(repo_dir)?;
    let out_path = repo_dir.join("index.db");
    db.save(&out_path)?;
    println!(
        "Generated {} ({} package(s)).",
        out_path.display(),
        db.entries.len()
    );
    Ok(())
}

fn run() -> io::Result<()> {
    let mut root = env::var("PIZ_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"));
    let mut repo = env::var("PIZ_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let mut force = false;
    let mut available = false;

    let mut positional = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                let val = args.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--root requires a path")
                })?;
                root = PathBuf::from(val);
            }
            "--repo" => {
                let val = args.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--repo requires a URL or path")
                })?;
                repo = val;
            }
            "-f" | "--force" => force = true,
            "-a" | "--available" => available = true,
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-V" | "--version" => {
                println!("piz-get {VERSION}");
                return Ok(());
            }
            _ => positional.push(arg),
        }
    }

    let Some(cmd) = positional.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    let rest = &positional[1..];
    let cfg = Config {
        root,
        repo,
        force,
        available,
    };

    match cmd {
        "update" | "obnovi" => cmd_update(&cfg),
        "install" | "stav" | "ebash" => cmd_install(&cfg, rest),
        "remove" | "snosi" | "nahuy" => cmd_remove(&cfg, rest),
        "upgrade" | "apgreyd" => cmd_upgrade(&cfg),
        "list" | "spisok" => cmd_list(&cfg),
        "search" | "ishi" => cmd_search(&cfg, rest.first().map(String::as_str)),
        "info" | "poyasni" => cmd_info(&cfg, rest.first().map(String::as_str)),
        "pack" | "pakuy" => cmd_pack(rest),
        "index" => cmd_index(rest),
        "help" | "huelp" => {
            print_help();
            Ok(())
        }
        "version" => {
            println!("piz-get {VERSION}");
            Ok(())
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown command '{other}' (try 'piz-get help')"),
        )),
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("piz-get error: {err}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("piz_get_test_{label}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn end_to_end_pack_index_install_remove() {
        let base = temp_dir("e2e");
        let repo_dir = base.join("repo");
        let root_dir = base.join("rootfs");
        let src_lib = base.join("src_lib");
        let src_app = base.join("src_app");
        fs::create_dir_all(src_lib.join("usr/lib")).unwrap();
        fs::create_dir_all(src_app.join("usr/bin")).unwrap();
        fs::create_dir_all(&repo_dir).unwrap();
        fs::create_dir_all(&root_dir).unwrap();

        fs::write(
            src_lib.join("piz.manifest"),
            "name=libbase\nversion=1.0.0\narch=x86_64\ndesc=Base lib\ndepends=\n",
        )
        .unwrap();
        fs::write(src_lib.join("usr/lib/base.txt"), b"hello lib").unwrap();

        fs::write(
            src_app.join("piz.manifest"),
            "name=pizfetch\nversion=0.1.0\narch=x86_64\ndesc=System info\ndepends=libbase\n",
        )
        .unwrap();
        fs::write(src_app.join("usr/bin/pizfetch"), b"#!/bin/sh\necho pizdOS\n").unwrap();

        pack_dir(&src_lib, &repo_dir.join("libbase-1.0.0-x86_64.piz")).unwrap();
        pack_dir(&src_app, &repo_dir.join("pizfetch-0.1.0-x86_64.piz")).unwrap();

        let idx = IndexDb::generate_from_dir(&repo_dir).unwrap();
        idx.save(&repo_dir.join("index.db")).unwrap();

        let cfg = Config {
            root: root_dir.clone(),
            repo: repo_dir.to_string_lossy().to_string(),
            force: false,
            available: false,
        };

        cmd_update(&cfg).unwrap();
        cmd_install(&cfg, &[String::from("pizfetch")]).unwrap();

        assert!(root_dir.join("usr/lib/base.txt").exists());
        assert!(root_dir.join("usr/bin/pizfetch").exists());

        // Removing libbase without --force must fail because pizfetch depends on it
        assert!(cmd_remove(&cfg, &[String::from("libbase")]).is_err());

        cmd_remove(&cfg, &[String::from("pizfetch"), String::from("libbase")]).unwrap();
        assert!(!root_dir.join("usr/bin/pizfetch").exists());
        assert!(!root_dir.join("usr/lib/base.txt").exists());

        let _ = fs::remove_dir_all(&base);
    }
}
