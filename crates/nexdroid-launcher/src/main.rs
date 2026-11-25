// SPDX-License-Identifier: GPL-3.0-or-later
//! Small native entry point so file managers launch NexDroid as a program.
use std::{os::unix::process::CommandExt, path::Path, process::Command};

fn entry(executable: &Path) -> std::io::Result<std::path::PathBuf> {
    let parent = executable.parent().ok_or_else(|| std::io::Error::other("No application directory"))?;
    let path = parent.join("launch");
    if !path.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound,
            "NexDroid bundle is incomplete: keep its runtime and launch file beside the executable"));
    }
    Ok(path)
}

fn main() {
    let result = std::env::current_exe().and_then(|exe| entry(&exe)).map(|script|
        Command::new("/bin/bash").arg(script).args(std::env::args_os().skip(1)).exec());
    let error = match result { Ok(error) | Err(error) => error };
    eprintln!("NexDroid: {error}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_bundle_reports_an_actionable_error() {
        let error = entry(Path::new("/definitely-missing-nexdroid-test/NexDroid")).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error.to_string().contains("bundle is incomplete"));
    }
}
