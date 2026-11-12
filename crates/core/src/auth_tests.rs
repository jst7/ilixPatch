//! D39 storage and interpolation tests.

use std::fs;

use indexmap::IndexMap;

use crate::auth::{AddTo, ApiKeyAuth, Auth, BasicAuth, BearerAuth, OAuth2Auth};
use crate::storage::{
    COLLECTION_FILE, FOLDER_FILE, load_collection, save_collection_auth, save_collection_variables,
    save_folder_auth, save_request,
};
use crate::{Request, Scope, VarError};

fn collection() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(COLLECTION_FILE),
        "# my collection\nversion = 1\nname = \"Demo\" # inline comment\n\n[variables]\nbase = \"http://x\"\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("users")).unwrap();
    fs::write(
        dir.path().join("users/list.toml"),
        "name = \"List\"\nurl = \"{{base}}/users\"\n",
    )
    .unwrap();
    dir
}

fn bearer(t: &str) -> Auth {
    Auth::Bearer(BearerAuth { token: t.into() })
}

#[test]
fn collection_auth_round_trip_keeps_comments() {
    let dir = collection();
    let root = dir.path();
    save_collection_auth(root, Some(&bearer("{{token}}"))).unwrap();
    let text = fs::read_to_string(root.join(COLLECTION_FILE)).unwrap();
    assert!(text.starts_with("# my collection\n"), "{text}");
    assert!(text.contains("name = \"Demo\" # inline comment"), "{text}");
    assert!(
        text.contains("[auth]\ntype = \"bearer\"\ntoken = \"{{token}}\""),
        "{text}"
    );
    let c = load_collection(root).unwrap();
    assert_eq!(c.auth, bearer("{{token}}"));
    assert_eq!(c.variables["base"], "http://x");

    // OAuth 2.0 with a nested table survives too.
    let mut o = OAuth2Auth {
        token_name: "t".into(),
        ..Default::default()
    };
    o.extra_params.insert("audience".into(), "api".into());
    save_collection_auth(root, Some(&Auth::Oauth2(o.clone()))).unwrap();
    assert_eq!(load_collection(root).unwrap().auth, Auth::Oauth2(o));

    save_collection_auth(root, None).unwrap();
    let text = fs::read_to_string(root.join(COLLECTION_FILE)).unwrap();
    assert!(!text.contains("auth"), "{text}");
    assert!(text.contains("# my collection"));
    assert_eq!(load_collection(root).unwrap().auth, Auth::Inherit);

    let mut vars = IndexMap::new();
    vars.insert("base".to_string(), "https://y".to_string());
    vars.insert("new".to_string(), "1".to_string());
    save_collection_variables(root, &vars).unwrap();
    let c = load_collection(root).unwrap();
    assert_eq!(c.variables, vars);
    assert!(
        fs::read_to_string(root.join(COLLECTION_FILE))
            .unwrap()
            .contains("# inline comment")
    );
    save_collection_variables(root, &IndexMap::new()).unwrap();
    assert!(load_collection(root).unwrap().variables.is_empty());

    assert!(save_collection_auth(root.join("users"), None).is_err());
}

#[test]
fn folder_auth_file() {
    let dir = collection();
    let users = dir.path().join("users");
    let basic = Auth::Basic(BasicAuth {
        username: "u".into(),
        password: "p".into(),
    });
    save_folder_auth(&users, Some(&basic)).unwrap();
    let c = load_collection(dir.path()).unwrap();
    let folder = &c.root.folders[0];
    assert_eq!(folder.auth, basic);
    // folder.toml is not a request.
    assert_eq!(folder.requests.len(), 1);

    // Keep other content; remove the file when empty.
    let file = users.join(FOLDER_FILE);
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("# keep me\nnote = \"x\"\n\n{text}")).unwrap();
    save_folder_auth(&users, None).unwrap();
    let kept = fs::read_to_string(&file).unwrap();
    assert!(kept.contains("# keep me") && kept.contains("note = \"x\"") && !kept.contains("auth"));
    fs::write(&file, "[auth]\ntype = \"no_auth\"\n").unwrap();
    assert_eq!(
        load_collection(dir.path()).unwrap().root.folders[0].auth,
        Auth::NoAuth
    );
    save_folder_auth(&users, Some(&Auth::Inherit)).unwrap();
    assert!(!file.exists());
    assert_eq!(
        load_collection(dir.path()).unwrap().root.folders[0].auth,
        Auth::Inherit
    );

    assert!(
        save_folder_auth(dir.path(), None).is_err(),
        "the collection root is not a folder"
    );
}

#[test]
fn request_auth_round_trip() {
    let dir = collection();
    let path = dir.path().join("users/list.toml");
    let mut req: Request = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(req.auth, Auth::Inherit);
    req.auth = Auth::ApiKey(ApiKeyAuth {
        key: "X-Key".into(),
        value: "{{key}}".into(),
        add_to: AddTo::Query,
    });
    save_request(&path, &req).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("[auth]") && text.contains("in = \"query\""),
        "{text}"
    );
    let back: Request = toml::from_str(&text).unwrap();
    assert_eq!(back.auth, req.auth);
    req.auth = Auth::Inherit;
    save_request(&path, &req).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains("[auth]"));
}

#[test]
fn auth_fields_are_interpolated() {
    let mut vars = IndexMap::new();
    vars.insert("user".to_string(), "ann".to_string());
    vars.insert("pass".to_string(), "s3cret".to_string());
    let scope = Scope::new().push(&vars);
    let mut req: Request = toml::from_str("name = \"x\"\nurl = \"http://h\"\n").unwrap();
    req.auth = Auth::Basic(BasicAuth {
        username: "{{user}}".into(),
        password: "{{pass}}!".into(),
    });
    let r = scope.resolve_request(&req).unwrap();
    assert_eq!(
        r.auth,
        Auth::Basic(BasicAuth {
            username: "ann".into(),
            password: "s3cret!".into(),
        })
    );

    let mut o = OAuth2Auth {
        token_name: "{{user}}-token".into(),
        access_token: Some("{{a}}".into()),
        ..Default::default()
    };
    o.extra_params.insert("x".into(), "{{b}}".into());
    req.auth = Auth::Oauth2(o);
    assert_eq!(
        scope.resolve_request(&req).unwrap_err(),
        VarError::Undefined(vec!["a".into(), "b".into()])
    );
    // Dynamic variables work in auth too.
    req.auth = bearer("{{$guid}}");
    let Auth::Bearer(b) = scope.resolve_request(&req).unwrap().auth else {
        panic!()
    };
    assert_eq!(b.token.len(), 36);
}
