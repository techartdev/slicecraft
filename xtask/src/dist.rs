//! `cargo xtask dist`: build the release binaries and pack them for people to
//! download — a zip (or tar.gz off Windows) and, on Windows when Inno Setup is
//! installed, an installer — with SHA-256 checksums beside them. Everything
//! lands in `target/dist/`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Files from the repository that travel with the program.
const DOCS: [&str; 5] = ["README.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ATTRIBUTION.md"];

fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
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

pub fn dist() -> ExitCode {
    match build() {
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

fn build() -> Result<Vec<PathBuf>, String> {
    run(
        "cargo build --release",
        Command::new("cargo").args([
            "build",
            "--release",
            "-p",
            "slicecraft",
            "-p",
            "slicecraft-cli",
        ]),
    )?;
    let name = format!("slicecraft-{VERSION}-{}", platform());
    let dist = PathBuf::from("target").join("dist");
    let stage = dist.join(&name);
    if stage.exists() {
        std::fs::remove_dir_all(&stage)
            .map_err(|e| format!("cannot clear {}: {e}", stage.display()))?;
    }
    std::fs::create_dir_all(stage.join("examples"))
        .map_err(|e| format!("cannot create {}: {e}", stage.display()))?;
    let release = PathBuf::from("target").join("release");
    for bin in ["slicecraft", "slicecraft-cli"] {
        copy(&release.join(exe(bin)), &stage.join(exe(bin)))?;
    }
    for doc in DOCS {
        copy(Path::new(doc), &stage.join(doc))?;
    }
    copy(Path::new("examples/cube.stl"), &stage.join("examples").join("cube.stl"))?;
    // the icon, drawn by the program itself
    let ico = stage.join("slicecraft.ico");
    run(
        "write the icon",
        Command::new(release.join(exe("slicecraft"))).arg("--write-icon").arg(&ico),
    )?;

    let mut out = Vec::new();
    // the archive: zip on Windows (tar there writes zip), tar.gz elsewhere
    let archive = if cfg!(windows) {
        dist.join(format!("{name}.zip"))
    } else {
        dist.join(format!("{name}.tar.gz"))
    };
    if archive.exists() {
        std::fs::remove_file(&archive)
            .map_err(|e| format!("cannot replace {}: {e}", archive.display()))?;
    }
    let flags = if cfg!(windows) { "-a -cf" } else { "-czf" };
    let mut tar = Command::new("tar");
    tar.args(flags.split(' '))
        .arg(archive.file_name().unwrap_or_default())
        .arg(&name)
        .current_dir(&dist);
    run("pack the archive", &mut tar)?;
    out.push(archive);

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
    let sums_path = dist.join(format!("slicecraft-{VERSION}-{}-SHA256SUMS.txt", platform()));
    std::fs::write(&sums_path, sums)
        .map_err(|e| format!("cannot write {}: {e}", sums_path.display()))?;
    out.push(sums_path);
    Ok(out)
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
