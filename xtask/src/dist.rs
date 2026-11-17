//! `cargo xtask dist`: build the release binaries and pack them for people to
//! download, with SHA-256 checksums beside them. Everything lands in
//! `target/dist/`.
//!
//! - Windows: a zip and, when Inno Setup is installed, an installer.
//! - macOS: `SliceCraft.app` in a disk image (`.dmg`). `--universal` builds
//!   it for Intel and Apple Silicon in one bundle.
//! - Linux and the rest: a tar.gz with a desktop file and a PNG icon.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Files from the repository that travel with the program.
const DOCS: [&str; 5] = ["README.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ATTRIBUTION.md"];

/// The programs in a release.
const BINS: [&str; 2] = ["slicecraft", "slicecraft-cli"];

/// The targets of a universal macOS build, and the oldest macOS each runs on.
const MAC_TARGETS: [(&str, &str); 2] =
    [("x86_64-apple-darwin", "10.13"), ("aarch64-apple-darwin", "11.0")];

fn platform(universal: bool) -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) if universal => "macos-universal",
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        // `uname -m` names: the shared release workflow's Flatpak job looks for
        // slicecraft-<version>-linux-<x86_64|aarch64>.tar.gz
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("freebsd", _) => "freebsd",
        _ => "other",
    }
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn run(what: &str, cmd: &mut Command) -> Result<(), String> {
    println!("== {what}");
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{what} failed ({s})")),
        Err(e) => Err(format!("{what}: cannot start: {e}")),
    }
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("cannot create {}: {e}", p.display()))
}

fn write(p: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    std::fs::write(p, data).map_err(|e| format!("cannot write {}: {e}", p.display()))
}

/// A fresh, empty directory.
fn fresh(p: &Path) -> Result<(), String> {
    if p.exists() {
        std::fs::remove_dir_all(p).map_err(|e| format!("cannot clear {}: {e}", p.display()))?;
    }
    mkdir(p)
}

pub fn dist(args: &[String]) -> ExitCode {
    let universal = args.iter().any(|a| a == "--universal");
    if universal && !cfg!(target_os = "macos") {
        eprintln!("xtask dist: --universal is for macOS");
        return ExitCode::FAILURE;
    }
    match build(universal) {
        Ok(files) => {
            println!("xtask dist: ok");
            for f in files {
                println!("  {}", f.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("xtask dist: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Build the programs; returns the directory holding them.
fn compile(universal: bool) -> Result<PathBuf, String> {
    let package_args = BINS.iter().flat_map(|b| ["-p", b]);
    if !universal {
        run(
            "cargo build --release",
            Command::new("cargo").args(["build", "--release"]).args(package_args),
        )?;
        return Ok(PathBuf::from("target").join("release"));
    }
    for (target, oldest) in MAC_TARGETS {
        run(
            &format!("cargo build --release --target {target}"),
            Command::new("cargo")
                .args(["build", "--release", "--target", target])
                .args(package_args.clone())
                .env("MACOSX_DEPLOYMENT_TARGET", oldest),
        )?;
    }
    // one binary holding both architectures
    let out = PathBuf::from("target").join("universal-apple-darwin").join("release");
    mkdir(&out)?;
    for bin in BINS {
        let mut lipo = Command::new("lipo");
        lipo.arg("-create").arg("-output").arg(out.join(bin));
        for (target, _) in MAC_TARGETS {
            lipo.arg(PathBuf::from("target").join(target).join("release").join(bin));
        }
        run(&format!("lipo {bin}"), &mut lipo)?;
    }
    Ok(out)
}

fn build(universal: bool) -> Result<Vec<PathBuf>, String> {
    let bins = compile(universal)?;
    let name = format!("slicecraft-{VERSION}-{}", platform(universal));
    let dist = PathBuf::from("target").join("dist");
    let stage = dist.join(&name);
    fresh(&stage)?;
    mkdir(&stage.join("examples"))?;
    for doc in DOCS {
        copy(Path::new(doc), &stage.join(doc))?;
    }
    copy(Path::new("examples/cube.stl"), &stage.join("examples").join("cube.stl"))?;
    let gui = bins.join(exe("slicecraft"));

    let archive = if cfg!(target_os = "macos") {
        mac_bundle(&bins, &stage)?;
        dist.join(format!("{name}.dmg"))
    } else {
        for bin in BINS {
            copy(&bins.join(exe(bin)), &stage.join(exe(bin)))?;
        }
        // the icon, drawn by the program itself
        let icon = if cfg!(windows) { "slicecraft.ico" } else { "slicecraft.png" };
        run("write the icon", Command::new(&gui).arg("--write-icon").arg(stage.join(icon)))?;
        if cfg!(windows) {
            dist.join(format!("{name}.zip"))
        } else {
            copy(
                &Path::new("packaging").join("linux").join("slicecraft.desktop"),
                &stage.join("slicecraft.desktop"),
            )?;
            dist.join(format!("{name}.tar.gz"))
        }
    };
    if archive.exists() {
        std::fs::remove_file(&archive)
            .map_err(|e| format!("cannot replace {}: {e}", archive.display()))?;
    }
    if cfg!(target_os = "macos") {
        run(
            "make the disk image",
            Command::new("hdiutil")
                .args(["create", "-quiet", "-ov", "-format", "UDZO", "-volname"])
                .arg(format!("SliceCraft {VERSION}"))
                .arg("-srcfolder")
                .arg(&stage)
                .arg(&archive),
        )?;
    } else {
        // zip on Windows (tar there writes zip), tar.gz elsewhere
        let flags = if cfg!(windows) { "-a -cf" } else { "-czf" };
        let mut tar = Command::new("tar");
        tar.args(flags.split(' '))
            .arg(archive.file_name().unwrap_or_default())
            .arg(&name)
            .current_dir(&dist);
        run("pack the archive", &mut tar)?;
    }
    let mut out = vec![archive];

    if cfg!(windows) {
        match inno_setup() {
            Some(iscc) => {
                let setup = dist.join(format!("{name}-setup.exe"));
                // plain absolute paths: Inno Setup refuses verbatim (`\\?\`) ones
                let here = std::env::current_dir().unwrap_or_default();
                let abs = |p: &Path| here.join(p);
                run(
                    "build the installer (Inno Setup)",
                    Command::new(iscc)
                        .arg("/Q")
                        .arg(format!("/DAppVersion={VERSION}"))
                        .arg(format!("/DSourceDir={}", abs(&stage).display()))
                        .arg(format!("/DOutputDir={}", abs(&dist).display()))
                        .arg(format!("/DOutputName={name}-setup"))
                        .arg(Path::new("packaging").join("windows").join("slicecraft.iss")),
                )?;
                out.push(setup);
            }
            None => println!(
                "Inno Setup not found (set ISCC to ISCC.exe): no installer, the zip is ready"
            ),
        }
    }

    let mut sums = String::new();
    for f in &out {
        let bytes = std::fs::read(f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
        let file = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        sums.push_str(&format!("{}  {file}\n", hex(&sha256(&bytes))));
    }
    let sums_path =
        dist.join(format!("slicecraft-{VERSION}-{}-SHA256SUMS.txt", platform(universal)));
    write(&sums_path, sums)?;
    out.push(sums_path);
    Ok(out)
}

/// `SliceCraft.app` in `stage`, beside a link to /Applications to drag it
/// onto. The command-line tool travels inside the bundle.
fn mac_bundle(bins: &Path, stage: &Path) -> Result<(), String> {
    let app = stage.join("SliceCraft.app");
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    mkdir(&macos)?;
    mkdir(&resources)?;
    for bin in BINS {
        copy(&bins.join(bin), &macos.join(bin))?;
    }
    let plist = std::fs::read_to_string(Path::new("packaging").join("macos").join("Info.plist.in"))
        .map_err(|e| format!("cannot read packaging/macos/Info.plist.in: {e}"))?;
    write(&contents.join("Info.plist"), plist.replace("@VERSION@", VERSION))?;
    run(
        "write the icon",
        Command::new(macos.join("slicecraft"))
            .arg("--write-icon")
            .arg(resources.join("slicecraft.icns")),
    )?;
    // an ad-hoc signature, so Apple Silicon runs it; not notarized
    run(
        "sign the bundle (ad hoc)",
        Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&app),
    )?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("/Applications", stage.join("Applications"))
        .map_err(|e| format!("cannot link /Applications: {e}"))?;
    Ok(())
}

/// Inno Setup's compiler: `ISCC` from the environment, else its usual places.
fn inno_setup() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ISCC").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    [r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe", r"C:\Program Files\Inno Setup 6\ISCC.exe"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// SHA-256 (FIPS 180-4), so checksums need no tool or crate.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate().take(16) {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (chunk, v) in out.chunks_exact_mut(4).zip(h) {
        chunk.copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
