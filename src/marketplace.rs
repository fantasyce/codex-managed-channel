use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

pub fn managed_marketplace_root(home: &Path) -> PathBuf {
    env::var_os("CODEX_MANAGED_PERSONAL_MARKETPLACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share/codex-managed-channel/personal-marketplace"))
}

pub fn managed_bundled_marketplace_root(home: &Path) -> PathBuf {
    env::var_os("CODEX_MANAGED_BUNDLED_MARKETPLACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home.join(".local/share/codex-managed-channel/openai-bundled-marketplace")
        })
}

pub fn validate_marketplace(root: &Path) -> Result<()> {
    validate_named_marketplace(root, "personal")
}

pub fn prepare_marketplace_from_cache(
    root: &Path,
    cache_root: &Path,
    marketplace_name: &str,
) -> Result<()> {
    let mut names = match fs::read_dir(cache_root) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        Err(error)
            if marketplace_name == "personal" && error.kind() == std::io::ErrorKind::NotFound =>
        {
            Vec::new()
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("plugin cache is unavailable: {marketplace_name}"));
        }
    };
    names.sort();

    let plugins = names
        .into_iter()
        .filter(|name| {
            fs::read_dir(cache_root.join(name)).is_ok_and(|versions| {
                versions
                    .filter_map(|entry| entry.ok())
                    .any(|version| version.path().join(".codex-plugin/plugin.json").is_file())
            })
        })
        .map(|name| {
            json!({
                "name": name,
                "source": {"source": "local", "path": format!("./plugins/{name}")},
                "policy": {"installation": "AVAILABLE", "authentication": "ON_USE"}
            })
        })
        .collect::<Vec<_>>();
    if marketplace_name == "openai-bundled"
        && !plugins.iter().any(|plugin| {
            plugin.get("name").and_then(Value::as_str) == Some("unified-computer-use")
        })
    {
        bail!("installed unified-computer-use plugin is unavailable");
    }

    let catalog_dir = root.join(".agents/plugins");
    fs::create_dir_all(&catalog_dir)?;
    fs::create_dir_all(root.join("plugins"))?;
    let destination = catalog_dir.join("marketplace.json");
    let temporary = catalog_dir.join(format!(".marketplace.{}.tmp", std::process::id()));
    let catalog = json!({
        "name": marketplace_name,
        "interface": {"displayName": marketplace_name},
        "plugins": plugins
    });
    fs::write(&temporary, serde_json::to_vec_pretty(&catalog)?)?;
    fs::rename(&temporary, &destination)?;
    refresh_marketplace_links(root, cache_root)
}

pub fn refresh_marketplace_links(root: &Path, cache_root: &Path) -> Result<()> {
    let catalog_path = root.join(".agents/plugins/marketplace.json");
    let catalog: Value = serde_json::from_slice(&fs::read(&catalog_path).with_context(|| {
        format!(
            "managed marketplace catalog is missing at {}",
            catalog_path.display()
        )
    })?)
    .with_context(|| {
        format!(
            "managed marketplace catalog is invalid at {}",
            catalog_path.display()
        )
    })?;
    let plugins = catalog
        .get("plugins")
        .and_then(Value::as_array)
        .context("managed marketplace has no plugins")?;

    for plugin in plugins {
        let name = plugin
            .get("name")
            .and_then(Value::as_str)
            .context("managed marketplace plugin is missing name")?;
        let plugin_cache = cache_root.join(name);
        let mut installed = fs::read_dir(&plugin_cache)
            .with_context(|| format!("installed plugin is missing: {name}"))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.join(".codex-plugin/plugin.json").is_file())
            .collect::<Vec<_>>();
        installed.sort();
        let newest = installed
            .pop()
            .with_context(|| format!("installed plugin is missing: {name}"))?;

        let link = root.join("plugins").join(name);
        fs::create_dir_all(link.parent().context("marketplace link has no parent")?)?;
        if let Ok(metadata) = fs::symlink_metadata(&link)
            && !metadata.file_type().is_symlink()
        {
            bail!(
                "refusing to replace non-symlink marketplace entry: {}",
                link.display()
            );
        }
        let temporary = link.with_extension(format!("link.{}.tmp", std::process::id()));
        match fs::symlink_metadata(&temporary) {
            Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(&temporary)?,
            Ok(_) => bail!(
                "refusing to replace non-symlink marketplace temporary entry: {}",
                temporary.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        symlink(&newest, &temporary)?;
        fs::rename(&temporary, &link)?;
    }
    Ok(())
}

pub fn validate_named_marketplace(root: &Path, expected_name: &str) -> Result<()> {
    let catalog_path = root.join(".agents/plugins/marketplace.json");
    let catalog = std::fs::read(&catalog_path).with_context(|| {
        format!(
            "managed personal marketplace catalog is missing at {}",
            catalog_path.display()
        )
    })?;
    let value: Value = serde_json::from_slice(&catalog).with_context(|| {
        format!(
            "managed personal marketplace catalog is invalid at {}",
            catalog_path.display()
        )
    })?;
    if value.get("name").and_then(Value::as_str) != Some(expected_name) {
        bail!("managed marketplace must be named {expected_name}");
    }
    let plugins = value
        .get("plugins")
        .and_then(Value::as_array)
        .context("managed personal marketplace has no plugins")?;
    for plugin in plugins {
        let name = plugin
            .get("name")
            .and_then(Value::as_str)
            .context("managed personal marketplace plugin is missing name")?;
        let source = plugin
            .pointer("/source/path")
            .and_then(Value::as_str)
            .with_context(|| {
                format!("managed personal marketplace plugin {name} is missing source.path")
            })?;
        let manifest = root.join(source).join(".codex-plugin/plugin.json");
        if !manifest.is_file() {
            bail!(
                "managed personal marketplace plugin {name} is unavailable at {}",
                manifest.display()
            );
        }
    }
    Ok(())
}

pub fn config_override(root: &Path) -> Result<String> {
    let root = root
        .to_str()
        .context("managed personal marketplace path is not UTF-8")?;
    Ok(format!(
        "marketplaces.personal.source={}",
        serde_json::to_string(root)?
    ))
}
