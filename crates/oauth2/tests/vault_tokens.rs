//! D60: OAuth 2.0 tokens are stored encrypted (own binary: the vault state is process-wide).

use std::fs;

use ilixpatch_core::vault;
use ilixpatch_oauth2::{StoredToken, TokenStore};

#[test]
fn tokens_are_encrypted_and_plain_files_migrate() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let token = StoredToken {
        id: "t1".into(),
        token_name: "gh".into(),
        access_token: "ghp_supersecret".into(),
        token_type: Some("bearer".into()),
        refresh_token: Some("refresh_me".into()),
        id_token: None,
        scope: None,
        expires_at: None,
        created_at: 1,
        raw: serde_json::Value::Null,
    };
    // Written in plain text before a vault exists (as before D60).
    TokenStore::new(&data).add(token.clone()).unwrap();
    let path = TokenStore::new(&data).path().to_path_buf();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("ghp_supersecret")
    );

    assert!(vault::install(&t.path().join("config"), None).unwrap());
    let store = TokenStore::new(&data);
    assert_eq!(
        store.current("gh").unwrap().unwrap().access_token,
        "ghp_supersecret"
    );
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.starts_with("ilxv1:") && !text.contains("supersecret") && !text.contains("refresh_me")
    );
    assert_eq!(store.list(None).unwrap().len(), 1);

    vault::set_state(vault::State::Locked);
    assert!(store.list(None).unwrap_err().contains("locked"));
}
