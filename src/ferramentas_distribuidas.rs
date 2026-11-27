//! Ferramentas de distribuição: hashes ancorados no aplicativo, sem rede ou fallback.
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub fn verificar_sha256(path: &Path, esperado: &str) -> io::Result<()> {
    rejeitar_redirecionamentos(path)?;
    let file = File::open(path)?;
    verificar_arquivo(file, esperado)
}

fn verificar_arquivo(mut file: File, esperado: &str) -> io::Result<()> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != esperado {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Integridade SHA-256 inválida",
        ));
    }
    Ok(())
}

/// Recusa symlinks e reparse points (inclusive junctions) nos caminhos de ferramentas.
fn rejeitar_redirecionamentos(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        let metadata = match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        #[cfg(windows)]
        let redirecionado = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let redirecionado = metadata.file_type().is_symlink();
        if redirecionado {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Caminho de ferramenta contém redirecionamento",
            ));
        }
    }
    Ok(())
}

/// Mantém arquivo e diretórios abertos sem FILE_SHARE_DELETE; arquivo também sem SHARE_WRITE.
/// Os handles ficam vivos durante o processo filho e durante toda a vida do executor.
#[cfg(all(windows, feature = "windows-bundle"))]
pub fn proteger_execucao(path: &Path) -> io::Result<Vec<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    rejeitar_redirecionamentos(path)?;
    let hash = match path.file_name().and_then(|n| n.to_str()) {
        Some("ffmpeg.exe") => FFMPEG_HASH,
        Some("ffprobe.exe") => FFPROBE_HASH,
        _ => return Err(io::Error::other("Ferramenta não reconhecida")),
    };
    let mut guards = proteger_diretorios(
        path.parent()
            .ok_or_else(|| io::Error::other("Caminho sem diretório"))?,
    )?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x0020_0000)
        .open(path)?;
    rejeitar_redirecionamentos(path)?;
    verificar_arquivo(file.try_clone()?, hash)?;
    guards.push(file);
    Ok(guards)
}

#[cfg(all(windows, any(feature = "windows-bundle", test)))]
fn proteger_diretorios(base: &Path) -> io::Result<Vec<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    let mut guards = Vec::new();
    let ancestors: Vec<_> = base.ancestors().collect();
    for directory in ancestors.into_iter().rev() {
        guards.push(
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(directory)?,
        );
        rejeitar_redirecionamentos(directory)?;
    }
    Ok(guards)
}

#[cfg(feature = "windows-bundle")]
include!(concat!(env!("OUT_DIR"), "/tools.rs"));

#[cfg(feature = "windows-bundle")]
pub fn resolver(nome: &str) -> io::Result<std::path::PathBuf> {
    let hash = match nome {
        "ffmpeg" => FFMPEG_HASH,
        "ffprobe" => FFPROBE_HASH,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Ferramenta desconhecida",
            ));
        }
    };
    #[cfg(not(feature = "embedded-tools"))]
    let path = std::env::current_exe()?
        .parent()
        .ok_or_else(|| io::Error::other("Diretório do aplicativo indisponível"))?
        .join("ferramentas")
        .join(format!("{nome}.exe"));
    #[cfg(feature = "embedded-tools")]
    let path = {
        let base = dirs::data_local_dir()
            .ok_or_else(|| io::Error::other("Pasta local do usuário indisponível"))?
            .join("DivisorVideo")
            .join("ferramentas")
            .join(hash);
        let bytes = if nome == "ffmpeg" {
            FFMPEG_BYTES
        } else {
            FFPROBE_BYTES
        };
        extrair_verificado(&base, nome, bytes, hash)?
    };
    verificar_sha256(&path, hash)?;
    Ok(path)
}

#[cfg(any(feature = "embedded-tools", test))]
fn extrair_verificado(
    base: &Path,
    nome: &str,
    bytes: &[u8],
    hash: &str,
) -> io::Result<std::path::PathBuf> {
    use fs2::FileExt;
    use std::io::Write;
    rejeitar_redirecionamentos(base)?;
    std::fs::create_dir_all(base)?;
    rejeitar_redirecionamentos(base)?;
    #[cfg(windows)]
    let _diretorios = proteger_diretorios(base)?;
    rejeitar_redirecionamentos(&base.join("extracao.lock"))?;
    let mut options = std::fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3).custom_flags(0x0020_0000);
    }
    let lock = options
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(base.join("extracao.lock"))?;
    lock.lock_exclusive()?;
    let path = base.join(format!("{nome}.exe"));
    rejeitar_redirecionamentos(&path)?;
    if verificar_sha256(&path, hash).is_err() {
        let mut temp = tempfile::NamedTempFile::new_in(base)?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        verificar_sha256(temp.path(), hash)?;
        temp.persist(&path).map_err(|e| e.error)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hash_aceita_conteudo_correto_e_rejeita_corrupcao_e_ausencia() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.exe");
        std::fs::write(&path, b"abc").unwrap();
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        verificar_sha256(&path, hash).unwrap();
        std::fs::write(&path, b"alterado").unwrap();
        assert!(verificar_sha256(&path, hash).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(verificar_sha256(&path, hash).is_err());
    }
    #[test]
    fn extracao_reutiliza_repara_e_rejeita_payload_invalido() {
        let dir = tempfile::tempdir().unwrap();
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let path = extrair_verificado(dir.path(), "ffmpeg", b"abc", hash).unwrap();
        let modified = path.metadata().unwrap().modified().unwrap();
        extrair_verificado(dir.path(), "ffmpeg", b"abc", hash).unwrap();
        assert_eq!(modified, path.metadata().unwrap().modified().unwrap());
        std::fs::write(&path, b"corrompido").unwrap();
        extrair_verificado(dir.path(), "ffmpeg", b"abc", hash).unwrap();
        verificar_sha256(&path, hash).unwrap();
        assert!(extrair_verificado(dir.path(), "ffprobe", b"invalido", hash).is_err());
        assert!(!dir.path().join("ffprobe.exe").exists());
    }

    #[test]
    fn extracao_concorrente_publica_um_arquivo_integro() {
        let dir = tempfile::tempdir().unwrap();
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| extrair_verificado(dir.path(), "ffmpeg", b"abc", hash)))
                .collect();
            for handle in handles {
                verificar_sha256(&handle.join().unwrap().unwrap(), hash).unwrap();
            }
        });
    }
    #[cfg(unix)]
    #[test]
    fn cache_recusa_links_sem_alterar_destino() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        std::fs::write(&original, b"preservar").unwrap();
        std::os::unix::fs::symlink(&original, dir.path().join("ffmpeg.exe")).unwrap();
        assert!(extrair_verificado(dir.path(), "ffmpeg", b"abc", "invalido").is_err());
        assert_eq!(std::fs::read(original).unwrap(), b"preservar");
    }
    #[cfg(all(windows, feature = "embedded-tools"))]
    #[test]
    fn windows_bloqueia_escrita_e_substituicao_enquanto_em_uso() {
        let path = resolver("ffmpeg").unwrap();
        let guards = proteger_execucao(&path).unwrap();
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::rename(&path, path.with_extension("alterado")).is_err());
        verificar_sha256(&path, FFMPEG_HASH).unwrap();
        drop(guards);
    }
}
