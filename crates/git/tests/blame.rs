//! Annotate / blame against real temp repos.

mod common;

use std::path::Path;

use common::*;
use ilixpatch_git::{self as git, BlameInfo};

fn commit_as(dir: &Path, who: &str, time: &str, msg: &str) {
    let email = format!("{}@example.com", who.to_lowercase());
    let date = format!("{time} +0000");
    let env = [
        ("GIT_AUTHOR_NAME", who),
        ("GIT_AUTHOR_EMAIL", email.as_str()),
        ("GIT_AUTHOR_DATE", date.as_str()),
        ("GIT_COMMITTER_DATE", date.as_str()),
    ];
    sh(dir, &["add", "-A"]);
    sh_env(dir, &["commit", "-q", "-m", msg], &env);
}

fn who(b: &BlameInfo) -> &str {
    b.author.as_deref().unwrap_or("UNCOMMITTED")
}

fn whos(v: &[BlameInfo]) -> Vec<&str> {
    v.iter().map(who).collect()
}

const ANN: &str = r#"name = "Get user"
method = "GET"
url = "https://api/users/1"

[headers]
Accept = "application/json"
X-Old = "1"
Long = '''
first
'''

[query]
page = "1"

[body]
type = "json"
content = '''
{
  "a": 1
}
'''

[scripts]
tests = """pm.test("ok", () => {});
pm.expect(1).to.eql(1);"""
"#;

const BOB: &str = r#"name = "Get user"
method = "GET"
url = "https://api/users/2"

[headers]
Accept = "application/json"
X-New = "1"
Long = '''
second
'''

[query]
page = "1"

[body]
type = "json"
content = '''
{
  "a": 2
}
'''

[scripts]
tests = """pm.test("ok", () => {});
pm.expect(1).to.eql(1);"""
"#;

#[test]
fn request_fields_two_authors_and_uncommitted_edit() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    let f = r.join("col/get-user.toml");
    write(&f, ANN);
    commit_as(&r, "Ann", "1700000000", "add request");
    write(&f, BOB);
    commit_as(&r, "Bob", "1700001000", "change request");
    // Uncommitted edit of the name.
    write(&f, BOB.replace("Get user", "Get one user"));

    let b = git::blame_request(&f).unwrap();
    assert_eq!(b.path, f);
    assert_eq!(b.name.as_ref().map(who), Some("UNCOMMITTED"));
    assert_eq!(b.name.as_ref().unwrap().commit, None);
    let method = b.method.unwrap();
    assert_eq!(who(&method), "Ann");
    assert_eq!(method.author_email.as_deref(), Some("ann@example.com"));
    assert_eq!(method.time, Some(1700000000));
    assert_eq!(method.summary.as_deref(), Some("add request"));
    assert_eq!(method.commit.as_ref().unwrap().len(), 40);
    assert_eq!(who(&b.url.unwrap()), "Bob");

    // Renamed key belongs to Bob; multi-line value takes its newest line.
    let headers: Vec<_> = b
        .headers
        .iter()
        .map(|(k, v)| (k.as_str(), who(v)))
        .collect();
    assert_eq!(
        headers,
        [("Accept", "Ann"), ("Long", "Bob"), ("X-New", "Bob")]
    );
    assert_eq!(who(&b.query["page"]), "Ann");

    // Body content lines: first line after ''' is content line 1; trailing "" line too.
    assert_eq!(whos(&b.body_lines), ["Ann", "Bob", "Ann", "Ann"]);
    assert_eq!(whos(&b.tests_lines), ["Ann", "Ann"]);
    assert!(b.pre_request_lines.is_empty());
    assert!(b.form.is_empty());

    let fb = git::blame_file(&f).unwrap();
    assert_eq!(fb.lines.len(), BOB.lines().count());
    assert_eq!(who(&fb.lines[0]), "UNCOMMITTED");
    assert_eq!(who(&fb.lines[2]), "Bob");
    // Cached result is identical.
    assert_eq!(git::blame_file(&f).unwrap(), fb);
}

#[test]
fn single_line_strings_dotted_keys_and_form() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    let f = r.join("req.toml");
    write(
        &f,
        "name = 'x'\nurl = \"u\"\nheaders.Accept = \"a\"\n\n[body]\ntype = \"json\"\ncontent = \"{\\n  \\\"a\\\": 1\\n}\"\n",
    );
    commit_as(&r, "Ann", "1700000000", "one");
    let b = git::blame_request(&f).unwrap();
    assert_eq!(whos(&b.body_lines), ["Ann", "Ann", "Ann"]);
    assert_eq!(who(&b.headers["Accept"]), "Ann");
    assert!(b.method.is_none());

    let g = r.join("form.toml");
    write(
        &g,
        "name = \"f\"\nurl = \"u\"\n[body]\ntype = \"form\"\n[body.fields]\nuser = \"a\"\npass = \"b\"\n",
    );
    commit_as(&r, "Ann", "1700000000", "form");
    write(
        &g,
        "name = \"f\"\nurl = \"u\"\n[body]\ntype = \"form\"\n[body.fields]\nuser = \"a\"\npass = \"changed\"\n",
    );
    commit_as(&r, "Bob", "1700005000", "form change");
    let b = git::blame_request(&g).unwrap();
    let form: Vec<_> = b.form.iter().map(|(k, v)| (k.as_str(), who(v))).collect();
    assert_eq!(form, [("pass", "Bob"), ("user", "Ann")]);
    assert!(b.body_lines.is_empty());
}

#[test]
fn untracked_and_new_files_are_uncommitted() {
    require_git!();
    let (_t, base) = tmp();
    let r = new_repo(&base.join("r"));
    let f = r.join("new.toml");
    write(&f, "name = \"n\"\nurl = \"u\"\n");
    // Empty repo, untracked file.
    let fb = git::blame_file(&f).unwrap();
    assert_eq!(fb.lines, vec![BlameInfo::default(); 2]);
    write(&r.join("other.toml"), "x");
    sh(&r, &["add", "other.toml"]);
    sh(&r, &["commit", "-q", "-m", "other"]);
    let b = git::blame_request(&f).unwrap();
    assert_eq!(b.url, Some(BlameInfo::default()));

    // Not in a repo / invalid TOML.
    let outside = base.join("outside.toml");
    write(&outside, "name = \"n\"");
    assert!(git::blame_file(&outside).is_err());
    write(&f, "name = ");
    assert!(git::blame_request(&f).is_err());
}
