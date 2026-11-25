//! Surveys the translator over containers recorded from a title run, so the
//! shader work can be done without starting the title.
//!
//! The runtime records every container the title hands `d3d12_xs`; the same
//! bytes are what the translator refuses or accepts in game, so a corpus of
//! them answers "how many refuses, and why" in seconds, on the host, with no
//! Wine, no GPU and no title.
//!
//! ```sh
//! cargo +stable run --release -p victory-game --example corpus -- \
//!     ~/.local/share/project-tuxbox/prefixes/*/drive_c/users/*/AppData/Local/Temp/tuxbox_shader_cache/projectscarlett/*/requests \
//!     --dump /tmp/refused
//! ```
//!
//! Inputs may be recorded `.dxbc` requests (one container each) or the title's
//! archives and shader files, whose containers are packed in `SGD5` records.
//! `--dump <dir>` writes each refused container there, with `refused.txt`
//! listing hash, size and reason, which is what a fix is written against.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use victory_shader::emit;
use victory_shader::HwStage;

fn main() {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut dump: Option<PathBuf> = None;
    let mut limit = usize::MAX;
    let mut threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dump" => dump = args.next().map(PathBuf::from),
            "--limit" => limit = args.next().and_then(|n| n.parse().ok()).unwrap_or(limit),
            "--jobs" => threads = args.next().and_then(|n| n.parse().ok()).unwrap_or(threads).max(1),
            "--why" => {
                why(&PathBuf::from(args.next().unwrap_or_default()));
                return;
            }
            "-h" | "--help" => {
                println!("usage: corpus <file-or-directory>… [--dump DIR] [--limit N] [--jobs N]");
                return;
            }
            other => paths.push(PathBuf::from(other)),
        }
    }
    if paths.is_empty() {
        eprintln!("usage: corpus <file-or-directory>… [--dump DIR] [--limit N] [--jobs N]");
        std::process::exit(2);
    }
    if let Some(dir) = &dump {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("cannot create {}: {e}", dir.display());
            std::process::exit(2);
        }
    }

    let files = walk(&paths);
    let survey = Survey {
        seen: Mutex::new(Default::default()),
        translated: AtomicUsize::new(0),
        stages: Mutex::new(BTreeMap::new()),
        refused: Mutex::new(BTreeMap::new()),
        duplicates: AtomicUsize::new(0),
        containers: AtomicUsize::new(0),
        next: AtomicUsize::new(0),
        dump: dump.clone(),
    };
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| survey.run(&files, limit));
        }
    });

    let containers = survey.containers.load(Ordering::Relaxed);
    let duplicates = survey.duplicates.load(Ordering::Relaxed);
    let translated = survey.translated.load(Ordering::Relaxed);
    let stages = survey.stages.lock().unwrap_or_else(|e| e.into_inner());
    let refused = survey.refused.lock().unwrap_or_else(|e| e.into_inner());
    let refused_total: usize = refused.values().map(|(n, _)| n).sum();
    println!(
        "{} files, {containers} containers ({duplicates} duplicates), {} distinct",
        files.len(),
        containers - duplicates
    );
    println!(
        "translated {translated}, refused {refused_total} in {} classes",
        refused.len()
    );
    let stages: Vec<_> = stages.iter().collect();
    if !stages.is_empty() {
        let line: Vec<String> = stages
            .iter()
            .map(|(stage, n)| format!("{} {n}", name(**stage)))
            .collect();
        println!("translated by stage: {}", line.join(", "));
    }
    let mut refused: Vec<_> = refused.iter().collect();
    refused.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (reason, (n, example)) in refused {
        println!("{n:>7}  {reason}\n         e.g. {example}");
    }
    if let Some(dir) = dump {
        println!("refused containers written to {}", dir.display());
    }
}

/// Why one container refuses: the program's layout and the code around the
/// refused instruction, so a fix can be written against the real shader.
fn why(path: &Path) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", path.display());
        std::process::exit(2);
    });
    let container = victory_shader::Container::parse(&bytes).expect("a DXBC container");
    let Some(dxil) = container.part(victory_shader::FourCC::DXIL) else {
        println!("no DXIL part");
        return;
    };
    let programs = victory_shader::native_programs(dxil.data).expect("native programs");
    let offset = match emit::translate(&bytes) {
        Ok(shader) => {
            println!("translates ({})", name(shader.stage));
            return;
        }
        Err(victory_shader::ShaderError::Unsupported { offset, what }) => {
            println!("refused: {what}");
            offset
        }
        Err(victory_shader::ShaderError::Isa { offset, reason }) => {
            println!("refused: {reason}");
            offset
        }
        Err(e) => {
            println!("refused: {e}");
            return;
        }
    };
    for program in &programs {
        println!(
            "{} {} (wave64 {}, {} VGPRs)",
            name(program.stage),
            if program.wave64 { 64 } else { 32 },
            program.wave64,
            program.vgprs
        );
        let code = program.patched_code();
        let instructions = victory_shader::gfx10::decode(&code).expect("decodable code");
        let at = instructions
            .iter()
            .position(|i| i.offset as usize >= offset)
            .unwrap_or(0);
        let from = if std::env::var("CORPUS_ALL").is_ok() { 0 } else { at.saturating_sub(24) };
        let to = if std::env::var("CORPUS_ALL").is_ok() { instructions.len() } else { (at + 8).min(instructions.len()) };
        for inst in &instructions[from..to] {
            let marker = if inst.offset as usize >= offset { "=>" } else { "  " };
            println!(
                "{marker} {:#06x} {} {:?}",
                inst.offset,
                inst.name().unwrap_or("?"),
                inst.fields(program.wave64)
            );
        }
        let mut roots: Vec<_> = program
            .layout
            .iter()
            .filter(|p| !matches!(p.root, victory_shader::RootType::ImmediateSampler))
            .collect();
        roots.sort_by_key(|p| p.user_data);
        roots.dedup_by_key(|p| p.user_data);
        for param in roots {
            println!(
                "  root slot {}: {:?} at user SGPR {}..{} (first register {})",
                param.slot,
                param.root,
                program.stage.user_sgpr(param.user_data),
                program.stage.user_sgpr(param.user_data) + param.sgprs as u8,
                param.base_register
            );
        }
    }
}

fn name(stage: HwStage) -> String {
    format!("{stage:?}").to_lowercase()
}

struct Survey {
    /// Containers already translated, by length and hash: the title records
    /// the same container once per request.
    seen: Mutex<std::collections::HashSet<(usize, u64)>>,
    translated: AtomicUsize,
    stages: Mutex<BTreeMap<HwStage, usize>>,
    /// Reason (offset dropped) → (count, first container).
    refused: Mutex<BTreeMap<String, (usize, String)>>,
    duplicates: AtomicUsize,
    containers: AtomicUsize,
    next: AtomicUsize,
    dump: Option<PathBuf>,
}

impl Survey {
    fn run(&self, files: &[PathBuf], limit: usize) {
        loop {
            let i = self.next.fetch_add(1, Ordering::Relaxed);
            let Some(path) = files.get(i) else { return };
            let Ok(bytes) = std::fs::read(path) else { continue };
            for container in containers(&bytes) {
                if self.containers.fetch_add(1, Ordering::Relaxed) >= limit {
                    return;
                }
                self.one(&container, path);
            }
        }
    }

    fn one(&self, container: &[u8], path: &Path) {
        let key = (container.len(), fnv(container));
        if !self.seen.lock().unwrap_or_else(|e| e.into_inner()).insert(key) {
            self.duplicates.fetch_add(1, Ordering::Relaxed);
            return;
        }
        match emit::translate(container) {
            Ok(shader) => {
                self.translated.fetch_add(1, Ordering::Relaxed);
                *self
                    .stages
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .entry(shader.stage)
                    .or_insert(0) += 1;
            }
            Err(error) => {
                // The offset moves with any change to the shader; the class
                // of refusal is what a fix targets.
                let text = error.to_string();
                let reason = match text.split_once(": ") {
                    Some((head, tail)) if head.starts_with("GFX10 code at ") || head == "malformed shader" => tail.to_string(),
                    _ => text.clone(),
                };
                let example = format!("{} ({} B)", path.display(), container.len());
                let mut refused = self.refused.lock().unwrap_or_else(|e| e.into_inner());
                let entry = refused.entry(reason).or_insert((0, example));
                entry.0 += 1;
                if let Some(dir) = &self.dump {
                    let at = dir.join(format!("{:016x}.dxbc", fnv(container)));
                    let _ = std::fs::write(at, container);
                    if let Ok(mut list) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(dir.join("refused.txt"))
                    {
                        use std::io::Write;
                        let _ = writeln!(list, "{:016x} {} B  {}", fnv(container), container.len(), error);
                    }
                }
            }
        }
    }
}

/// Every container in a file: recorded requests hold one; `SGD5` holders and
/// archives store them deflate-packed, one record each.
fn containers(file: &[u8]) -> Vec<Vec<u8>> {
    if file.starts_with(b"DXBC") {
        return vec![file.to_vec()];
    }
    if !victory_game::sgd5::is_sgd5(file) {
        return Vec::new();
    }
    victory_game::sgd5::records(file)
        .filter_map(|record| record.unpack())
        .collect()
}

fn walk(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() {
            if let Ok(entries) = std::fs::read_dir(input) {
                let mut children: Vec<_> = entries.flatten().map(|e| e.path()).collect();
                children.sort();
                files.extend(walk(&children));
            }
        } else if input.is_file() {
            files.push(input.clone());
        }
    }
    files
}

/// FNV-1a, to name dumped containers without a hash dependency.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}
