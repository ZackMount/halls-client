use std::{env, path::PathBuf, process::Command};
fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("{command:?}: {e}"));
    assert!(status.success(), "{command:?}: {status}");
}
fn words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for c in s.chars() {
        if Some(c) == quote {
            quote = None;
        } else if quote.is_none() && (c == '\'' || c == '"') {
            quote = Some(c);
        } else if quote.is_none() && c.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}
// Diversification only: both shares ship in the executable. This prevents a
// contiguous import blob, not extraction by an owner of the running process.
fn key_material(out: &std::path::Path) {
    println!("cargo:rerun-if-env-changed=HALLS_PROOF_KEYS");
    let path = PathBuf::from(
        env::var_os("HALLS_PROOF_KEYS")
            .expect("Set HALLS_PROOF_KEYS to a proof_keys.h generated outside the repository"),
    )
    .canonicalize()
    .expect("Read HALLS_PROOF_KEYS");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        !path.starts_with(root),
        "Keep proof keys outside the repository"
    );
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path).expect("Read proof keys");
    let name = "proof_signing_key";
    let start = source.find(name).expect("Signing key");
    let begin = start + source[start..].find('{').unwrap() + 1;
    let end = begin + source[begin..].find('}').unwrap();
    let raw: Vec<u8> = source[begin..end]
        .split(',')
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().parse().expect("Key byte"))
        .collect();
    assert_eq!(raw.len(), 104);
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let mut a = [0u8; 104];
    let mut b = [0u8; 104];
    for (i, value) in raw.iter().enumerate() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let slot = (i * 73 + 19) % 104;
        a[slot] = seed as u8;
        b[slot] = *value ^ a[slot];
    }
    let list = |v: &[u8]| v.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
    let mut generated = format!(
        "#define PROOF_SIGNING_SIZE 104\nstatic const volatile unsigned char proof_share_a[104]={{{}}};\nstatic const volatile unsigned char proof_share_b[104]={{{}}};\nstatic inline void proof_signing_material(unsigned char* out){{for(unsigned i=0;i<104;i++){{unsigned k=(i*73+19)%104;out[i]=proof_share_a[k]^proof_share_b[k];}}}}\n",
        list(&a),
        list(&b)
    );
    for public in ["proof_recipient_key", "proof_key_id"] {
        let start = source
            .find(&format!("static const unsigned char {public}"))
            .expect("Public proof material");
        let end = start + source[start..].find(';').unwrap() + 1;
        generated.push_str(&source[start..end]);
        generated.push('\n');
    }
    let material = out.join("proof_material.h");
    std::fs::write(&material, generated).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(material, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}
fn main() {
    let target = env::var("TARGET").unwrap();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let windows = target.contains("windows");
    let portable = !windows;
    if portable {
        println!("cargo:rerun-if-changed=src/proof_portable.c");
        if target.contains("apple") {
            println!("cargo:rerun-if-changed=src/proof_apple.c");
            println!("cargo:rustc-link-lib=framework=Security");
            println!("cargo:rustc-link-lib=framework=CoreFoundation");
        }
    }

    println!("cargo:rerun-if-changed=src/entity_vm.s");
    let assembly = std::fs::read_to_string("src/entity_vm.s").unwrap();
    let assembly = assembly
        .lines()
        .filter(|line| windows || !line.trim().starts_with(".seh_"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(out.join("entity_vm.s"), assembly).unwrap();
    let suffix = target.replace('-', "_");
    for name in ["CC", "CFLAGS", "AR"] {
        println!("cargo:rerun-if-env-changed={name}");
        println!("cargo:rerun-if-env-changed={name}_{suffix}");
    }
    let setting = |key: &str| env::var(format!("{key}_{suffix}")).or_else(|_| env::var(key));
    let compiler =
        setting("CC").unwrap_or_else(|_| if windows { "cl".into() } else { "cc".into() });
    key_material(&out);
    let expanded = env::var("OPT_LEVEL").as_deref() == Ok("3");
    let mut objects = Vec::new();
    for source in [
        "audio_backend",
        "terminal_backend",
        "runtime_backend",
        "proof_backend",
    ] {
        println!("cargo:rerun-if-changed=src/{source}.c");
        let object = out.join(format!("{source}.{}", if windows { "obj" } else { "o" }));
        let mut c = Command::new(&compiler);
        if windows {
            let crt = if env::var("CARGO_CFG_TARGET_FEATURE")
                .unwrap_or_default()
                .contains("crt-static")
            {
                "/MT"
            } else {
                "/MD"
            };
            c.args([
                "/nologo",
                if expanded { "/O2" } else { "/O1" },
                "/W3",
                "/std:c11",
                crt,
                "/GS-",
                "/Zl",
                "/c",
            ])
            .arg(format!("/Fo{}", object.display()));
        } else {
            c.args([
                if expanded { "-O3" } else { "-Os" },
                "-std=c11",
                "-D_DEFAULT_SOURCE",
                "-ffunction-sections",
                "-fdata-sections",
                "-c",
                "-o",
            ])
            .arg(&object);
        }
        if expanded {
            c.arg(if windows {
                "/Ob3"
            } else {
                "-finline-functions"
            });
        }
        if target.contains("apple") {
            c.args([
                "-arch",
                if target.starts_with("x86_64") {
                    "x86_64"
                } else {
                    "arm64"
                },
                "-mmacosx-version-min=13.0",
            ]);
        }
        if let Ok(flags) = setting("CFLAGS") {
            c.args(words(&flags));
        }
        c.arg(if windows {
            format!("/I{}", out.display())
        } else {
            format!("-I{}", out.display())
        });
        c.arg(format!("src/{source}.c"));
        run(&mut c);
        objects.push(object);
    }
    let archive = out.join(if windows {
        "halls_native.lib"
    } else {
        "libhalls_native.a"
    });
    if windows {
        println!("cargo:rustc-link-arg=/DEBUG:NONE");
        let ar = setting("AR").unwrap_or_else(|_| "lib".into());
        run(Command::new(ar)
            .arg("/nologo")
            .arg(format!("/OUT:{}", archive.display()))
            .args(&objects));
        println!("cargo:rustc-link-lib=winmm");
        println!("cargo:rustc-link-lib=bcrypt");
        println!("cargo:rustc-link-lib=kernel32");

        if env::var_os("CARGO_FEATURE_TOOLS").is_none() {
            println!("cargo:rustc-link-arg-bin=halls=/ENTRY:halls_entry");
            println!("cargo:rustc-link-arg-bin=halls=/NODEFAULTLIB");
            println!("cargo:rustc-link-arg-bin=halls=/SUBSYSTEM:CONSOLE");
        }
    } else {
        run(Command::new(setting("AR").unwrap_or_else(|_| "ar".into()))
            .arg("crs")
            .arg(&archive)
            .args(&objects));
        if target.contains("apple") {
            println!("cargo:rustc-link-lib=framework=AudioToolbox");
            println!("cargo:rustc-link-lib=System");
        } else {
            println!("cargo:rustc-link-lib=dl");
            println!("cargo:rustc-link-lib=pthread");
            println!("cargo:rustc-link-lib=m");
            println!("cargo:rustc-link-lib=c");
            println!("cargo:rustc-link-lib=gcc_s");
        }
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=halls_native");
    if portable && !target.contains("apple") {
        println!("cargo:rustc-link-lib=crypto");
    }
}
