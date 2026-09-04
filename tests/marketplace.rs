use std::fs;
use std::os::unix::fs::symlink;

use codex_managed_channel::marketplace::{
    prepare_marketplace_from_cache, refresh_marketplace_links, validate_named_marketplace,
};

const PERSONAL_PLUGINS: [&str; 2] = ["sample-personal", "second-sample"];
const BUNDLED_PLUGINS: [&str; 3] = ["unified-computer-use", "browser", "chrome"];

#[test]
fn prepare_marketplace_builds_a_bounded_catalog_from_installed_plugins() {
    let temp = tempfile::tempdir().unwrap();
    for name in PERSONAL_PLUGINS {
        let manifest = temp.path().join(format!(
            ".codex/plugins/cache/personal/{name}/1.0.0/.codex-plugin/plugin.json"
        ));
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
        )
        .unwrap();
    }
    for name in BUNDLED_PLUGINS {
        let manifest = temp.path().join(format!(
            ".codex/plugins/cache/openai-bundled/{name}/1.0.0/.codex-plugin/plugin.json"
        ));
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            &manifest,
            format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
        )
        .unwrap();
    }

    let root = temp
        .path()
        .join(".local/share/codex-managed-channel/personal-marketplace");
    prepare_marketplace_from_cache(
        &root,
        &temp.path().join(".codex/plugins/cache/personal"),
        "personal",
    )
    .unwrap();
    assert!(root.join(".agents/plugins/marketplace.json").is_file());
    for name in PERSONAL_PLUGINS {
        let link = root.join("plugins").join(name);
        assert!(link.is_symlink(), "{} is not a symlink", link.display());
        assert!(link.join(".codex-plugin/plugin.json").is_file());
    }
    let bundled_root = temp
        .path()
        .join(".local/share/codex-managed-channel/openai-bundled-marketplace");
    prepare_marketplace_from_cache(
        &bundled_root,
        &temp.path().join(".codex/plugins/cache/openai-bundled"),
        "openai-bundled",
    )
    .unwrap();
    assert!(
        bundled_root
            .join(".agents/plugins/marketplace.json")
            .is_file()
    );
    for name in BUNDLED_PLUGINS {
        let link = bundled_root.join("plugins").join(name);
        assert!(link.is_symlink(), "{} is not a symlink", link.display());
        assert!(link.join(".codex-plugin/plugin.json").is_file());
    }
}

#[test]
fn refresh_marketplace_links_repairs_versions_pruned_by_a_desktop_update() {
    let temp = tempfile::tempdir().unwrap();
    let marketplace = temp.path().join("managed-marketplace");
    let catalog = marketplace.join(".agents/plugins/marketplace.json");
    fs::create_dir_all(catalog.parent().unwrap()).unwrap();
    fs::write(
        &catalog,
        r#"{
            "name": "openai-bundled",
            "plugins": [{
                "name": "unified-computer-use",
                "source": {"source": "local", "path": "./plugins/unified-computer-use"}
            }]
        }"#,
    )
    .unwrap();

    let cache = temp.path().join("cache/openai-bundled");
    let current = cache.join("unified-computer-use/26.901.31953");
    let manifest = current.join(".codex-plugin/plugin.json");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(&manifest, r#"{"name":"unified-computer-use"}"#).unwrap();

    let plugins = marketplace.join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    symlink(
        cache.join("unified-computer-use/26.831.21537"),
        plugins.join("unified-computer-use"),
    )
    .unwrap();

    refresh_marketplace_links(&marketplace, &cache).unwrap();

    assert_eq!(
        fs::read_link(plugins.join("unified-computer-use")).unwrap(),
        current
    );
    validate_named_marketplace(&marketplace, "openai-bundled").unwrap();
}
