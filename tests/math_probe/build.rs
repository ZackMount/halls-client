use std::{env, fs, path::PathBuf};
fn main() {
    let path = PathBuf::from("../../src/win64_memory.s");
    println!("cargo:rerun-if-changed={}", path.display());
    let code = fs::read_to_string(path).unwrap();
    let mut code = code
        .lines()
        .filter(|line| !line.trim().starts_with(".seh_"))
        .collect::<Vec<_>>()
        .join("\n");
    for name in ["memcpy", "memmove", "memset", "memcmp", "__chkstk"] {
        code = code.replace(name, &format!("_probe_{name}"));
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("memory.s"),
        code,
    )
    .unwrap();
}
