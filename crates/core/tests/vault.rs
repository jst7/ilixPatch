//! D60: secrets encrypted at rest. One test (the vault state is process-wide).

use std::fs;

use ilixpatch_core::environments::{
    EnvStore, EnvVariable, EnvironmentDoc, load_globals, save_globals,
};
use ilixpatch_core::project;
use ilixpatch_core::storage::StorageError;
use ilixpatch_core::vault::{self, DEFAULT_PASSWORD, Vault, VaultError};

fn secret(key: &str, value: &str) -> EnvVariable {
    EnvVariable {
        secret: true,
        ..EnvVariable::new(key, value)
    }
}

fn dev() -> EnvironmentDoc {
    EnvironmentDoc {
        name: "Dev".into(),
        variables: vec![
            EnvVariable::new("base", "http://dev"),
            secret("token", "s3cret"),
        ],
        color: None,
        icon: None,
    }
}

fn value(doc: &EnvironmentDoc, key: &str) -> String {
    doc.variables
        .iter()
        .find(|v| v.key == key)
        .unwrap()
        .value
        .clone()
}

#[test]
fn secrets_are_encrypted_migrated_locked_and_unlocked() {
    let t = tempfile::tempdir().unwrap();
    let config = t.path().join("config");
    let root = t.path().join("proj");
    fs::create_dir_all(&root).unwrap();
    project::init_project(&root).unwrap();
    let envs = root.join(".ilixpatch/environments");

    // A plain-text secret file from before D60.
    fs::create_dir_all(&envs).unwrap();
    fs::write(
        envs.join("old.toml"),
        "name = \"Old\"\n[[variables]]\nkey = \"pwd\"\nsecret = true\n",
    )
    .unwrap();
    fs::write(
        envs.join("old.secret.toml"),
        "[variables]\npwd = \"plain\"\n",
    )
    .unwrap();

    assert!(
        vault::install(&config, None).unwrap(),
        "default password opens"
    );
    assert!(vault::uses_default_password(&config));

    // Project environment: the secret file is encrypted.
    let store = EnvStore::project(&root);
    store.save(&dev(), None).unwrap();
    let text = fs::read_to_string(envs.join("dev.secret.toml")).unwrap();
    assert!(
        text.contains("encrypted = \"ilxv1:") && !text.contains("s3cret"),
        "{text}"
    );
    assert_eq!(value(&store.get("Dev").unwrap(), "token"), "s3cret");

    // Migration: reading the old file rewrites it encrypted.
    assert_eq!(value(&store.get("Old").unwrap(), "pwd"), "plain");
    let old = fs::read_to_string(envs.join("old.secret.toml")).unwrap();
    assert!(old.contains("encrypted") && !old.contains("plain"), "{old}");
    assert_eq!(value(&store.get("Old").unwrap(), "pwd"), "plain");

    // Global environments and globals keep secrets inline, encrypted.
    let global = EnvStore::global(&config);
    global.save(&dev(), None).unwrap();
    let g = fs::read_to_string(config.join("environments/dev.toml")).unwrap();
    assert!(
        g.contains("ilxv1:") && !g.contains("s3cret") && g.contains("http://dev"),
        "{g}"
    );
    assert_eq!(value(&global.get("Dev").unwrap(), "token"), "s3cret");
    save_globals(
        &config,
        &[secret("api_key", "k3y"), EnvVariable::new("region", "eu")],
    )
    .unwrap();
    let gl = fs::read_to_string(config.join("globals.toml")).unwrap();
    assert!(!gl.contains("k3y") && gl.contains("eu"), "{gl}");
    assert_eq!(load_globals(&config).unwrap()[0].value, "k3y");

    // A new password: without it the secrets are locked.
    Vault::change_password(&config, DEFAULT_PASSWORD, "hunter2").unwrap();
    assert!(!vault::install(&config, None).unwrap());
    match store.get("Dev") {
        Err(StorageError::Vault(VaultError::Locked)) => {}
        other => panic!("expected locked, got {other:?}"),
    }
    assert!(matches!(
        store.save(&dev(), None),
        Err(StorageError::Vault(VaultError::Locked))
    ));
    assert!(vault::install(&config, Some("hunter2")).unwrap());
    assert_eq!(value(&store.get("Dev").unwrap(), "token"), "s3cret");

    // Reset (forgotten password): old secrets read as empty, the rest stays.
    vault::set_state(vault::State::Unlocked(std::sync::Arc::new(
        Vault::reset(&config).unwrap(),
    )));
    let after = store.get("Dev").unwrap();
    assert_eq!(
        (value(&after, "token"), value(&after, "base")),
        (String::new(), "http://dev".into())
    );
    assert_eq!(value(&global.get("Dev").unwrap(), "token"), "");
    store.save(&dev(), None).unwrap();
    assert_eq!(value(&store.get("Dev").unwrap(), "token"), "s3cret");
}
