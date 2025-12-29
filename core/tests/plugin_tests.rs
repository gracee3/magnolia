use talisman_core::PluginLoader;

#[test]
fn test_plugin_discovery() {
    let loader = PluginLoader::new();
    let plugins = loader.discover().unwrap();

    println!("Discovered {} plugins", plugins.len());
    for plugin in &plugins {
        println!("  - {}", plugin.display());
    }
}

#[test]
fn test_plugin_loading() {
    let mut loader = PluginLoader::new();

    // Add workspace root plugins directory
    let workspace_plugins = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("plugins");
    loader.add_plugin_dir(workspace_plugins);

    unsafe {
        let count = loader.load_all().unwrap();
        println!("Loaded {} plugins", count);

        for plugin in &loader.loaded {
            println!("Plugin: {}", plugin.name());
        }

        assert!(count > 0, "Should load at least one plugin");
    }
}
