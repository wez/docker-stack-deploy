use crate::deploy_file::*;
use crate::secrets::*;
use anyhow::Context;
use clap::Parser;
use log::LevelFilter;
use std::path::{Path, PathBuf};

mod deploy_file;
mod secrets;

#[derive(Parser)]
struct Args {
    /// Path to a KeePass .kdbx file containing secrets
    #[arg(long)]
    kdbx: String,

    /// Password that can be used to decrypt the kdbx file
    #[arg(long)]
    password: Option<String>,

    /// Prompt for missing information
    #[arg(long)]
    interactive: bool,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Parser)]
enum Command {
    GetSecret {
        path: String,
    },
    StackDeploy {
        /// Path to the root of the project.
        /// This path will be recursively searched for stack-deploy.toml
        /// files
        #[arg(long, default_value = ".")]
        root: String,

        /// Instead of searching for a deploy file, specify its path.
        /// Can be used multiple times
        #[arg(long = "file")]
        files: Vec<PathBuf>,
    },
    StackStop {
        /// Path to the root of the project.
        /// This path will be recursively searched for stack-deploy.toml
        /// files
        #[arg(long, default_value = ".")]
        root: String,

        /// Instead of searching for a deploy file, specify its path.
        /// Can be used multiple times
        #[arg(long = "file")]
        files: Vec<PathBuf>,
    },
}

impl Args {
    fn open_kdbx(&self) -> anyhow::Result<KeePassDB> {
        let password = if let Some(pwd) = self.password.as_ref().map(Clone::clone) {
            pwd
        } else if let Ok(s) = std::env::var("STACK_KDBX_PASS") {
            s
        } else if self.interactive {
            rpassword::prompt_password("Password:")?
        } else {
            anyhow::bail!(
                "Missing --password and $STACK_KDBX_PASS env var value \
                and --interactive is not set"
            );
        };

        KeePassDB::open_with_password(&self.kdbx, &password)
    }
}

fn do_compose_down(path: &Path) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("docker");
    cmd.args(["compose", "down", "--remove-orphans"]);
    cmd.current_dir(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("path {path:?} has no parent!?"))?,
    );

    let status = cmd
        .status()
        .with_context(|| format!("failed to run docker compose down in directory of {path:?}"))?;
    anyhow::ensure!(status.success(), "exit status is {status:?}");
    Ok(())
}

fn do_compose_up(db: &KeePassDB, path: &Path, deploy: &StackDeploy) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("docker");
    cmd.args(["compose", "up", "--remove-orphans", "--detach", "--wait"]);
    cmd.current_dir(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("path {path:?} has no parent!?"))?,
    );

    let mut failed = false;
    for (k, v) in deploy.secret_env.iter() {
        match db.resolve_value(&v) {
            Some(v) => {
                cmd.env(k, v);
            }
            None => {
                log::error!("secret_env {k}: {v} was not found in database");
                failed = true;
            }
        }
    }

    anyhow::ensure!(
        !failed,
        "Cannot deploy {path:?} because of the errors above"
    );

    let status = cmd
        .status()
        .with_context(|| format!("failed to run docker compose up in directory of {path:?}"))?;
    anyhow::ensure!(status.success(), "exit status is {status:?}");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    env_logger::builder().filter_level(LevelFilter::Info).init();

    match &args.cmd {
        Command::GetSecret { path } => {
            let db = args.open_kdbx()?;
            match db.resolve_value(&path) {
                Some(v) => {
                    println!("{v}");
                }
                None => {
                    log::error!("{path} not found in {}", args.kdbx);
                    std::process::exit(1);
                }
            }
        }
        Command::StackDeploy { root, files } => {
            let db = args.open_kdbx()?;
            let sorted = load_stacks(root, files)?;

            for entry in sorted {
                match do_compose_up(&db, &entry.path, &entry.deploy) {
                    Ok(()) => {
                        log::info!("Deployed {:?}!", entry.path);
                    }
                    Err(err) => {
                        log::error!("Failed to deploy {:?}: {err:#}", entry.path);
                    }
                }
            }
        }
        Command::StackStop { root, files } => {
            let mut sorted = load_stacks(root, files)?;
            // Go in reverse order when stopping
            sorted.reverse();

            for entry in sorted {
                match do_compose_down(&entry.path) {
                    Ok(()) => {
                        log::info!("Deployed {:?}!", entry.path);
                    }
                    Err(err) => {
                        log::error!("Failed to deploy {:?}: {err:#}", entry.path);
                    }
                }
            }
        }
    }

    Ok(())
}
