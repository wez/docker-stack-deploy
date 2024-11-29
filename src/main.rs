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
    kdbx: Option<String>,

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
    Run {
        /// Local path into which the repo should be cloned
        #[arg(long)]
        repo_dir: String,

        /// URL from which the repo should be cloned if provided
        #[arg(long)]
        repo_url: Option<String>,

        /// How many seconds to wait between checking the repo for updates. 0 to disable.
        #[arg(long, default_value = "300")]
        poll_interval: u64,
    },
    Bootstrap {
        /// Where to place the compose.yml and .env
        #[arg(long)]
        project_dir: String,

        /// The repo that should be cloned
        #[arg(long)]
        git_url: String,

        /// The git username to use
        #[arg(long, default_value = "oauth2")]
        git_username: String,

        /// How many seconds between git pulls
        #[arg(long, default_value = "300")]
        poll_interval: u32,
    },
}

impl Args {
    fn open_kdbx(&self) -> anyhow::Result<KeePassDB> {
        let kdbx = self
            .kdbx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no --kdbx file was specified"))?;
        self.open_kdbx_path(&kdbx)
    }

    fn open_kdbx_path(&self, path: &str) -> anyhow::Result<KeePassDB> {
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

        KeePassDB::open_with_password(path, &password)
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

fn run_deploy(args: &Args, repo_dir: &str) -> anyhow::Result<()> {
    let secrets_path = format!("{repo_dir}/.secrets.kdbx");
    let db = args.open_kdbx_path(&secrets_path)?;

    let sorted = load_stacks(repo_dir, &[])?;

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
                    log::error!("{path} not found in {:?}", args.kdbx);
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
        Command::Run {
            repo_dir,
            repo_url,
            poll_interval,
        } => {
            let interval = std::time::Duration::from_secs(*poll_interval);
            let mut first_run = true;

            loop {
                match repo_url {
                    Some(repo_url) => {
                        let hash = clone_or_update(repo_url, repo_dir)?;
                        log::debug!("hash is {hash:?}");
                        if hash.updated() || first_run {
                            log::info!("Running a deploy {hash:?}");
                            if let Err(err) = run_deploy(&args, repo_dir) {
                                log::error!("Error running deploy: {err:#}");
                            }
                        }
                        first_run = false;
                    }
                    None => {
                        log::info!("Running a deploy");
                        if let Err(err) = run_deploy(&args, repo_dir) {
                            log::error!("Error running deploy: {err:#}");
                        }
                    }
                }

                // Disable polling if the interval is 0
                if *poll_interval == 0 {
                    break;
                }

                std::thread::sleep(interval);
            }
        }
        Command::Bootstrap {
            project_dir,
            git_url,
            git_username,
            poll_interval,
        } => {
            std::fs::create_dir_all(project_dir)
                .with_context(|| format!("failed to create_dir_all {project_dir}"))?;

            let github_token = rpassword::prompt_password("Github Token:")?;
            let db_password = rpassword::prompt_password("KeePass Passphrase:")?;

            let compose_yml = include_str!("../compose.yml");
            let compose_file = format!("{project_dir}/compose.yml");
            std::fs::write(&compose_file, compose_yml)
                .with_context(|| format!("failed to write {compose_file}"))?;
            let env_file = format!("{project_dir}/.env");
            std::fs::write(
                &env_file,
                format!(
                    "GITHUB_URL=\"{git_url}\"\n\
                    GITHUB_USERNAME=\"{git_username}\"\n\
                    GITHUB_TOKEN=\"{github_token}\"\n\
                    STACK_KDBX_PASS=\"{db_password}\"\n\
                    POLL_INTERVAL=\"{poll_interval}\"\n"
                ),
            )
            .with_context(|| format!("failed to write {env_file}"))?;

            let mut cmd = std::process::Command::new("docker");
            cmd.args(["compose", "up", "--remove-orphans", "--detach", "--wait"]);
            cmd.current_dir(project_dir);

            let status = cmd
                .status()
                .with_context(|| format!("failed to run docker compose up in {project_dir}"))?;
            anyhow::ensure!(status.success(), "exit status is {status:?}");
        }
    }

    Ok(())
}

fn getenv(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("env var {name} not found"))
}

#[derive(Debug)]
#[allow(unused)]
enum RepoUpdateStatus {
    Cloned(String),
    Updated(String),
    Same(String),
}

impl RepoUpdateStatus {
    pub fn updated(&self) -> bool {
        match self {
            Self::Cloned(_) | Self::Updated(_) => true,
            Self::Same(_) => false,
        }
    }
}

fn get_repo_commit_hash(repo_dir: &str) -> anyhow::Result<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo_dir);
    cmd.args(["rev-parse", "HEAD"]);
    let output = cmd
        .output()
        .with_context(|| format!("failed to get current commit hash of git repo {repo_dir}"))?;
    anyhow::ensure!(
        output.status.success(),
        "exit status is {:?}",
        output.status
    );

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn clone_or_update(repo_url: &str, repo_dir: &str) -> anyhow::Result<RepoUpdateStatus> {
    let dot_git = format!("{repo_dir}/.git");

    let recreate = match std::fs::metadata(&dot_git) {
        Ok(meta) => !meta.is_dir(),
        Err(err) => {
            log::warn!("Error getting metadata for {dot_git}: {err:#}");
            true
        }
    };

    let mut cmd = std::process::Command::new("git");
    // TODO: if we have the repo checked out, we could try to read current
    // versions of these creds from the secrets file, which would allow
    // managing token expiration without redeploying the redeployer.
    let username = getenv("GITHUB_USERNAME")?;
    let password = getenv("GITHUB_TOKEN")?;

    // We want to avoid baking the PAT from the time we clone the repo
    // into the repo so that we can update the token over time.
    // These ad-hoc config overrides facilitate passing in the creds
    // <https://stackoverflow.com/a/77199818/149111>
    cmd.args(["-c", &format!("credential.username={username}")]);
    cmd.args([
        "-c",
        "credential.helper=!f(){ test \"$1\" = get && echo \"password=${GITHUB_TOKEN}\"; }; f",
    ]);
    cmd.env("GITHUB_TOKEN", password);

    let mut hash_before = None;

    if recreate {
        if let Err(err) = std::fs::remove_dir_all(&repo_dir) {
            log::warn!("Error removing {repo_dir}: {err:#}");
        }

        cmd.args(["clone", &repo_url, repo_dir]);
    } else {
        hash_before = get_repo_commit_hash(repo_dir).ok();

        cmd.current_dir(repo_dir);
        cmd.args(["pull", "--rebase"]);
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to update git repo {repo_dir} from {repo_url}"))?;
    anyhow::ensure!(status.success(), "exit status is {status:?}");

    let hash_after = get_repo_commit_hash(repo_dir)?;

    Ok(match (hash_before, hash_after) {
        (Some(before), after) if before == after => RepoUpdateStatus::Same(after),
        (Some(_before), after) => RepoUpdateStatus::Updated(after),
        (None, after) => RepoUpdateStatus::Cloned(after),
    })
}
