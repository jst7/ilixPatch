//! Postman export (D58): document shape, secrets, and round trip through the importer.

use std::fs;
use std::path::{Path, PathBuf};

use ilixpatch_core::auth::{AddTo, ApiKeyAuth, Auth, BasicAuth, BearerAuth};
use ilixpatch_core::environments::{EnvStore, EnvVariable, EnvironmentDoc};
use ilixpatch_core::storage::{
    load_collection, save_collection_auth, save_collection_variables, save_folder_auth,
    save_request,
};
use ilixpatch_core::{
    Body, Extract, ExtractFrom, ExtractScope, HttpMethod, Request, RequestSettings, Scripts,
    project,
};
use ilixpatch_importer::{
    ApplyContext, EnvScope, ExportOptions, PlanOptions, apply, default_plan, export_postman,
    parse_paths,
};
use indexmap::IndexMap;
use serde_json::Value;

fn new_project(t: &tempfile::TempDir, name: &str) -> PathBuf {
    let root = dunce::canonicalize(t.path()).unwrap().join(name);
    fs::create_dir_all(&root).unwrap();
    project::init_project(&root).unwrap();
    root
}

fn map(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn request(name: &str) -> Request {
    Request {
        name: name.into(),
        method: HttpMethod::Get,
        url: String::new(),
        headers: IndexMap::new(),
        query: IndexMap::new(),
        body: None,
        scripts: Scripts::default(),
        settings: RequestSettings::default(),
        auth: Auth::Inherit,
        extract: Vec::new(),
    }
}

/// A project with one collection covering what the exporter converts.
fn source_project(t: &tempfile::TempDir) -> PathBuf {
    let root = new_project(t, "src");
    let col = project::create_collection(&root, "Shop API").unwrap();
    save_collection_variables(&col, &map(&[("base_url", "https://api.example.com")])).unwrap();
    save_collection_auth(
        &col,
        Some(&Auth::ApiKey(ApiKeyAuth {
            key: "X-Key".into(),
            value: "{{api_key}}".into(),
            add_to: AddTo::Header,
        })),
    )
    .unwrap();

    let mut login = request("Login");
    login.method = HttpMethod::Post;
    login.url = "{{base_url}}/auth/login".into();
    login.headers = map(&[("Accept", "application/json")]);
    login.body = Some(Body::Json {
        content: "{\n  \"user\": \"{{user}}\"\n}".into(),
    });
    login.scripts.pre_request = Some("console.log('hi');".into());
    login.extract = vec![
        Extract {
            var: "token".into(),
            from: ExtractFrom::Json,
            path: "data.items[0].token".into(),
            scope: ExtractScope::Environment,
            enabled: true,
        },
        Extract {
            var: "rid".into(),
            from: ExtractFrom::Header,
            path: "X-Request-Id".into(),
            scope: ExtractScope::Collection,
            enabled: true,
        },
        Extract {
            var: "off".into(),
            from: ExtractFrom::Status,
            path: String::new(),
            scope: ExtractScope::Global,
            enabled: false,
        },
    ];
    login.settings = RequestSettings {
        follow_redirects: Some(false),
        max_redirects: Some(3),
        verify_ssl: Some(false),
        timeout_ms: Some(5000),
        http_version: None,
    };
    save_request(col.join("login.toml"), &login).unwrap();

    let users = project::create_folder(&col, "Users").unwrap();
    save_folder_auth(
        &users,
        Some(&Auth::Basic(BasicAuth {
            username: "admin".into(),
            password: "{{pwd}}".into(),
        })),
    )
    .unwrap();
    let mut list = request("List users");
    list.url = "http://localhost:8080/users?active=true".into();
    list.query = map(&[("page", "{{page}}")]);
    save_request(users.join("list.toml"), &list).unwrap();

    let mut create = request("Create user");
    create.method = HttpMethod::Put;
    create.url = "{{base_url}}/users".into();
    create.body = Some(Body::Form {
        fields: map(&[("name", "Ann"), ("role", "admin")]),
    });
    create.auth = Auth::Bearer(BearerAuth {
        token: "{{token}}".into(),
    });
    create.scripts.tests = Some("pm.test('ok', () => {});".into());
    save_request(users.join("create.toml"), &create).unwrap();

    EnvStore::project(&root)
        .save(
            &EnvironmentDoc {
                name: "Dev".into(),
                variables: vec![
                    EnvVariable::new("base_url", "https://dev.example.com"),
                    EnvVariable {
                        secret: true,
                        ..EnvVariable::new("api_key", "s3cret")
                    },
                    EnvVariable {
                        enabled: false,
                        ..EnvVariable::new("old", "x")
                    },
                ],
                color: None,
                icon: None,
            },
            None,
        )
        .unwrap();
    root
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn item<'a>(items: &'a Value, name: &str) -> &'a Value {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == name)
        .unwrap_or_else(|| panic!("no item {name}"))
}

#[test]
fn exports_a_postman_v21_collection_and_environments() {
    let t = tempfile::tempdir().unwrap();
    let root = source_project(&t);
    let out = t.path().join("out");
    let report = export_postman(&root, &out, &ExportOptions::default()).unwrap();

    assert_eq!(
        (report.collections, report.requests, report.environments),
        (1, 3, 1)
    );
    assert_eq!(report.secrets_omitted, 1);
    let names: Vec<_> = report
        .files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "shop-api.postman_collection.json",
            "dev.postman_environment.json"
        ]
    );
    let codes: Vec<_> = report.warnings.iter().map(|w| w.code.as_str()).collect();
    assert_eq!(codes, ["extract_script", "setting_timeout"]);
    assert_eq!(report.warnings[0].item, "Shop API/Login");

    let c = read_json(&report.files[0]);
    assert_eq!(c["info"]["name"], "Shop API");
    assert_eq!(
        c["info"]["schema"],
        "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
    );
    assert_eq!(c["auth"]["type"], "apikey");
    assert_eq!(c["variable"][0]["key"], "base_url");

    let login = item(&c["item"], "Login");
    assert_eq!(login["request"]["method"], "POST");
    assert_eq!(login["request"]["body"]["mode"], "raw");
    assert_eq!(
        login["request"]["body"]["options"]["raw"]["language"],
        "json"
    );
    assert_eq!(login["request"]["url"]["raw"], "{{base_url}}/auth/login");
    assert_eq!(login["request"]["url"]["host"][0], "{{base_url}}");
    assert_eq!(login["request"]["url"]["path"][1], "login");
    assert!(login["request"].get("auth").is_none(), "inherit is omitted");
    assert_eq!(login["protocolProfileBehavior"]["followRedirects"], false);
    assert_eq!(login["protocolProfileBehavior"]["strictSSL"], false);
    let events = login["event"].as_array().unwrap();
    assert_eq!(events[0]["listen"], "prerequest");
    let test: Vec<_> = events[1]["script"]["exec"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap().to_string())
        .collect();
    let test = test.join("\n");
    assert!(
        test.contains(r#"jsonAt(["data", "items", 0, "token"])"#),
        "{test}"
    );
    assert!(test.contains(r#"pm.environment.set("token", v)"#), "{test}");
    assert!(
        test.contains(r#"pm.response.headers.get("X-Request-Id")"#)
            && test.contains(r#"pm.collectionVariables.set("rid", v)"#),
        "{test}"
    );
    assert!(!test.contains("\"off\""), "disabled rules are skipped");

    let users = item(&c["item"], "users");
    assert_eq!(users["auth"]["type"], "basic");
    let list = item(&users["item"], "List users");
    let url = &list["request"]["url"];
    assert_eq!(
        url["raw"],
        "http://localhost:8080/users?active=true&page={{page}}"
    );
    assert_eq!(
        (url["protocol"].as_str(), url["port"].as_str()),
        (Some("http"), Some("8080"))
    );
    assert_eq!(url["query"][1]["key"], "page");
    let create = item(&users["item"], "Create user");
    assert_eq!(create["request"]["body"]["mode"], "urlencoded");
    assert_eq!(create["request"]["auth"]["type"], "bearer");
    assert_eq!(create["request"]["auth"]["bearer"][0]["value"], "{{token}}");

    let env = read_json(&report.files[1]);
    assert_eq!(env["name"], "Dev");
    assert_eq!(env["_postman_variable_scope"], "environment");
    let vals = env["values"].as_array().unwrap();
    assert_eq!(vals[1]["type"], "secret");
    assert_eq!(vals[1]["value"], "", "secrets are empty by default");
    assert_eq!(vals[2]["enabled"], false);

    let with = export_postman(
        &root,
        &t.path().join("out2"),
        &ExportOptions {
            include_secrets: true,
        },
    )
    .unwrap();
    assert_eq!(with.secrets_omitted, 0);
    assert_eq!(read_json(&with.files[1])["values"][1]["value"], "s3cret");
}

#[test]
fn round_trips_through_the_importer() {
    let t = tempfile::tempdir().unwrap();
    let root = source_project(&t);
    let out = t.path().join("out");
    let report = export_postman(
        &root,
        &out,
        &ExportOptions {
            include_secrets: true,
        },
    )
    .unwrap();

    let dest = new_project(&t, "dest");
    let parsed = parse_paths(&report.files);
    let plan = default_plan(
        &parsed,
        "s",
        &dest,
        None,
        EnvScope::Project,
        PlanOptions::default(),
    );
    let imported = apply(&parsed, &plan, &ApplyContext { global_dir: None }).unwrap();
    assert_eq!((imported.collections, imported.requests), (1, 3));

    let before = load_collection(
        project::load_project(&root).unwrap().collections[0]
            .path
            .clone(),
    )
    .unwrap();
    let after = load_collection(
        project::load_project(&dest).unwrap().collections[0]
            .path
            .clone(),
    )
    .unwrap();
    assert_eq!(after.name, before.name);
    assert_eq!(after.variables, before.variables);
    assert_eq!(after.auth, before.auth);

    let by_name = |c: &ilixpatch_core::Collection| -> IndexMap<String, Request> {
        c.requests()
            .into_iter()
            .map(|(_, r)| (r.name.clone(), r.clone()))
            .collect()
    };
    let (b, a) = (by_name(&before), by_name(&after));
    for name in ["Login", "List users", "Create user"] {
        let (x, y) = (&b[name], &a[name]);
        assert_eq!(y.method, x.method, "{name}");
        assert_eq!(y.headers, x.headers, "{name}");
        assert_eq!(y.body, x.body, "{name}");
        assert_eq!(y.auth, x.auth, "{name}");
        assert_eq!(y.scripts.pre_request, x.scripts.pre_request, "{name}");
    }
    // Inline query parameters are split out by the importer.
    assert_eq!(a["List users"].url, "http://localhost:8080/users");
    assert_eq!(
        a["List users"].query,
        map(&[("active", "true"), ("page", "{{page}}")])
    );
    // Settings Postman can hold come back; the timeout has no place in Postman.
    let s = a["Login"].settings;
    assert_eq!(
        (
            s.follow_redirects,
            s.max_redirects,
            s.verify_ssl,
            s.timeout_ms
        ),
        (Some(false), Some(3), Some(false), None)
    );
    // Response variables come back as a test script.
    assert!(
        a["Login"]
            .scripts
            .tests
            .as_deref()
            .unwrap()
            .contains("pm.environment.set(\"token\"")
    );

    let env = EnvStore::project(&dest).get("Dev").unwrap();
    let api_key = env.variables.iter().find(|v| v.key == "api_key").unwrap();
    assert_eq!(api_key.value, "s3cret");
}
