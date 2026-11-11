//! Postman import: parsing, preview, conversion and writing into a project.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ilixpatch_core::environments::{EnvStore, load_globals};
use ilixpatch_core::{Body, Request, project};
use ilixpatch_importer::{
    ApplyContext, EnvScope, ImportNode, ImportPlacement, OnConflict, PlaceAs, PlanOptions,
    Sessions, apply, default_plan, parse_paths,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn all_fixtures() -> Vec<PathBuf> {
    [
        "shop.postman_collection.json",
        "dev.postman_environment.json",
        "workspace.postman_globals.json",
    ]
    .iter()
    .map(|f| fixture(f))
    .collect()
}

fn find<'a>(nodes: &'a [ImportNode], name: &str) -> Option<&'a ImportNode> {
    for n in nodes {
        if n.name == name {
            return Some(n);
        }
        if let Some(f) = find(&n.children, name) {
            return Some(f);
        }
    }
    None
}

fn new_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap().join("proj");
    fs::create_dir_all(&root).unwrap();
    project::init_project(&root).unwrap();
    let global = t.path().canonicalize().unwrap().join("config");
    fs::create_dir_all(&global).unwrap();
    (t, root, global)
}

fn req(path: &Path) -> Request {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn preview_tree_ids_and_warnings() {
    let mut paths = all_fixtures();
    paths.push(fixture("old.postman_collection.json"));
    let parsed = parse_paths(&paths);
    let p = parsed.preview("s1");
    assert_eq!(p.session, "s1");
    assert_eq!(p.collections.len(), 1);
    assert_eq!(p.errors.len(), 1);
    assert!(p.errors[0].message.contains("v1"), "{:?}", p.errors);

    let c = &p.collections[0];
    assert_eq!(c.name, "Shop API");
    assert_eq!(c.auth.as_deref(), Some("bearer"));
    assert_eq!(c.variables.as_ref().unwrap().len(), 3);
    let list = find(&p.collections, "List users").unwrap();
    assert_eq!(list.auth.as_deref(), Some("inherit"));
    assert!(list.has_scripts);
    assert_eq!(
        list.url.as_deref(),
        Some("{{base_url}}/users?page=1&limit=10")
    );
    assert_eq!(
        find(&p.collections, "Public ping").unwrap().auth.as_deref(),
        Some("noauth")
    );
    assert_eq!(
        find(&p.collections, "Admin").unwrap().auth.as_deref(),
        Some("basic")
    );

    // Ids are stable across parses and unique.
    let again = parse_paths(&paths).preview("s2");
    assert_eq!(again.collections, p.collections);
    let mut ids = Vec::new();
    fn collect(n: &ImportNode, ids: &mut Vec<String>) {
        ids.push(n.id.clone());
        n.children.iter().for_each(|c| collect(c, ids));
    }
    collect(c, &mut ids);
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n);

    assert_eq!(p.environments.len(), 1);
    let dev = &p.environments[0];
    assert_eq!(dev.name, "Dev");
    assert!(
        dev.variables
            .iter()
            .find(|v| v.key == "token")
            .unwrap()
            .secret
    );
    assert!(
        !dev.variables
            .iter()
            .find(|v| v.key == "unused")
            .unwrap()
            .enabled
    );
    assert_eq!(p.globals.as_ref().unwrap().len(), 2);

    let codes: Vec<&str> = p.warnings.iter().map(|w| w.code.as_str()).collect();
    for code in [
        "description",
        "disabled_header",
        "disabled_query",
        "path_variable",
        "body_formdata",
        "body_file",
        "auth_oauth2",
        "body_graphql",
        "disabled_param",
    ] {
        assert!(codes.contains(&code), "missing {code}: {codes:?}");
    }
    assert!(!codes.contains(&"parent_script"));
}

#[test]
fn applies_whole_collection_with_conversions() {
    let (_t, root, global) = new_project();
    let parsed = parse_paths(&all_fixtures());
    let plan = default_plan(
        &parsed,
        "s",
        &root,
        None,
        EnvScope::Project,
        PlanOptions::default(),
    );
    let report = apply(
        &parsed,
        &plan,
        &ApplyContext {
            global_dir: Some(global.clone()),
        },
    )
    .unwrap();
    assert_eq!(
        (
            report.collections,
            report.folders,
            report.requests,
            report.environments,
            report.skipped
        ),
        (1, 3, 9, 1, 0)
    );
    let col = root.join("shop-api");
    assert!(report.files.contains(&col.join("collection.toml")));
    assert!(
        report.files.iter().all(|f| f.is_file()),
        "{:?}",
        report.files
    );

    // The project loads and the collection keeps enabled variables only.
    let p = project::load_project(&root).unwrap();
    let c = &p.collections[0];
    assert_eq!(c.name, "Shop API");
    assert_eq!(
        c.variables.get("base_url").map(String::as_str),
        Some("https://shop.example.com")
    );
    assert!(!c.variables.contains_key("old"));
    assert_eq!(c.requests().len(), 9);

    let list = req(&col.join("users/list-users.toml"));
    assert_eq!(list.url, "{{base_url}}/users");
    assert_eq!(list.query.len(), 1);
    assert_eq!(list.query["page"], "1");
    assert_eq!(list.headers["Accept"], "application/json");
    assert!(!list.headers.contains_key("X-Debug"));
    assert_eq!(list.headers["Authorization"], "Bearer {{token}}");
    let pre = list.scripts.pre_request.unwrap();
    assert!(
        pre.starts_with("// From collection \"Shop API\"\npm.variables.set"),
        "{pre}"
    );
    let tests = list.scripts.tests.unwrap();
    assert!(tests.starts_with("// From folder \"Users\""), "{tests}");
    assert!(tests.ends_with("pm.test(\"list\", () => {});"));

    let get = req(&col.join("users/get-user.toml"));
    assert_eq!(get.url, "{{base_url}}/users/{{userId}}/:format");

    let del = req(&col.join("users/admin/delete-user.toml"));
    assert_eq!(del.url, "{{base_url}}/users/42");
    assert_eq!(del.headers["Authorization"], "Basic YWRtaW46czNjcmV0");
    let ping = req(&col.join("users/admin/public-ping.toml"));
    assert!(!ping.headers.contains_key("Authorization"));

    let login = req(&col.join("auth/login.toml"));
    assert!(matches!(&login.body, Some(Body::Json { content }) if content.contains("\"user\"")));
    assert!(!login.headers.contains_key("Authorization"));
    let pre = login.scripts.pre_request.unwrap();
    assert!(
        pre.contains(r#"btoa(pm.variables.replaceIn("{{user}}:{{pass}}"))"#),
        "{pre}"
    );

    let upload = req(&col.join("upload-avatar.toml"));
    assert_eq!(upload.query["api_key"], "{{apiKey}}");
    assert!(!upload.headers.contains_key("Authorization"));
    match &upload.body {
        Some(Body::Form { fields }) => assert_eq!(fields.keys().collect::<Vec<_>>(), ["name"]),
        b => panic!("{b:?}"),
    }

    let gql = req(&col.join("graphql.toml"));
    match &gql.body {
        Some(Body::Json { content }) => {
            let v: serde_json::Value = serde_json::from_str(content).unwrap();
            assert_eq!(v["variables"]["id"], 1);
            assert!(v["query"].as_str().unwrap().contains("user(id: $id)"));
        }
        b => panic!("{b:?}"),
    }
    assert!(!gql.headers.contains_key("Authorization"));

    let form = req(&col.join("form-login.toml"));
    assert!(matches!(&form.body, Some(Body::Form { fields }) if fields.len() == 1));
    let notes = req(&col.join("notes.toml"));
    assert!(matches!(&notes.body, Some(Body::Text { content }) if content == "plain text"));

    // Environments: secrets by Postman type and by pattern go to .secret.toml.
    let dev = EnvStore::project(&root).get("Dev").unwrap();
    let secret = |k: &str| dev.variables.iter().find(|v| v.key == k).unwrap().secret;
    assert!(secret("token") && secret("db_password") && !secret("base_url"));
    let main = fs::read_to_string(root.join(".ilixpatch/environments/dev.toml")).unwrap();
    assert!(!main.contains("tok-123") && !main.contains("\"pw\""));
    assert!(
        report
            .files
            .contains(&root.join(".ilixpatch/environments/dev.secret.toml"))
    );

    // Globals merged into the user's globals.
    let g = load_globals(&global).unwrap();
    assert!(g.iter().any(|v| v.key == "tenant" && v.value == "acme"));
    assert!(g.iter().find(|v| v.key == "global_token").unwrap().secret);
}

#[test]
fn placements_exclusions_and_conflicts() {
    let (_t, root, global) = new_project();
    let parsed = parse_paths(&[fixture("shop.postman_collection.json")]);
    let preview = parsed.preview("s");
    let col_id = preview.collections[0].id.clone();
    let users = find(&preview.collections, "Users").unwrap().id.clone();
    let admin = find(&preview.collections, "Admin").unwrap().id.clone();
    let notes = find(&preview.collections, "Notes").unwrap().id.clone();
    let ctx = ApplyContext {
        global_dir: Some(global),
    };
    let existing = project::create_collection(&root, "Existing").unwrap();

    let mut plan = default_plan(
        &parsed,
        "s",
        &root,
        None,
        EnvScope::Project,
        PlanOptions::default(),
    );
    plan.placements = vec![
        ImportPlacement {
            source_id: users.clone(),
            target_parent: existing.clone(),
            place_as: PlaceAs::Folder,
            name: Some("People".into()),
        },
        ImportPlacement {
            source_id: notes.clone(),
            target_parent: existing.clone(),
            place_as: PlaceAs::Request,
            name: Some("Write notes".into()),
        },
        ImportPlacement {
            source_id: admin.clone(),
            target_parent: existing.clone(),
            place_as: PlaceAs::Request,
            name: None,
        },
    ];
    plan.excluded = vec![find(&preview.collections, "Get user").unwrap().id.clone()];
    let r = apply(&parsed, &plan, &ctx).unwrap();
    assert!(existing.join("people/list-users.toml").is_file());
    assert!(!existing.join("people/get-user.toml").exists());
    // Admin is placed on its own, so it's not copied inside People too.
    assert!(!existing.join("people/admin").exists());
    assert!(existing.join("write-notes.toml").is_file());
    assert_eq!(req(&existing.join("write-notes.toml")).name, "Write notes");
    // Admin flattened as requests.
    assert!(
        existing.join("delete-user.toml").is_file() && existing.join("public-ping.toml").is_file()
    );
    assert_eq!((r.collections, r.folders, r.requests), (0, 1, 4));

    // Same collection twice: rename, then skip, then overwrite.
    let mut whole = default_plan(
        &parsed,
        "s",
        &root,
        None,
        EnvScope::Project,
        PlanOptions::default(),
    );
    apply(&parsed, &whole, &ctx).unwrap();
    let r = apply(&parsed, &whole, &ctx).unwrap();
    assert!(root.join("shop-api-2/collection.toml").is_file());
    assert_eq!(r.collections, 1);
    whole.options.on_conflict = OnConflict::Skip;
    let r = apply(&parsed, &whole, &ctx).unwrap();
    assert_eq!((r.skipped, r.requests, r.files.len()), (1, 0, 0));
    assert!(!root.join("shop-api-3").exists());
    whole.options.on_conflict = OnConflict::Overwrite;
    fs::write(
        root.join("shop-api/notes.toml"),
        "name = \"x\"\nurl = \"changed\"\n",
    )
    .unwrap();
    let r = apply(&parsed, &whole, &ctx).unwrap();
    assert_eq!(r.requests, 9);
    assert!(!root.join("shop-api-3").exists());
    assert_eq!(
        req(&root.join("shop-api/notes.toml")).url,
        "{{base_url}}/notes"
    );

    // Validation: nothing is written when a placement is invalid.
    let bad = |target: PathBuf, place_as| {
        let mut p = default_plan(
            &parsed,
            "s",
            &root,
            None,
            EnvScope::Project,
            PlanOptions::default(),
        );
        p.placements = vec![ImportPlacement {
            source_id: col_id.clone(),
            target_parent: target,
            place_as,
            name: None,
        }];
        apply(&parsed, &p, &ApplyContext::default())
    };
    assert!(bad(existing.clone(), PlaceAs::Collection).is_err());
    assert!(bad(root.clone(), PlaceAs::Folder).is_err());
    assert!(bad(root.parent().unwrap().to_path_buf(), PlaceAs::Collection).is_err());
    assert!(bad(root.join(".ilixpatch"), PlaceAs::Folder).is_err());
    // Global environments need a config dir.
    let mut p = default_plan(
        &parse_paths(&all_fixtures()),
        "s",
        &root,
        None,
        EnvScope::Global,
        PlanOptions::default(),
    );
    p.placements.clear();
    assert!(apply(&parse_paths(&all_fixtures()), &p, &ApplyContext::default()).is_err());
}

#[test]
fn nested_placement_moves_the_node_out_of_its_ancestor() {
    let (_t, root, _) = new_project();
    let parsed = parse_paths(&[fixture("shop.postman_collection.json")]);
    let preview = parsed.preview("s");
    let admin = find(&preview.collections, "Admin").unwrap().id.clone();
    let notes = find(&preview.collections, "Notes").unwrap().id.clone();
    let other = project::create_collection(&root, "Other").unwrap();
    let mut plan = default_plan(
        &parsed,
        "s",
        &root,
        None,
        EnvScope::Project,
        PlanOptions::default(),
    );
    for (id, as_) in [(admin, PlaceAs::Folder), (notes, PlaceAs::Request)] {
        plan.placements.push(ImportPlacement {
            source_id: id,
            target_parent: other.clone(),
            place_as: as_,
            name: None,
        });
    }
    let r = apply(&parsed, &plan, &ApplyContext::default()).unwrap();
    assert!(!root.join("shop-api/users/admin").exists());
    assert!(!root.join("shop-api/notes.toml").exists());
    assert!(other.join("admin/delete-user.toml").is_file());
    assert!(other.join("notes.toml").is_file());
    assert_eq!(r.requests, 9);
}

#[test]
fn script_and_auth_options() {
    let (_t, root, _) = new_project();
    let parsed = parse_paths(&[fixture("shop.postman_collection.json")]);
    let mut opts = PlanOptions {
        inline_parent_scripts: false,
        ..PlanOptions::default()
    };
    let plan = default_plan(&parsed, "s", &root, None, EnvScope::Project, opts.clone());
    let r = apply(&parsed, &plan, &ApplyContext::default()).unwrap();
    assert!(r.warnings.iter().any(|w| w.code == "parent_script"));
    let list = req(&root.join("shop-api/users/list-users.toml"));
    assert_eq!(list.scripts.pre_request, None);
    assert_eq!(
        list.scripts.tests.as_deref(),
        Some("pm.test(\"list\", () => {});")
    );

    opts.keep_scripts = false;
    opts.convert_auth = false;
    let plan = default_plan(&parsed, "s", &root, None, EnvScope::Project, opts);
    apply(&parsed, &plan, &ApplyContext::default()).unwrap();
    let list = req(&root.join("shop-api-2/users/list-users.toml"));
    assert!(list.scripts.tests.is_none() && !list.headers.contains_key("Authorization"));
    let login = req(&root.join("shop-api-2/auth/login.toml"));
    assert!(login.scripts.pre_request.is_none());
}

#[test]
fn zip_dumps_and_directories() {
    let t = tempfile::tempdir().unwrap();
    let zip_path = t.path().join("export.zip");
    let mut w = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    let o = zip::write::SimpleFileOptions::default();
    for (name, file) in [
        ("collection/1.json", "shop.postman_collection.json"),
        ("environment/2.json", "dev.postman_environment.json"),
    ] {
        w.start_file(name, o).unwrap();
        w.write_all(&fs::read(fixture(file)).unwrap()).unwrap();
    }
    w.start_file("archive.json", o).unwrap();
    w.write_all(br#"{"collection":{"1":true},"environment":{"2":true}}"#)
        .unwrap();
    w.finish().unwrap();

    let p = parse_paths(&[&zip_path]).preview("z");
    assert_eq!(
        (p.collections.len(), p.environments.len(), p.errors.len()),
        (1, 1, 0),
        "{:?}",
        p.errors
    );
    assert!(
        p.collections[0].id
            != parse_paths(&[fixture("shop.postman_collection.json")])
                .preview("x")
                .collections[0]
                .id
    );

    // A directory: Postman files are picked up, other JSON is ignored silently.
    let dir = t.path().join("dir");
    fs::create_dir_all(dir.join("nested")).unwrap();
    fs::copy(
        fixture("shop.postman_collection.json"),
        dir.join("nested/shop.json"),
    )
    .unwrap();
    fs::copy(
        fixture("workspace.postman_globals.json"),
        dir.join("g.json"),
    )
    .unwrap();
    fs::write(dir.join("package.json"), "{\"name\": \"x\"}").unwrap();
    let p = parse_paths(&[&dir]).preview("d");
    assert_eq!((p.collections.len(), p.errors.len()), (1, 0));
    assert!(p.globals.is_some());

    // Explicit non-Postman files and missing paths are reported.
    let p = parse_paths(&[dir.join("package.json"), dir.join("missing.json")]).preview("e");
    assert_eq!(p.errors.len(), 2);
}

#[test]
fn sessions_store_and_expire_on_remove() {
    let s = Sessions::new();
    let id = s.insert(parse_paths(&[fixture("shop.postman_collection.json")]));
    assert!(s.get(&id).is_some());
    s.remove(&id);
    assert!(s.get(&id).is_none());
}
