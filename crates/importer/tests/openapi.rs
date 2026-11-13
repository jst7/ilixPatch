//! OpenAPI / Swagger import (D53): parsing, grouping and writing into a project.

use std::fs;
use std::path::{Path, PathBuf};

use ilixpatch_core::auth::{
    AddTo, ApiKeyAuth, Auth, BasicAuth, BearerAuth, OAuth2Auth, OAuth2Grant,
};
use ilixpatch_core::{Body, HttpMethod, Request, project};
use ilixpatch_importer::{
    ApplyContext, EnvScope, GroupBy, ImportNode, ImportReport, ParseOptions, Parsed, PlanOptions,
    apply, default_plan, parse_paths, parse_paths_with,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn new_project() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(t.path()).unwrap().join("proj");
    fs::create_dir_all(&root).unwrap();
    project::init_project(&root).unwrap();
    (t, root)
}

fn import(parsed: &Parsed, root: &Path) -> ImportReport {
    let plan = default_plan(
        parsed,
        "s",
        root,
        None,
        EnvScope::Project,
        PlanOptions::default(),
    );
    apply(parsed, &plan, &ApplyContext { global_dir: None }).unwrap()
}

fn req(path: &Path) -> Request {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap()
}

fn names(nodes: &[ImportNode]) -> Vec<&str> {
    nodes.iter().map(|n| n.name.as_str()).collect()
}

fn codes(report: &ImportReport) -> Vec<&str> {
    report.warnings.iter().map(|w| w.code.as_str()).collect()
}

#[test]
fn openapi_31_yaml_grouped_by_tags() {
    let parsed = parse_paths(&[fixture("zoo.openapi.yaml")]);
    let preview = parsed.preview("s");
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert!(preview.openapi);
    let zoo = &preview.collections[0];
    assert_eq!(zoo.name, "Zoo API");
    // Declared tag order, untagged operations last; TRACE is skipped.
    assert_eq!(names(&zoo.children), ["keepers", "animals", "Health"]);
    assert_eq!(
        names(&zoo.children[1].children),
        [
            "List animals",
            "createAnimal",
            "Get an animal",
            "Replace an animal",
            "Upload a photo"
        ]
    );
    let get = &zoo.children[1].children[2];
    assert_eq!(get.method, Some(HttpMethod::Get));
    assert_eq!(get.url.as_deref(), Some("{{baseUrl}}/animals/{{animalId}}"));
    assert_eq!(get.auth.as_deref(), Some("apikey"));
    assert_eq!(zoo.children[1].children[0].auth.as_deref(), Some("inherit"));

    let (_t, root) = new_project();
    let report = import(&parsed, &root);
    assert_eq!(
        (report.collections, report.folders, report.requests),
        (1, 2, 8)
    );
    let warnings = codes(&report);
    for code in [
        "openapi_relative_server",
        "openapi_servers",
        "openapi_method",
        "openapi_ref",
        "body_file",
        "body_formdata",
        "description",
    ] {
        assert!(warnings.contains(&code), "missing {code}: {warnings:?}");
    }

    let p = project::load_project(&root).unwrap();
    let c = &p.collections[0];
    // Server variables are substituted; a relative URL stays relative (with a warning).
    assert_eq!(c.variables["baseUrl"], "/v2");
    // Auth placeholders are not collection variables (collection.toml is committed).
    assert_eq!(c.variables.len(), 1);
    assert_eq!(
        c.auth,
        Auth::Bearer(BearerAuth {
            token: "{{bearerToken}}".into()
        })
    );
    let col = root.join("zoo-api");

    let list = req(&col.join("animals/list-animals.toml"));
    assert_eq!(list.url, "{{baseUrl}}/animals");
    // Required → always (default value); optional → only with a given value.
    assert_eq!(
        list.query.iter().collect::<Vec<_>>(),
        [
            (&"limit".to_string(), &"20".to_string()),
            (&"species".into(), &"lion".into())
        ]
    );
    assert_eq!(list.headers["X-Trace"], "{{X-Trace}}");
    assert_eq!(list.headers["Cookie"], "session=abc");
    // The Accept *parameter* is ignored (OpenAPI rule); Accept comes from the response.
    assert_eq!(list.headers["Accept"], "application/json");
    assert!(list.auth.is_inherit());

    let create = req(&col.join("animals/createanimal.toml"));
    assert_eq!(create.method, HttpMethod::Post);
    let Some(Body::Json { content }) = &create.body else {
        panic!("{:?}", create.body)
    };
    // allOf merged, readOnly and recursive properties left out, key order kept.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(content).unwrap(),
        serde_json::json!({"name": "string", "species": "lion", "born": "2024-01-01",
                           "tags": ["string"], "weight": 0, "keeper": "user@example.com"})
    );
    assert!(content.find("\"name\"").unwrap() < content.find("\"keeper\"").unwrap());

    let get = req(&col.join("animals/get-an-animal.toml"));
    assert_eq!(
        get.auth,
        Auth::ApiKey(ApiKeyAuth {
            key: "X-API-Key".into(),
            value: "{{apiKey}}".into(),
            add_to: AddTo::Header,
        })
    );
    assert_eq!(get.headers["Accept"], "application/xml");

    let put = req(&col.join("animals/replace-an-animal.toml"));
    // The operation's own parameter (with an example) overrides the path-level one.
    assert_eq!(put.url, "{{baseUrl}}/animals/7");
    assert_eq!(put.headers["Content-Type"], "application/xml");
    assert!(matches!(&put.body, Some(Body::Text { content }) if content == "<animal/>"));

    let photo = req(&col.join("animals/upload-a-photo.toml"));
    assert_eq!(photo.url, "{{baseUrl}}/animals/{{animalId}}/photo");
    assert!(
        matches!(&photo.body, Some(Body::Form { fields }) if fields.len() == 1 && fields["caption"] == "string")
    );

    let login = req(&col.join("keepers/log-in.toml"));
    assert_eq!(login.auth, Auth::NoAuth);
    assert!(
        matches!(&login.body, Some(Body::Form { fields }) if fields["user"] == "ana" && fields["password"].is_empty())
    );

    let schedule = req(&col.join("keepers/keeper-schedule.toml"));
    let Auth::Oauth2(OAuth2Auth {
        grant_type,
        access_token_url,
        client_id,
        client_secret,
        scope,
        ..
    }) = &schedule.auth
    else {
        panic!("{:?}", schedule.auth)
    };
    // Client credentials is preferred: it also works in the CLI.
    assert_eq!(*grant_type, OAuth2Grant::ClientCredentials);
    assert_eq!(
        access_token_url.as_deref(),
        Some("https://auth.zoo.example.com/token")
    );
    assert_eq!(client_id, "{{clientId}}");
    assert_eq!(client_secret.as_deref(), Some("{{clientSecret}}"));
    assert_eq!(scope.as_deref(), Some("schedule:read keepers"));

    // `security: [{}]` (optional auth) → no auth; the external $ref parameter is skipped.
    let health = req(&col.join("health.toml"));
    assert_eq!(health.auth, Auth::NoAuth);
    assert!(health.query.is_empty());
}

#[test]
fn openapi_grouped_by_path() {
    let parsed = parse_paths_with(
        &[fixture("zoo.openapi.yaml")],
        ParseOptions {
            group_by: GroupBy::Path,
        },
    );
    let zoo = &parsed.preview("s").collections[0];
    assert_eq!(names(&zoo.children), ["animals", "keepers", "health"]);
    let animals = &zoo.children[0];
    assert_eq!(
        names(&animals.children),
        ["List animals", "createAnimal", "{animalId}"]
    );
    assert_eq!(
        names(&animals.children[2].children),
        ["Get an animal", "Replace an animal", "photo"]
    );
    // A folder with no requests and one sub-folder is merged with it.
    assert_eq!(
        names(&zoo.children[1].children),
        ["login", "{keeperId}/schedule"]
    );

    let (_t, root) = new_project();
    let report = import(&parsed, &root);
    assert_eq!(report.requests, 8);
    assert!(
        report.files.iter().all(|f| f.is_file()),
        "{:?}",
        report.files
    );
}

#[test]
fn swagger_20_json() {
    let parsed = parse_paths(&[fixture("shelter.swagger.json")]);
    let preview = parsed.preview("s");
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    let (_t, root) = new_project();
    let report = import(&parsed, &root);
    assert_eq!((report.folders, report.requests), (1, 3));

    let c = &project::load_project(&root).unwrap().collections[0];
    // https preferred among the declared schemes.
    assert_eq!(c.variables["baseUrl"], "https://shelter.example.com/api");
    assert_eq!(
        c.auth,
        Auth::Basic(BasicAuth {
            username: "{{basicUsername}}".into(),
            password: "{{basicPassword}}".into()
        })
    );
    let col = root.join("shelter/dogs");

    let add = req(&col.join("add-a-dog.toml"));
    assert!(add.auth.is_inherit());
    assert_eq!(add.headers["Accept"], "application/json");
    assert!(!add.headers.contains_key("Content-Type"));
    let Some(Body::Json { content }) = &add.body else {
        panic!("{:?}", add.body)
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(content).unwrap(),
        serde_json::json!({"name": "Rex", "age": 0})
    );

    let find = req(&col.join("find-dogs.toml"));
    assert_eq!(find.query["status"], "available");
    assert_eq!(find.query["page"], "1");
    assert_eq!(
        find.auth,
        Auth::ApiKey(ApiKeyAuth {
            key: "api_key".into(),
            value: "{{key}}".into(),
            add_to: AddTo::Query,
        })
    );

    let photo = req(&col.join("upload-a-photo.toml"));
    assert_eq!(photo.url, "{{baseUrl}}/dogs/3/photo");
    assert!(matches!(&photo.body, Some(Body::Form { fields }) if fields.contains_key("note")));
    assert!(codes(&report).contains(&"body_file"));
    let Auth::Oauth2(o) = &photo.auth else {
        panic!("{:?}", photo.auth)
    };
    assert_eq!(o.grant_type, OAuth2Grant::ClientCredentials);
    assert_eq!(o.scope.as_deref(), Some("write"));
}

#[test]
fn downloads_resolve_relative_servers_and_yaml_without_extension() {
    let yaml = fs::read(fixture("zoo.openapi.yaml")).unwrap();
    let mut parsed = Parsed::new(ParseOptions::default());
    parsed.add_download("https://zoo.example.com/docs/spec?format=yaml", &yaml);
    let preview = parsed.preview("s");
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    let (_t, root) = new_project();
    let report = import(&parsed, &root);
    assert!(!codes(&report).contains(&"openapi_relative_server"));
    let c = &project::load_project(&root).unwrap().collections[0];
    assert_eq!(c.variables["baseUrl"], "https://zoo.example.com/v2");
}

#[test]
fn unsupported_and_invalid_documents() {
    let t = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let p = t.path().join(name);
        fs::write(&p, text).unwrap();
        p
    };
    let v1 = write("old.json", r#"{"swagger":"1.2","apis":[]}"#);
    let v4 = write("future.yaml", "openapi: 4.0.0\ninfo: {title: x}\n");
    let bad = write("bad.yaml", "openapi: 3.0.0\npaths: [\n");
    let other = write("compose.yml", "services: {web: {image: nginx}}\n");
    let parsed = parse_paths(&[&v1, &v4, &bad, &other]);
    let errors = parsed.preview("s").errors;
    let msg = |p: &Path| {
        errors
            .iter()
            .find(|e| e.file == p.display().to_string())
            .map(|e| e.message.clone())
            .unwrap_or_default()
    };
    assert!(
        msg(&v1).contains("Swagger 1.2 is not supported"),
        "{errors:?}"
    );
    assert!(msg(&v4).contains("OpenAPI 4.0.0 is not supported"));
    assert!(msg(&bad).starts_with("invalid YAML"));
    assert_eq!(msg(&other), "not an OpenAPI or Swagger spec");

    // Inside a folder, YAML files that aren't specs are ignored silently.
    let dir = t.path().join("specs");
    fs::create_dir(&dir).unwrap();
    fs::copy(fixture("zoo.openapi.yaml"), dir.join("zoo.yaml")).unwrap();
    fs::write(dir.join("ci.yml"), "on: push\n").unwrap();
    let preview = parse_paths(&[&dir]).preview("s");
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert_eq!(names(&preview.collections), ["Zoo API"]);
}
