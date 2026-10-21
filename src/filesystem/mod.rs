//! Acesso a disco compartilhado pela IDE: pasta de configuracao e gravacao atomica.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "Argila";

/// `%APPDATA%\Argila` no Windows; `~/.config/Argila` como alternativa.
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

/// Grava em um temporario na mesma pasta e renomeia por cima do destino.
///
/// Se o processo cair no meio da gravacao, o arquivo original continua intacto.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "caminho sem nome de arquivo")
    })?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_and_replaces_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub").join("a.txt");
        atomic_write(&target, b"primeiro").unwrap();
        atomic_write(&target, b"segundo").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"segundo");
        let leftovers: Vec<_> = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporarios esquecidos: {leftovers:?}"
        );
    }
}
