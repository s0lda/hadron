use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::adapter::registry::AcpTarget;

const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// A snapshot of [`REGISTRY_URL`] compiled into the binary.
///
/// Without it the catalogue is empty on any machine that has neither a fetched cache
/// nor Zed installed — a fresh clone, a CI box, an offline laptop — and the wizard
/// falls back to the bare-binary-name guesses this whole module exists to replace.
/// Same trick every other must-survive-a-clone asset here uses. Refresh with:
/// `curl -sSL <REGISTRY_URL> > crates/hadron-gluon/src/adapter/registry/acp_registry.json`
const BUNDLED_REGISTRY: &str = include_str!("acp_registry.json");

/// The bundled snapshot, parsed. The last resort behind the fetched cache and Zed's copy.
pub fn bundled_registry() -> Option<AcpRegistryData> {
    parse_registry_json(BUNDLED_REGISTRY).ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRegistryData {
    pub version: String,
    #[serde(default)]
    pub agents: Vec<AcpRegistryAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRegistryAgent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub distribution: AcpRegistryDistribution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRegistryDistribution {
    #[serde(default)]
    pub npx: Option<NpxDistribution>,
    #[serde(default)]
    pub uvx: Option<UvxDistribution>,
    #[serde(default)]
    pub binary: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpxDistribution {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UvxDistribution {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    BinaryNotSupported,
    NoSupportedDistribution(String),
    AgentNotFound(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::BinaryNotSupported => {
                write!(
                    f,
                    "Agent binary distribution not supported yet (requires manual command configuration)"
                )
            }
            RegistryError::NoSupportedDistribution(id) => {
                write!(
                    f,
                    "No supported distribution mechanism (npx or uvx) found for agent '{id}'"
                )
            }
            RegistryError::AgentNotFound(id) => {
                write!(f, "Agent '{id}' not found in registry")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// Pure function: parse a JSON string into `AcpRegistryData`.
pub fn parse_registry_json(json_str: &str) -> Result<AcpRegistryData, serde_json::Error> {
    serde_json::from_str(json_str)
}

/// Pure function: resolve a `AcpRegistryAgent`'s distribution into an `AcpTarget`.
/// Scoped to `npx` + `uvx` only. `binary` returns `Err(RegistryError::BinaryNotSupported)`.
pub fn resolve_agent_command(agent: &AcpRegistryAgent) -> Result<AcpTarget, RegistryError> {
    if let Some(npx) = &agent.distribution.npx {
        let mut args = vec!["-y".to_string(), npx.package.clone()];
        args.extend(npx.args.clone());
        let env = npx.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        Ok(AcpTarget {
            program: "npx".to_string(),
            args,
            env,
        })
    } else if let Some(uvx) = &agent.distribution.uvx {
        let mut args = vec![uvx.package.clone()];
        args.extend(uvx.args.clone());
        let env = uvx.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        Ok(AcpTarget {
            program: "uvx".to_string(),
            args,
            env,
        })
    } else if agent.distribution.binary.is_some() {
        Err(RegistryError::BinaryNotSupported)
    } else {
        Err(RegistryError::NoSupportedDistribution(agent.id.clone()))
    }
}

/// This platform's key in a `binary` distribution map (`linux-x86_64`,
/// `darwin-aarch64`, `windows-x86_64`, …) — the registry's own naming, which is
/// `<os>-<arch>` with macOS spelled `darwin`.
fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// The **argv** a `binary`-distribution agent documents for ACP mode, without
/// downloading anything: `goose` publishes `{"cmd": "./goose", "args": ["acp"]}`, so
/// the subcommand is knowable even though the archive is not something Hadron will
/// fetch and execute.
///
/// This exists because the alternative was worse in both directions. Offering a bare
/// `goose` is a row that CONNECTS AND HANGS — an agent launched without its ACP flag
/// starts its interactive TUI and never answers `initialize` (documented on the
/// `copilot` preset, which was fixed the same way). Offering nothing greys out eight
/// agents the human may well have installed themselves. Taking the publisher's args
/// and keeping our own program name — the CLI on `PATH` — is the only option that is
/// both honest and useful.
///
/// This platform's entry when present, else any entry: the args are the agent's own
/// ACP subcommand and are identical across platforms in every row of the snapshot,
/// while `cmd` (which we deliberately ignore — it points inside the archive) is not.
pub fn binary_acp_args(agent: &AcpRegistryAgent) -> Option<Vec<String>> {
    let map = agent.distribution.binary.as_ref()?.as_object()?;
    let entry = map.get(&platform_key()).or_else(|| map.values().next())?;
    let args = entry.get("args")?.as_array()?;
    Some(args.iter().filter_map(|a| a.as_str().map(str::to_string)).collect())
}

/// Pure function: find and resolve an agent by id or vendor from `AcpRegistryData`.
pub fn resolve_from_registry_data(
    data: &AcpRegistryData,
    id: &str,
) -> Result<AcpTarget, RegistryError> {
    // `super::vendor_for` is the ONE place that maps a published registry id onto our
    // vendor key. Re-spelling its `-acp`-stripping rule here is what let the two drift:
    // `available_agents` and this lookup disagreed about `github-copilot-cli`, so the
    // wizard listed Copilot twice and `for_vendor("copilot")` still found nothing.
    let agent = data
        .agents
        .iter()
        .find(|a| a.id == id || super::vendor_for(&a.id) == id)
        .ok_or_else(|| RegistryError::AgentNotFound(id.to_string()))?;
    resolve_agent_command(agent)
}

/// Path to the cached ACP registry file: `~/.hadron/acp_registry.json`.
pub fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".hadron").join("acp_registry.json"))
}

/// Offline fallback path: `~/.local/share/zed/external_agents/registry/registry.json`.
pub fn fallback_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("zed")
            .join("external_agents")
            .join("registry")
            .join("registry.json"),
    )
}

/// Read cached registry data from disk (`~/.hadron/acp_registry.json` or fallback).
pub fn load_cached_registry() -> Option<AcpRegistryData> {
    if let Some(cp) = cache_path() {
        if let Ok(contents) = fs::read_to_string(&cp) {
            if let Ok(data) = parse_registry_json(&contents) {
                return Some(data);
            }
        }
    }
    if let Some(fp) = fallback_path() {
        if let Ok(contents) = fs::read_to_string(&fp) {
            if let Ok(data) = parse_registry_json(&contents) {
                return Some(data);
            }
        }
    }
    // Zed's copy is a convenience, not a dependency — a box without it still has a
    // catalogue.
    bundled_registry()
}

/// Save registry data to `~/.hadron/acp_registry.json`.
pub fn save_cached_registry(data: &AcpRegistryData) -> Result<(), std::io::Error> {
    let cp = cache_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No home dir"))?;
    if let Some(parent) = cp.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)?;
    let tmp = cp.with_extension("tmp");
    fs::write(&tmp, json)?;
    fs::rename(tmp, cp)?;
    Ok(())
}

/// Fetch latest registry from CDN via `curl` subprocess, cache to `~/.hadron/acp_registry.json`,
/// falling back to cached disk data if offline or fetch fails.
pub fn fetch_and_cache_registry() -> Option<AcpRegistryData> {
    let output = std::process::Command::new("curl")
        .args(["-sSL", "--max-time", "5", REGISTRY_URL])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            if let Ok(text) = std::str::from_utf8(&out.stdout) {
                if let Ok(data) = parse_registry_json(text) {
                    let _ = save_cached_registry(&data);
                    return Some(data);
                }
            }
        }
    }
    load_cached_registry()
}
