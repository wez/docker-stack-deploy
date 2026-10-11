use anyhow::Context;
use filenamegen::Glob;
use petgraph::prelude::DiGraphMap;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DeployFile {
    pub path: PathBuf,
    pub deploy: StackDeploy,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct StackDeploy {
    /// Name of this stack
    pub name: String,

    /// List of stacks that should be deployed before this one
    #[serde(default)]
    pub depends_on: Vec<String>,

    /// Map of environment variables that should be expanded
    /// from the keepass db when running docker compose.
    #[serde(default)]
    pub secret_env: BTreeMap<String, String>,

    // TODO: secret_file
    /// List of host names on which to run this service
    pub runs_on: Vec<String>,
}

impl DeployFile {}

/// Load stacks from the specified root and/or list of files.
/// The result is returned in dependency order, such that stacks that depend
/// on others will be ordered after those dependencies.
pub fn load_stacks(root: &str, files: &[PathBuf]) -> anyhow::Result<Vec<DeployFile>> {
    let files_specified = !files.is_empty();
    let files = if files.is_empty() {
        let glob = Glob::new("**/stack-deploy.toml")?;
        glob.walk(root)
            .into_iter()
            .map(|relative| Path::new(root).join(relative))
            .collect()
    } else {
        files.to_vec()
    };
    let hostname = gethostname::gethostname()
        .to_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "localhost".to_string());
    println!("my hostname is {hostname}");

    let mut stacks = BTreeMap::new();

    for path in files {
        let toml_text =
            std::fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))?;
        let deploy: StackDeploy = toml::from_str(&toml_text)
            .with_context(|| format!("failed to parse {path:?} as toml"))?;
        println!("{deploy:#?}");

        if deploy.runs_on.contains(&hostname) || deploy.runs_on.contains(&"*".to_string()) {
            anyhow::ensure!(
                !stacks.contains_key(&deploy.name),
                "multiple stacks have the same name {}",
                deploy.name
            );

            stacks.insert(
                deploy.name.to_string(),
                DeployFile {
                    path: path.to_path_buf(),
                    deploy,
                },
            );
        } else {
            log::info!(
                "Skipping {path:?} because my hostname {hostname} is not in runs_on: {:?}",
                deploy.runs_on
            );
        }
    }

    let mut graph = DiGraphMap::new();
    for (name, entry) in stacks.iter() {
        graph.add_node(name);
        for dep in &entry.deploy.depends_on {
            if !stacks.contains_key(dep) {
                if files_specified {
                    anyhow::bail!("{name} depends on {dep}, but {dep} is not present in any of the specified stack deploy files");
                }
                anyhow::bail!(
                    "{name} depends on {dep}, but {dep} is not present in any stack deploy file"
                );
            }
            graph.add_edge(name, dep, ());
        }
    }

    let mut sorted = petgraph::algo::toposort(&graph, None)
        .map_err(|err| anyhow::anyhow!("Dependency cycle detected for {}", err.node_id()))?;

    // Reverse the order, so that it is sequenced from ~start to finish
    sorted.reverse();

    let mut result = vec![];
    for name in sorted {
        match stacks.get(name).cloned() {
            Some(entry) => result.push(entry),
            None if files_specified => {
                anyhow::bail!("dependency {name} was not found in the list of files provided")
            }
            None => {
                anyhow::bail!("dependency {name} was not found in any of the stack-deploy files")
            }
        }
    }
    Ok(result)
}
