//! What a release's YM-builder carries: everything a player's PC builds the game with. The sources the compile
//! reads (host/, gpu/, the third-party code and assets the build reads, the release's name), the prebuilt third-party
//! objects, and the build tools, embedded in the program as one pack (`YM-builder dev release sources` makes it,
//! with [`write`] here; the YM-builder crate's build.rs embeds the file `YM_SOURCES` names). A developer's build carries none and
//! builds from a checkout (`--root`, `YM_ROOT`, or the checkout the program is in).
//!
//! The pack is made of members, each a zstd frame of its own, so that a release whose tools (or most of whose sources)
//! did not change has the same bytes for those members, and a delta from the last release (`zstd --patch-from`) is
//! small:
//! - `tree/<part>`: the build's tree ([`TREE`] as tracked, but [`LEFT_OUT`]; its parts by [`group`]).
//! - `tools/<name>`: a build tool, at `tools/<name>/` in the tree ([`tool_dir`]). Each is unpacked once into
//!   `<cache>/tools/<name>-<its hash>/` and hard-linked from there into the tree, so the next release that carries the
//!   same tool unpacks nothing of it.
//!
//! The tree is `<cache>/sources/<the members' hash>/`, the build's root ([`crate::layout::tree_root`]).
//!
//! Each folder is unpacked once, its members' and every file's SHA-256 checked, into a folder of its own, put in place
//! whole (a rename) once it is complete, its [`COMPLETE`] note written last. A command that finds its folders there
//! reads the pack's table alone (the tree's hash is of the members' recorded hashes): nothing of the pack's hundreds
//! of MB is hashed again. Each use marks its folders ([`USED`]): the cache's budget (crate::storage) lets the least
//! recently used go, never while a build runs.
//!
//! ```text
//! the pack:    PACK_MAGIC, u32 members (LE), then for each: u16 name length, name, u64 offset, u64 length (from the
//!              pack's start, LE), 32 bytes SHA-256 of the member's bytes; then the members' bytes
//! a member:    one zstd frame of: MAGIC, then for each file: u16 path length (LE), path (UTF-8, / separated,
//!              relative), u8 mode (0 plain, 1 executable, 2 a relative symbolic link inside the member, its target
//!              the bytes; Unix only), u64 size (LE), 32 bytes SHA-256, the bytes; then u16 0,
//!              u64 the number of files (LE)
//! ```
//!
//! This file is the format's one writer ([`write`], [`tree_entries`], [`tool_entries`]) and its one reader ([`unpack`],
//! [`unpack_into`]). The writer compresses each member with the zstd program at [`MEMBER_ZSTD`] (one thread, the
//! frame's checksum), so the same files give the same bytes, and a release's members that did not change keep theirs.

use build_core::{Error, IoContext, Result, fsx};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The pack's first bytes, its format's version in them.
pub const PACK_MAGIC: &[u8; 8] = b"LSPAK02\n";
/// A member's first bytes (after its zstd frame is undone).
pub const MAGIC: &[u8; 8] = b"LSSRC01\n";
/// In an unpacked folder: written last, the hash it was unpacked from and its files.
pub const COMPLETE: &str = ".complete";
/// In an unpacked folder: touched by each use (its time is the folder's last use).
pub const USED: &str = ".used";

static EMBEDDED: OnceLock<&'static [u8]> = OnceLock::new();
static TOOLS: Mutex<BTreeMap<String, PathBuf>> = Mutex::new(BTreeMap::new());

/// The pack the program carries (the YM-builder binary sets it first thing; a developer's build, nothing).
pub fn set_embedded(pack: &'static [u8]) {
    let _ = EMBEDDED.set(pack);
}

/// The pack the program carries; empty when it carries none.
pub fn embedded() -> &'static [u8] {
    EMBEDDED.get().copied().unwrap_or(&[])
}

/// The folder a carried tool was unpacked into by the last [`unpack`], by its member's name (`tools/<name>`).
pub fn tool_dir(name: &str) -> Option<PathBuf> {
    TOOLS.lock().unwrap_or_else(|p| p.into_inner()).get(name).cloned()
}

use build_core::key::to_hex as hex;

/// An unpacked pack.
#[derive(Clone, Debug)]
pub struct Unpacked {
    /// The tree's folder: the build's root.
    pub dir: PathBuf,
    /// The tree's hash (of its members').
    pub hash: String,
    /// The tree's files.
    pub files: u64,
    /// Each carried tool's folder.
    pub tools: BTreeMap<String, PathBuf>,
    /// Folders this call unpacked (the others were there).
    pub fresh: usize,
}

fn bad(why: impl Into<String>) -> Error {
    Error::player(
        "tools_incomplete",
        format!(
            "What this YM-builder carries could not be unpacked ({}). Download YM-builder again.",
            why.into()
        ),
    )
}

use build_core::fsx::{link_inside, relative_ok as path_ok, through_link};

fn read_exact(r: &mut impl Read, n: usize) -> Result<Vec<u8>> {
    let mut b = vec![0; n];
    r.read_exact(&mut b).map_err(|e| bad(e.to_string()))?;
    Ok(b)
}

fn le_u64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().unwrap_or_default())
}

/// One member of the pack.
struct Member<'a> {
    name: String,
    bytes: &'a [u8],
    sha: String,
}

/// The pack's members, from its table (their bytes are checked when they are unpacked: [`place`]).
fn members(pack: &[u8]) -> Result<Vec<Member<'_>>> {
    let short = || bad("the pack ends early");
    if pack.get(..8) != Some(&PACK_MAGIC[..]) {
        return Err(bad("not a pack of this format"));
    }
    let count = u32::from_le_bytes(pack.get(8..12).ok_or_else(short)?.try_into().unwrap_or_default()) as usize;
    let mut at = 12;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u16::from_le_bytes(pack.get(at..at + 2).ok_or_else(short)?.try_into().unwrap_or_default()) as usize;
        at += 2;
        let name = std::str::from_utf8(pack.get(at..at + len).ok_or_else(short)?)
            .map_err(|_| bad("a member's name is not UTF-8"))?
            .to_string();
        at += len;
        let head = pack.get(at..at + 48).ok_or_else(short)?;
        let (off, n) = (le_u64(&head[..8]) as usize, le_u64(&head[8..16]) as usize);
        let want = hex(&head[16..48]);
        at += 48;
        let bytes = pack.get(off..off.checked_add(n).ok_or_else(short)?).ok_or_else(short)?;
        let ok = name.strip_prefix("tree/").or_else(|| name.strip_prefix("tools/")).is_some_and(path_ok);
        if !ok {
            return Err(bad(format!("a member named {name:?}")));
        }
        out.push(Member { name, bytes, sha: want });
    }
    Ok(out)
}

/// `members` unpacked together into `dir` with `links` (a folder's files linked at a path in it), unless it is there
/// already, complete with `key`; whether it was unpacked now, and its files.
fn place(members: &[&Member<'_>], key: &str, dir: &Path, links: &[(PathBuf, PathBuf)]) -> Result<(bool, u64)> {
    let complete = |d: &Path| {
        let note = std::fs::read_to_string(d.join(COMPLETE)).unwrap_or_default();
        let mut w = note.split_whitespace();
        (w.next() == Some(key)).then(|| w.next().and_then(|n| n.parse().ok()).unwrap_or(0))
    };
    if let Some(n) = complete(dir) {
        return Ok((false, n));
    }
    for m in members {
        if hex(&Sha256::digest(m.bytes)) != m.sha {
            return Err(bad(format!("{} is damaged", m.name)));
        }
    }
    let parent = dir.parent().unwrap_or(dir);
    std::fs::create_dir_all(parent).at(parent)?;
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let part = parent.join(format!("{name}.part-{}", std::process::id()));
    let _ = fsx::remove_tree(&part);
    let mut files = 0;
    for m in members {
        match write_files(m, &part) {
            Ok(n) => files += n,
            Err(e) => {
                let _ = fsx::remove_tree(&part);
                return Err(e);
            }
        }
    }
    for (from, rel) in links {
        if let Err(e) = link_dir(from, &part.join(rel)) {
            let _ = fsx::remove_tree(&part);
            return Err(e);
        }
    }
    fsx::write_atomic(&part.join(COMPLETE), format!("{key} {files}\n").as_bytes())?;
    if dir.exists() {
        // (another builder's that just won, or one a stopped unpack left half done: replaced)
        if complete(dir).is_some() {
            let _ = fsx::remove_tree(&part);
            return Ok((false, files));
        }
        fsx::remove_tree(dir)?;
    }
    if let Err(e) = fsx::rename(&part, dir) {
        let _ = fsx::remove_tree(&part);
        if complete(dir).is_none() {
            return Err(Error::io(dir, e));
        }
        return Ok((false, files));
    }
    Ok((true, files))
}

/// The tree's hash: of its members' names and recorded hashes (the pack's table alone).
fn tree_hash(all: &[Member<'_>]) -> String {
    let mut h = Sha256::new();
    for m in all {
        h.update(m.name.as_bytes());
        h.update(m.sha.as_bytes());
    }
    hex(&h.finalize())
}

/// Whether [`unpack`] of `pack` into `cache` would unpack its tree now (the first run of a release: seconds, minutes
/// on a hard drive), from the pack's table alone. A pack it cannot read: true (the unpack says why).
pub fn unpacks_now(pack: &[u8], cache: &Path) -> bool {
    let Ok(all) = members(pack) else { return true };
    let hash = tree_hash(&all);
    let note = std::fs::read_to_string(cache.join("sources").join(&hash[..16]).join(COMPLETE)).unwrap_or_default();
    note.split_whitespace().next() != Some(hash.as_str())
}

/// The release a pack carries (its tree's `VERSION`, in its `tree/files` member: a few small files, decoded alone) and
/// its tree's hash (16 digits); `None` when it carries none.
pub fn carried_release(pack: &[u8]) -> Option<(String, String)> {
    let all = members(pack).ok()?;
    let hash = tree_hash(&all)[..16].to_string();
    let files = all.iter().find(|m| m.name == "tree/files")?;
    if hex(&Sha256::digest(files.bytes)) != files.sha {
        return None;
    }
    let mut z = ruzstd::decoding::StreamingDecoder::new(files.bytes).ok()?;
    if read_exact(&mut z, MAGIC.len()).ok()? != MAGIC {
        return None;
    }
    loop {
        let len = u16::from_le_bytes(read_exact(&mut z, 2).ok()?.try_into().ok()?) as usize;
        if len == 0 {
            return None;
        }
        let rel = String::from_utf8(read_exact(&mut z, len).ok()?).ok()?;
        let _mode = read_exact(&mut z, 1).ok()?;
        let size = le_u64(&read_exact(&mut z, 8).ok()?);
        let _sha = read_exact(&mut z, 32).ok()?;
        // (a top-level file is small; anything larger here is not a pack of this format)
        let bytes = read_exact(&mut z, usize::try_from(size).ok().filter(|&n| n <= 16 << 20)?).ok()?;
        if rel == "VERSION" {
            return Some((String::from_utf8_lossy(&bytes).trim().to_string(), hash));
        }
    }
}

/// `pack` unpacked into the cache: its tree into `<cache>/sources/<hash>/` with each tool at `tools/<name>/` in it,
/// hard-linked from `<cache>/tools/<name>-<hash>/`, each folder unpacked only when it is not there already. Two
/// builders at once each unpack into a folder of their own and the first rename wins; a stopped one leaves a folder
/// with no [`COMPLETE`] note, never taken for a finished one.
pub fn unpack(pack: &[u8], cache: &Path) -> Result<Unpacked> {
    let all = members(pack)?;
    let tree: Vec<&Member<'_>> = all.iter().filter(|m| m.name.starts_with("tree/")).collect();
    if tree.is_empty() {
        return Err(bad("no tree in it"));
    }
    let tool_members: Vec<&Member<'_>> = all.iter().filter(|m| m.name.starts_with("tools/")).collect();
    let hash = tree_hash(&all);
    let dir = cache.join("sources").join(&hash[..16]);
    let mut fresh = 0;
    let mut cached = BTreeMap::new();
    let tools: BTreeMap<String, PathBuf> = tool_members
        .iter()
        .map(|m| {
            let name = m.name["tools/".len()..].to_string();
            (name.clone(), dir.join("tools").join(&name))
        })
        .collect();
    // (each tool once in the cache: the tree links its files)
    let complete_tree = std::fs::read_to_string(dir.join(COMPLETE))
        .ok()
        .is_some_and(|n| n.split_whitespace().next() == Some(hash.as_str()));
    if !complete_tree {
        for m in &tool_members {
            let name = &m.name["tools/".len()..];
            let at = cache.join("tools").join(format!("{name}-{}", &m.sha[..16]));
            fresh += usize::from(place(&[*m], &m.sha, &at, &[])?.0);
            cached.insert(name.to_string(), at);
        }
    }
    let links: Vec<(PathBuf, PathBuf)> =
        cached.iter().map(|(name, at)| (at.clone(), PathBuf::from("tools").join(name))).collect();
    let (fresh_tree, files) = place(&tree, &hash, &dir, &links)?;
    fresh += usize::from(fresh_tree);
    *TOOLS.lock().unwrap_or_else(|p| p.into_inner()) = tools.clone();
    // (this release's folders marked used: the cache's budget keeps them by it, crate::storage)
    let mut current = vec![dir.clone()];
    for m in &tool_members {
        current.push(cache.join("tools").join(format!("{}-{}", &m.name["tools/".len()..], &m.sha[..16])));
    }
    for d in &current {
        let _ = std::fs::write(d.join(USED), b"");
    }
    Ok(Unpacked { dir, hash, files, tools, fresh })
}

/// The files of folder `from` placed at `to` (made when missing), each hard-linked where it can be, else copied, its
/// relative symbolic links kept as links (a carried tool into the unpacked tree; `from-source`'s tools into its source).
pub fn link_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).at(to)?;
    for e in std::fs::read_dir(from).at(from)? {
        let e = e.at(from)?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let kind = e.file_type().at(&src)?;
        if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&src).at(&src)?, &dst).at(&dst)?;
            #[cfg(not(unix))]
            return Err(bad(format!("a symbolic link, {}, on a system that unpacks none", src.display())));
        } else if kind.is_dir() {
            if e.file_name() != COMPLETE {
                link_dir(&src, &dst)?;
            }
        } else if e.file_name() != COMPLETE {
            fsx::link_or_copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// A member's files written under `dir`, each checked; how many.
fn write_files(m: &Member<'_>, dir: &Path) -> Result<u64> {
    let rel_err = |e: String| bad(format!("{}: {e}", m.name));
    let mut z = ruzstd::decoding::StreamingDecoder::new(m.bytes).map_err(|e| rel_err(e.to_string()))?;
    if read_exact(&mut z, MAGIC.len())? != MAGIC {
        return Err(rel_err("not a member of this format".into()));
    }
    let mut n = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let len = u16::from_le_bytes(read_exact(&mut z, 2)?.try_into().unwrap_or_default()) as usize;
        if len == 0 {
            let count = le_u64(&read_exact(&mut z, 8)?);
            if count != n {
                return Err(rel_err(format!("{n} files, it says {count}")));
            }
            return Ok(n);
        }
        let rel =
            String::from_utf8(read_exact(&mut z, len)?).map_err(|_| rel_err("a path that is not UTF-8".into()))?;
        if !path_ok(&rel) {
            return Err(rel_err(format!("a path outside its folder: {rel:?}")));
        }
        let mode = read_exact(&mut z, 1)?[0];
        let size = le_u64(&read_exact(&mut z, 8)?);
        let want = hex(&read_exact(&mut z, 32)?);
        let path = dir.join(&rel);
        if let Some(link) = through_link(dir, &rel) {
            return Err(rel_err(format!("{rel} goes through the link {link}")));
        }
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(rel_err(format!("{rel} is there twice")));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        if mode == 2 {
            let target = read_exact(&mut z, size as usize)?;
            if hex(&Sha256::digest(&target)) != want {
                return Err(rel_err(format!("{rel} is not the link it should be")));
            }
            let target = String::from_utf8(target).map_err(|_| rel_err(format!("{rel}: a link that is not UTF-8")))?;
            if !link_inside(&rel, &target) {
                return Err(rel_err(format!("{rel} links outside its folder ({target})")));
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &path).at(&path)?;
            #[cfg(not(unix))]
            return Err(rel_err(format!("{rel}: a symbolic link, on a system that unpacks none")));
            #[cfg(unix)]
            {
                n += 1;
                continue;
            }
        }
        let mut out = std::fs::File::create(&path).at(&path)?;
        let mut h = Sha256::new();
        let mut left = size;
        while left > 0 {
            let take = left.min(buf.len() as u64) as usize;
            z.read_exact(&mut buf[..take]).map_err(|e| rel_err(format!("{rel}: {e}")))?;
            h.update(&buf[..take]);
            out.write_all(&buf[..take]).at(&path)?;
            left -= take as u64;
        }
        if hex(&h.finalize()) != want {
            return Err(rel_err(format!("{rel} is not the file it should be")));
        }
        #[cfg(unix)]
        if mode == 1 {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).at(&path)?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        n += 1;
    }
}

// ---- the writer (`YM-builder dev release sources`, `unpack`): the one that makes what the reader above reads ----

/// The tree a player's build reads, as tracked: the game's sources (`host/`, `game/`, `gpu/`, and the game's Rust, whose
/// key says whether the prebuilt `libYM_rs.a` added with `--add` is theirs: build-compile's `rust`), the
/// third-party code it compiles and the licences the game folder gets, the assets the finish makes its files from.
pub const TREE: [&str; 15] = [
    "Cargo.toml",
    "Cargo.lock",
    "YM-rs",
    "host",
    "game",
    "gpu",
    "third_party",
    "assets/profiles",
    "assets/pipeline_states",
    "assets/prompt_faces",
    "assets/YM.png",
    "assets/YM.ico",
    "assets/addresses.ini",
    "LICENSE",
    "tools.lock",
];
/// What of [`TREE`] a player's build never reads: the third-party code only developers build (Tracy's client,
/// FidelityFX SDK's shader sources) and RenHDrum's Rust sources (its built library comes as a file added: `--add`).
pub const LEFT_OUT: [&str; 7] = [
    "third_party/a64-kit/",
    "third_party/tracy/",
    "third_party/fidelityfx-sdk/",
    "third_party/renhdrum/src/",
    "third_party/renhdrum/vendor/",
    "third_party/renhdrum/Cargo.",
    "third_party/renhdrum/build.rs",
];
/// The zstd program's arguments for a member: one thread and its checksum, so the same files give the same bytes.
pub const MEMBER_ZSTD: [&str; 5] = ["-19", "-T1", "-q", "-c", "--check"];

/// A member's file: its bytes from a file or given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// The file at this path (read when the member is made).
    File(PathBuf),
    /// These bytes (a link's: its target).
    Bytes(Vec<u8>),
}

/// One file of a member: its path in the member, its mode (0 plain, 1 executable, 2 a link) and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Relative, `/` separated.
    pub path: String,
    /// 0 plain, 1 executable, 2 a relative symbolic link (its bytes the target).
    pub mode: u8,
    /// Its bytes.
    pub body: Body,
}

/// The files of the checkout at `root` tracked under `paths`, relative with `/`, sorted: jj's in a jj checkout (its
/// working copy as the last jj command saw it: `jj status` first takes in what changed since), else git's.
pub fn tracked(root: &Path, paths: &[&str]) -> std::result::Result<Vec<String>, String> {
    if root.join(".jj").is_dir() {
        let out = build_core::child::output(
            build_core::child::command("jj")
                .args(["file", "list", "--ignore-working-copy", "-r", "@"])
                .args(paths)
                .current_dir(root),
        );
        if let Some(out) = out.ok().filter(|o| o.status.success()) {
            let mut files: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|l| l.trim().replace('\\', "/"))
                .filter(|l| !l.is_empty())
                .collect();
            files.sort();
            return Ok(files);
        }
    }
    let out = build_core::child::output(
        build_core::child::command("git").args(["ls-files", "-z", "--"]).args(paths).current_dir(root),
    )
    .map_err(|e| format!("git did not start: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let mut files: Vec<String> =
        String::from_utf8_lossy(&out.stdout).split('\0').filter(|f| !f.is_empty()).map(str::to_string).collect();
    files.sort();
    Ok(files)
}

/// Every file of the folder at `root` under `paths` (each a folder or a file), relative with `/`, sorted: [`tracked`]'s
/// list for a folder that is no checkout, a commit's archive unpacked (`from-source`), whose files are all tracked.
pub fn listed(root: &Path, paths: &[&str]) -> std::result::Result<Vec<String>, String> {
    let mut files = Vec::new();
    for p in paths {
        let full = root.join(p);
        if full.is_file() {
            files.push(p.to_string());
        } else if full.is_dir() {
            for rel in fsx::files_under(&full).map_err(|e| e.to_string())? {
                files.push(format!("{p}/{}", rel.to_string_lossy().replace('\\', "/")));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Every file under `folder` (links to files followed, a link to a folder not walked): (relative path with `/`, path).
fn files_under(folder: &Path) -> std::result::Result<Vec<(String, PathBuf)>, String> {
    Ok(crate::tools::archive::entries(folder)?.into_iter().filter(|(_, p)| p.is_file()).collect())
}

fn executable_file(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| crate::tools::archive::executable(&m))
}

/// The tree's files, sorted: `tracked` (the checkout's tracked files under [`TREE`], at `root`) but [`LEFT_OUT`],
/// `VERSION` (the release's name), the third-party objects at `third_party_objects/` (`YM-builder compile
/// --third-party-objects`'s folder, its `set.json` in it) and each of `add` (`<path in the tree>=<file>`, over a tracked
/// file of that path). A tree with no pipeline state list is refused: a release ships them.
pub fn tree_entries(
    root: &Path,
    tracked: &[String],
    version: &str,
    objects: Option<&Path>,
    add: &[String],
) -> std::result::Result<Vec<Entry>, String> {
    tree_entries_leaving(root, tracked, version, objects, add, &LEFT_OUT)
}

/// [`tree_entries`] with another list of what is left out than [`LEFT_OUT`] (`from-source`: the source's own).
pub fn tree_entries_leaving(
    root: &Path,
    tracked: &[String],
    version: &str,
    objects: Option<&Path>,
    add: &[String],
    left_out: &[&str],
) -> std::result::Result<Vec<Entry>, String> {
    let mut all: BTreeMap<String, Entry> = BTreeMap::new();
    let mut put = |path: String, mode: u8, body: Body| {
        all.insert(path.clone(), Entry { path, mode, body });
    };
    for rel in tracked {
        let full = root.join(rel);
        if left_out.iter().any(|l| rel.starts_with(l)) || !full.is_file() {
            continue;
        }
        put(rel.clone(), u8::from(executable_file(&full)), Body::File(full));
    }
    put("VERSION".into(), 0, Body::Bytes(format!("{version}\n").into_bytes()));
    if let Some(objects) = objects {
        if !objects.join("set.json").is_file() {
            return Err(format!(
                "{} has no set.json (YM-builder compile --third-party-objects makes it)",
                objects.display()
            ));
        }
        for (rel, full) in files_under(objects)? {
            put(format!("third_party_objects/{rel}"), 0, Body::File(full));
        }
    }
    for spec in add {
        let (rel, file) = spec.split_once('=').unwrap_or((spec, ""));
        let file = Path::new(file);
        if rel.is_empty() || !file.is_file() {
            return Err(format!("--add {spec}: not <path in the tree>=<file>"));
        }
        put(rel.replace('\\', "/"), u8::from(executable_file(file)), Body::File(file.to_path_buf()));
    }
    if !all.keys().any(|r| r.starts_with("assets/pipeline_states/") && r.ends_with(".bin.xz")) {
        return Err("assets/pipeline_states/ holds no pipeline state list: a release ships them".into());
    }
    Ok(all.into_values().collect())
}

/// A tool's folder as a member's files, sorted: its files, and its relative links kept as links (mode 2); a link
/// leading out of the folder is refused.
pub fn tool_entries(folder: &Path) -> std::result::Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for (rel, full) in crate::tools::archive::entries(folder)? {
        let meta = std::fs::symlink_metadata(&full).map_err(|e| format!("{}: {e}", full.display()))?;
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&full).map_err(|e| format!("{}: {e}", full.display()))?;
            let target = target.to_string_lossy().into_owned();
            if !link_inside(&rel, &target) {
                return Err(format!("{}: {rel} links outside the tool ({target})", folder.display()));
            }
            out.push(Entry { path: rel, mode: 2, body: Body::Bytes(target.into_bytes()) });
        } else if meta.is_file() {
            out.push(Entry {
                path: rel,
                mode: u8::from(crate::tools::archive::executable(&meta)),
                body: Body::File(full),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The tree member a file goes in: `tree/<its top folder>` (`third_party/<name>`, `assets/<name>` and `gpu/<name>` each
/// apart: a change compresses again only its own part), `tree/files` for the top-level files.
pub fn group(rel: &str) -> String {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        [_] => "tree/files".into(),
        [top @ ("third_party" | "assets" | "gpu"), name, _, ..] => format!("tree/{top}-{name}"),
        [top, ..] => format!("tree/{top}"),
        [] => "tree/files".into(),
    }
}

/// A member's bytes before compression: [`MAGIC`], its files, the end (the format above).
pub fn member_bytes(entries: &[Entry]) -> std::result::Result<Vec<u8>, String> {
    let mut out = MAGIC.to_vec();
    for e in entries {
        let read;
        let data: &[u8] = match &e.body {
            Body::Bytes(b) => b,
            Body::File(p) => {
                read = std::fs::read(p).map_err(|err| format!("{}: {err}", p.display()))?;
                &read
            }
        };
        let name = e.path.as_bytes();
        let len = u16::try_from(name.len()).map_err(|_| format!("{}: a path too long", e.path))?;
        out.extend(len.to_le_bytes());
        out.extend(name);
        out.push(e.mode);
        out.extend((data.len() as u64).to_le_bytes());
        out.extend(Sha256::digest(data));
        out.extend(data);
    }
    out.extend(0u16.to_le_bytes());
    out.extend((entries.len() as u64).to_le_bytes());
    Ok(out)
}

/// `(name, compressed bytes)` as the pack: its table, then the members' bytes in that order.
pub fn pack_bytes(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut head = PACK_MAGIC.to_vec();
    head.extend((members.len() as u32).to_le_bytes());
    let mut off = head.len() + members.iter().map(|(n, _)| 2 + n.len() + 48).sum::<usize>();
    for (name, data) in members {
        head.extend((name.len() as u16).to_le_bytes());
        head.extend(name.as_bytes());
        head.extend((off as u64).to_le_bytes());
        head.extend((data.len() as u64).to_le_bytes());
        head.extend(Sha256::digest(data));
        off += data.len();
    }
    for (_, data) in members {
        head.extend(data);
    }
    head
}

/// `raw` as one zstd frame, by the zstd program at [`MEMBER_ZSTD`] (the same bytes each time: one thread).
pub fn compress(raw: &[u8]) -> std::result::Result<Vec<u8>, String> {
    use std::process::Stdio;
    let mut child = build_core::child::spawn(
        build_core::child::command(crate::tools::archive::zstd()?)
            .args(MEMBER_ZSTD)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .map_err(|e| format!("zstd did not start: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("zstd's input")?;
    let (fed, out) = std::thread::scope(|s| {
        let feed = s.spawn(move || stdin.write_all(raw));
        let out = child.wait_with_output();
        (feed.join(), out)
    });
    let out = out.map_err(|e| format!("zstd: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("zstd failed: {}", &err[err.len().saturating_sub(300)..]));
    }
    fed.map_err(|_| "zstd's input".to_string())?.map_err(|e| format!("zstd's input: {e}"))?;
    Ok(out.stdout)
}

/// `raw` as one zstd frame, quickly: the zstd program at `-3` when this PC has one, else ruzstd's fastest level (no
/// program needed). A pack for this PC alone (`from-source`'s), never a release's: its bytes are not [`compress`]'s.
pub fn compress_fast(raw: &[u8]) -> std::result::Result<Vec<u8>, String> {
    use std::process::Stdio;
    let Ok(zstd) = crate::tools::archive::zstd() else {
        return Ok(ruzstd::encoding::compress_to_vec(raw, ruzstd::encoding::CompressionLevel::Fastest));
    };
    let mut child = build_core::child::spawn(
        build_core::child::command(zstd)
            .args(["-3", "-T0", "-q", "-c", "--check"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .map_err(|e| format!("zstd did not start: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("zstd's input")?;
    let (fed, out) = std::thread::scope(|s| {
        let feed = s.spawn(move || stdin.write_all(raw));
        let out = child.wait_with_output();
        (feed.join(), out)
    });
    let out = out.map_err(|e| format!("zstd: {e}"))?;
    if !out.status.success() {
        return Err("zstd failed".into());
    }
    fed.map_err(|_| "zstd's input".to_string())?.map_err(|e| format!("zstd's input: {e}"))?;
    Ok(out.stdout)
}

/// A member's compressor: its raw bytes to one zstd frame ([`compress`] for a release).
pub type Compressor<'a> = &'a dyn Fn(&[u8]) -> std::result::Result<Vec<u8>, String>;

/// What [`write`] made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// Its members.
    pub members: usize,
    /// The files in them.
    pub files: usize,
    /// The members' bytes before compression.
    pub raw: u64,
    /// The pack's bytes.
    pub size: u64,
    /// The pack's SHA-256.
    pub sha256: String,
}

/// The pack of `tree` (each [`group`] a member, by name) and `tools` (`(name, folder)`, each a member `tools/<name>`,
/// in that order), compressed by `zstd` ([`compress`] for a release), written to `out` through `out.new`.
pub fn write(
    out: &Path,
    tree: &[Entry],
    tools: &[(String, PathBuf)],
    zstd: Compressor<'_>,
) -> std::result::Result<Written, String> {
    let mut groups: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    for e in tree {
        groups.entry(group(&e.path)).or_default().push(e.clone());
    }
    let (mut members, mut raw_size, mut files) = (Vec::new(), 0u64, tree.len());
    for (name, entries) in &groups {
        let raw = member_bytes(entries)?;
        raw_size += raw.len() as u64;
        members.push((name.clone(), zstd(&raw)?));
    }
    for (name, folder) in tools {
        let entries = tool_entries(folder)?;
        let raw = member_bytes(&entries)?;
        raw_size += raw.len() as u64;
        files += entries.len();
        members.push((format!("tools/{name}"), zstd(&raw)?));
    }
    let data = pack_bytes(&members);
    if let Some(parent) = std::path::absolute(out).ok().as_deref().and_then(Path::parent) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut new = out.as_os_str().to_owned();
    new.push(".new");
    std::fs::write(&new, &data).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::rename(&new, out).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(Written {
        members: members.len(),
        files,
        raw: raw_size,
        size: data.len() as u64,
        sha256: hex(&Sha256::digest(&data)),
    })
}

/// A pack's tree written into `folder` and each tool into `folder/tools/<name>`, every member's and file's SHA-256
/// checked (as a release's program unpacks it, into a plain folder: a release's check of what it carries); the tree's
/// files.
pub fn unpack_into(pack: &[u8], folder: &Path) -> Result<u64> {
    let mut n = 0;
    for m in members(pack)? {
        if hex(&Sha256::digest(m.bytes)) != m.sha {
            return Err(bad(format!("{} is damaged", m.name)));
        }
        match m.name.strip_prefix("tools/") {
            Some(tool) => {
                write_files(&m, &folder.join("tools").join(tool))?;
            }
            None => n += write_files(&m, folder)?,
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A member of `files`, as [`write`] makes one (ruzstd's fastest level for the zstd program's: the reader takes any).
    fn member(files: &[(&str, &[u8])]) -> Vec<u8> {
        let modes: Vec<(&str, u8, &[u8])> = files.iter().map(|(p, d)| (*p, 0, *d)).collect();
        member_modes(&modes)
    }

    /// A member of `(path, mode, bytes)` entries (mode 2: a link, its bytes the target).
    fn member_modes(files: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let entries: Vec<Entry> = files
            .iter()
            .map(|(p, mode, d)| Entry { path: p.to_string(), mode: *mode, body: Body::Bytes(d.to_vec()) })
            .collect();
        fast_zstd(&member_bytes(&entries).unwrap()).unwrap()
    }

    fn fast_zstd(raw: &[u8]) -> std::result::Result<Vec<u8>, String> {
        Ok(ruzstd::encoding::compress_to_vec(raw, ruzstd::encoding::CompressionLevel::Fastest))
    }

    fn pack(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
        pack_bytes(&members.iter().map(|(n, b)| (n.to_string(), b.clone())).collect::<Vec<_>>())
    }

    /// One unpack at a time among the tests: each sets the process's tool table ([`tool_dir`]).
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static ONE: Mutex<()> = Mutex::new(());
        ONE.lock().unwrap_or_else(|p| p.into_inner())
    }

    use crate::scratch;

    #[test]
    fn unpacked_once_and_tools_kept_across_releases() {
        let _one = serial();
        let cache = scratch("once");
        let tool = member(&[("bin/clang", b"#!clang")]);
        let one = pack(&[
            ("tree/host", member(&[("host/a.c", b"int a;")])),
            ("tree/assets", member(&[("assets/x/y.bin", b"\x00\x01")])),
            ("tools/clang", tool.clone()),
        ]);
        let u = unpack(&one, &cache).unwrap();
        assert_eq!((u.files, u.fresh), (2, 2));
        assert_eq!(std::fs::read(u.dir.join("host/a.c")).unwrap(), b"int a;");
        assert_eq!(std::fs::read(u.tools["clang"].join("bin/clang")).unwrap(), b"#!clang");
        assert_eq!(u.tools["clang"], u.dir.join("tools/clang"));
        assert_eq!(tool_dir("clang"), Some(u.tools["clang"].clone()));
        assert_eq!(unpack(&one, &cache).unwrap().fresh, 0, "all there: nothing unpacked");
        // (the next release: one source changed, the same tool, which is not unpacked again)
        let two = pack(&[("tree/host", member(&[("host/a.c", b"int b;")])), ("tools/clang", tool)]);
        let v = unpack(&two, &cache).unwrap();
        assert_ne!(v.dir, u.dir);
        assert_eq!(v.fresh, 1, "the tree alone");
        assert_eq!(std::fs::read(v.tools["clang"].join("bin/clang")).unwrap(), b"#!clang");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
            assert_eq!(ino(&v.tools["clang"].join("bin/clang")), ino(&u.tools["clang"].join("bin/clang")), "linked");
        }
        // (a folder another builder left half done is replaced)
        std::fs::remove_file(v.dir.join(COMPLETE)).unwrap();
        assert_eq!(unpack(&two, &cache).unwrap().fresh, 1);
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_pack_unpacked_before_is_not_hashed_again() {
        let _one = serial();
        let cache = scratch("unhashed");
        let mut one = pack(&[("tree/host", member(&[("host/a.c", b"int a;")]))]);
        assert_eq!(unpack(&one, &cache).unwrap().fresh, 1);
        // (a member's bytes changed after the table: only an unpack reads them)
        let last = one.len() - 3;
        one[last] ^= 0xff;
        assert_eq!(unpack(&one, &cache).unwrap().fresh, 0, "the folder there: the pack's table alone read");
        let _ = std::fs::remove_dir_all(&cache);
        assert!(unpack(&one, &cache).is_err(), "unpacked anew: its bytes checked");
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn each_unpack_marks_its_folders_used() {
        let _one = serial();
        let cache = scratch("used");
        let p = pack(&[("tree/host", member(&[("host/a.c", b"1")])), ("tools/clang", member(&[("bin/c", b"1")]))]);
        let u = unpack(&p, &cache).unwrap();
        let tool = std::fs::read_dir(cache.join("tools")).unwrap().flatten().next().unwrap().path();
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 86_400);
        for d in [&u.dir, &tool] {
            std::fs::File::options().write(true).open(d.join(USED)).unwrap().set_modified(when).unwrap();
        }
        unpack(&p, &cache).unwrap();
        for d in [&u.dir, &tool] {
            let age = std::fs::metadata(d.join(USED)).unwrap().modified().unwrap().elapsed().unwrap_or_default();
            assert!(age < std::time::Duration::from_secs(3600), "{}", d.display());
        }
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn the_carried_release_is_read_from_the_pack_and_an_unpack_known_before_it_runs() {
        let _one = serial();
        let cache = scratch("release");
        let p = pack(&[
            ("tree/files", member(&[("VERSION", b"nightly-7\n"), ("README", b"r")])),
            ("tree/host", member(&[("host/a.c", b"int a;")])),
        ]);
        let (release, hash) = carried_release(&p).unwrap();
        assert_eq!((release.as_str(), hash.len()), ("nightly-7", 16));
        assert!(carried_release(&pack(&[("tree/host", member(&[("host/a.c", b"x")]))])).is_none());
        assert!(unpacks_now(&p, &cache));
        let u = unpack(&p, &cache).unwrap();
        assert!(!unpacks_now(&p, &cache) && u.hash.starts_with(&hash));
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn links_stay_inside() {
        assert!(link_inside("lib", "usr/lib"));
        assert!(link_inside("a/b/c", "../../x"));
        assert!(!link_inside("a/b", "../../x"));
        assert!(!link_inside("a", "/etc/passwd"));
        assert!(!link_inside("a", ""));
    }

    #[cfg(unix)]
    #[test]
    fn nested_links_do_not_escape() {
        let _one = serial();
        let cache = scratch("nested");
        // (each link alone is inside: a/b -> .. is a/, a/b/c -> .. would be a/b/.. = a/ too, but written through the
        // link a/b it lands at the folder's parent's parent)
        let hostile = member_modes(&[("a/b", 2, b".."), ("a/b/c", 2, b".."), ("a/b/c/escaped", 0, b"x")]);
        assert!(unpack(&pack(&[("tree/x", hostile)]), &cache).is_err());
        assert!(!cache.join("escaped").exists() && !cache.parent().unwrap().join("escaped").exists());
        // (nor a file written over a link a member made)
        let over = member_modes(&[("a/l", 2, b"x"), ("a/l", 0, b"y")]);
        assert!(unpack(&pack(&[("tree/x", over)]), &cache).is_err());
        // (a link inside, then files beside it: fine)
        let fine = member_modes(&[("usr/lib/a.so", 0, b"a"), ("lib", 2, b"usr/lib")]);
        assert!(unpack(&pack(&[("tree/x", fine)]), &cache).is_ok());
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn a_damaged_or_hostile_pack_is_refused() {
        let _one = serial();
        let cache = scratch("bad");
        assert!(unpack(&pack(&[("tree/x", member(&[("../escape", b"x")]))]), &cache).is_err());
        assert!(!cache.join("escape").exists());
        assert!(unpack(&pack(&[("tools/../x", member(&[("a", b"x")]))]), &cache).is_err());
        let mut p = pack(&[("tree/host", member(&[("host/a.c", b"int a;")]))]);
        let last = p.len() - 3;
        p[last] ^= 0xff;
        assert!(unpack(&p, &cache).is_err(), "a member whose SHA-256 is not its own");
        assert!(unpack(b"not a pack", &cache).is_err());
        assert!(unpack(&pack(&[("tools/clang", member(&[("a", b"x")]))]), &cache).is_err(), "no tree");
        let leftovers: Vec<std::fs::DirEntry> =
            std::fs::read_dir(cache.join("sources")).map(|d| d.flatten().collect()).unwrap_or_default();
        assert!(leftovers.is_empty(), "nothing half unpacked is left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// A small checkout: two tracked sources, one left out, a pipeline state list when `list`.
    fn checkout(d: &Path, list: bool) -> Vec<String> {
        let mut files = vec!["LICENSE", "host/a.c", "third_party/tracy/t.c", "gpu/vk/x/y.cpp"];
        if list {
            files.push("assets/pipeline_states/contrib/someone.bin.xz");
        }
        for f in &files {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), f.as_bytes()).unwrap();
        }
        files.iter().map(|f| f.to_string()).collect()
    }

    #[test]
    fn the_tree_carries_the_pipeline_state_lists_and_its_version() {
        let d = scratch("tree-entries");
        let tracked = checkout(&d, false);
        let e = tree_entries(&d, &tracked, "v0.0.0-test", None, &[]).unwrap_err();
        assert!(e.contains("no pipeline state list"), "{e}");
        let tracked = checkout(&d, true);
        let names: Vec<String> =
            tree_entries(&d, &tracked, "v0.0.0-test", None, &[]).unwrap().into_iter().map(|e| e.path).collect();
        assert!(names.contains(&"assets/pipeline_states/contrib/someone.bin.xz".to_string()));
        assert!(names.contains(&"VERSION".to_string()) && names.contains(&"host/a.c".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("third_party/tracy/")), "left out");
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        assert!(tree_entries(&d, &tracked, "v", Some(&d.join("no-objects")), &[]).is_err(), "no set.json");
        assert!(tree_entries(&d, &tracked, "v", None, &["host/b.c".into()]).is_err(), "not <path>=<file>");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_members_are_grouped_by_top_folder() {
        assert_eq!(group("VERSION"), "tree/files");
        assert_eq!(group("host/a.c"), "tree/host");
        assert_eq!(group("third_party/imgui/a.h"), "tree/third_party-imgui");
        assert_eq!(group("third_party/LICENSE"), "tree/third_party");
        assert_eq!(group("gpu/vk/x.cpp"), "tree/gpu-vk");
        assert_eq!(group("assets/YM.png"), "tree/assets");
        assert_eq!(group("third_party_objects/set.json"), "tree/third_party_objects");
    }

    #[cfg(unix)]
    #[test]
    fn what_the_writer_writes_the_reader_reads_the_same_each_time() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("writer");
        let tracked = checkout(&d, true);
        std::fs::create_dir_all(d.join("objects/linux")).unwrap();
        std::fs::write(d.join("objects/set.json"), b"{}").unwrap();
        std::fs::write(d.join("objects/linux/a.o"), b"obj").unwrap();
        std::fs::write(d.join("renhdrum.so"), b"lib").unwrap();
        let tool = d.join("bundle/clang");
        std::fs::create_dir_all(tool.join("usr/lib")).unwrap();
        std::fs::write(tool.join("usr/lib/a.so"), b"a").unwrap();
        std::fs::write(tool.join("clang"), b"#!clang").unwrap();
        std::fs::set_permissions(tool.join("clang"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("usr/lib", tool.join("lib")).unwrap();
        let add = ["third_party/renhdrum/renhdrum.so=".to_string() + &d.join("renhdrum.so").display().to_string()];
        let tree = tree_entries(&d, &tracked, "nightly-7", Some(&d.join("objects")), &add).unwrap();
        let tools = [("clang".to_string(), tool.clone())];
        let out = d.join("out/sources.pack");
        let w = write(&out, &tree, &tools, &fast_zstd).unwrap();
        let first = std::fs::read(&out).unwrap();
        assert_eq!((w.size, w.files), (first.len() as u64, tree.len() + 3));
        assert_eq!(write(&out, &tree, &tools, &fast_zstd).unwrap(), w, "the same files, the same bytes");
        assert_eq!(std::fs::read(&out).unwrap(), first);
        assert_eq!(carried_release(&first).unwrap().0, "nightly-7");
        let into = d.join("carried");
        assert_eq!(unpack_into(&first, &into).unwrap(), tree.len() as u64);
        assert_eq!(std::fs::read(into.join("host/a.c")).unwrap(), b"host/a.c");
        assert_eq!(std::fs::read(into.join("third_party_objects/linux/a.o")).unwrap(), b"obj");
        assert_eq!(std::fs::read(into.join("third_party/renhdrum/renhdrum.so")).unwrap(), b"lib");
        assert_eq!(std::fs::read(into.join("VERSION")).unwrap(), b"nightly-7\n");
        let clang = into.join("tools/clang");
        assert_eq!(std::fs::metadata(clang.join("clang")).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(std::fs::read_link(clang.join("lib")).unwrap(), Path::new("usr/lib"));
        assert!(unpack_into(&first, &into).is_err(), "nothing written over what is there");
        // (and the cache's unpack, a player's, reads the same pack)
        let _one = serial();
        let u = unpack(&first, &d.join("cache")).unwrap();
        assert_eq!(u.files, tree.len() as u64);
        assert_eq!(std::fs::read(u.tools["clang"].join("clang")).unwrap(), b"#!clang");
        // (a tool whose link leads out of it is refused)
        std::os::unix::fs::symlink("../../etc", tool.join("out")).unwrap();
        assert!(tool_entries(&tool).unwrap_err().contains("links outside the tool"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_zstd_program_gives_the_same_frame_each_time() {
        if crate::tools::archive::zstd().is_err() {
            eprintln!("no zstd program: skipped");
            return;
        }
        let raw = member_bytes(&[Entry { path: "a".into(), mode: 0, body: Body::Bytes(vec![7; 100_000]) }]).unwrap();
        let one = compress(&raw).unwrap();
        assert_eq!(compress(&raw).unwrap(), one);
        let mut back = Vec::new();
        ruzstd::decoding::StreamingDecoder::new(&one[..]).unwrap().read_to_end(&mut back).unwrap();
        assert_eq!(back, raw);
    }

    #[cfg(unix)]
    #[test]
    fn a_plain_unpack_writes_nothing_through_a_link() {
        // (tests/test_unpack_links.py's, which checked the Python unpacker this replaces)
        let d = scratch("plain-links");
        let hostile = member_modes(&[("a/b", 2, b".."), ("a/b/c", 2, b".."), ("a/b/c/escaped", 0, b"x")]);
        assert!(unpack_into(&pack(&[("tree/x", hostile)]), &d.join("h")).is_err());
        assert!(!d.join("escaped").exists() && !d.parent().unwrap().join("escaped").exists());
        let over = member_modes(&[("a/l", 2, b"x"), ("a/l", 0, b"y")]);
        assert!(unpack_into(&pack(&[("tree/x", over)]), &d.join("o")).is_err());
        let fine = member_modes(&[("usr/lib/a.so", 0, b"a"), ("lib", 2, b"usr/lib")]);
        assert_eq!(unpack_into(&pack(&[("tree/x", fine)]), &d.join("f")).unwrap(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
