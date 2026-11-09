//! Local release engineering only. Never authenticates, uploads, or runs dot-tui.
mod build;
mod licenses;
mod publish;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Build and package native macOS arm64; never launch or upload it.
    BuildRelease {
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        offline: bool,
    },
    /// Collect verbatim license texts from target-filtered Cargo metadata JSON.
    CollectLicenses {
        metadata: PathBuf,
        output: PathBuf,
        #[arg(long)]
        inventory: Option<PathBuf>,
    },
    /// Atomically replace a local executable after signature/hash verification.
    PublishRelease {
        source: PathBuf,
        destination: PathBuf,
    },
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .context("Could not start build/verification command")?;
    if !status.success() {
        bail!("Build/verification command failed: {status}");
    }
    Ok(())
}
fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .context("Could not start metadata command")?;
    if !result.status.success() {
        bail!("Metadata command failed: {}", result.status);
    }
    String::from_utf8(result.stdout).context("Command output is not UTF-8")
}
fn main() -> Result<()> {
    match Args::parse().command {
        Task::BuildRelease { output, offline } => build::build(&root(), output, offline),
        Task::CollectLicenses {
            metadata,
            output,
            inventory,
        } => {
            let data = serde_json::from_slice(&std::fs::read(metadata)?)?;
            let (text, details) = licenses::collect(&data, &root())?;
            if let Some(path) = inventory {
                std::fs::write(
                    path,
                    format!("{}\n", serde_json::to_string_pretty(&details)?),
                )?;
            }
            std::fs::write(output, text)?;
            Ok(())
        }
        Task::PublishRelease {
            source,
            destination,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&publish::publish(&source, &destination)?)?
            );
            Ok(())
        }
    }
}
