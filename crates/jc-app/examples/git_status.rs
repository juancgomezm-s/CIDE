//! Explicit headless status inspection; no document, UI, watcher or Git writes.
use jc_app::{
    git_read::GitReader,
    git_status::{read_status, GitStatusReadOptions},
};
use std::{env, error::Error, io, path::Path};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Uso: git_status RAÍZ_ABSOLUTA_DEL_REPOSITORIO",
        )
        .into());
    }
    let reader = GitReader::new()?;
    let snapshot = read_status(
        &reader,
        Path::new(&args[0]),
        GitStatusReadOptions::default(),
        || false,
    )?;
    println!("Repository: {:?}", snapshot.repository().root);
    println!(
        "{} entries; {} logical owned bytes (not process RAM)",
        snapshot.status().entries().len(),
        snapshot.memory_bytes()
    );
    println!("Isolated Git configuration; rename detection/submodule inspection disabled.");
    for entry in snapshot.status().entries() {
        // Escape each original byte, including controls/invalid UTF-8. These
        // display strings are never interpreted as paths or passed to Git.
        print!("{:?} {}", entry.state(), escaped(entry.path()));
        if let Some(original) = entry.original_path() {
            print!(" <- {}", escaped(original));
        }
        println!();
    }
    Ok(())
}

fn escaped(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|byte| std::ascii::escape_default(*byte))
        .map(char::from)
        .collect()
}
