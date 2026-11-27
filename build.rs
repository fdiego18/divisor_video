use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=DIVISOR_TOOLS_DIR");
    if env::var_os("CARGO_FEATURE_WINDOWS_BUNDLE").is_none() {
        return;
    }
    assert_eq!(
        env::var("TARGET").unwrap(),
        "x86_64-pc-windows-msvc",
        "Pacotes Windows exigem MSVC x64"
    );
    assert!(
        env::var("CARGO_CFG_TARGET_FEATURE")
            .unwrap_or_default()
            .split(',')
            .any(|f| f == "crt-static"),
        "Pacotes Windows exigem runtime C estático; verifique RUSTFLAGS e .cargo/config.toml"
    );
    let dir = PathBuf::from(env::var_os("DIVISOR_TOOLS_DIR").expect("Informe DIVISOR_TOOLS_DIR"));
    let mut generated = String::new();
    for name in ["ffmpeg", "ffprobe"] {
        let path = fs::canonicalize(dir.join(format!("{name}.exe")))
            .expect("Ferramenta obrigatória ausente");
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(&path).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let variable = format!("DIVISOR_{}_SHA256", name.to_uppercase());
        println!("cargo:rerun-if-env-changed={variable}");
        let expected =
            env::var(&variable).expect("Hash autenticado obrigatório na compilação do pacote");
        assert_eq!(
            hash,
            expected.to_lowercase(),
            "Ferramenta alterada entre aquisição e compilação"
        );
        generated.push_str(&format!(
            "pub const {}_HASH: &str = {:?};\n",
            name.to_uppercase(),
            hash
        ));
        if env::var_os("CARGO_FEATURE_EMBEDDED_TOOLS").is_some() {
            generated.push_str(&format!(
                "pub const {}_BYTES: &[u8] = include_bytes!({:?});\n",
                name.to_uppercase(),
                path
            ));
        }
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("tools.rs"),
        generated,
    )
    .unwrap();
}
