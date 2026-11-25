//! The community catalog: a fixture signed with a throwaway key (tests/fixtures/community-test.pub).
//! Nothing here touches the network.

use std::path::PathBuf;

use rigbrew_core::catalog::{self, CatalogError, ADD, LISTED};
use rigbrew_core::community::{self, Host, Target};
use rigbrew_core::manifest::PUBLIC_KEY;
use serde_json::json;

const CATALOG: &str = include_str!("fixtures/community.json");
const SIG: &str = include_str!("fixtures/community.json.minisig");

fn test_key() -> String {
    include_str!("fixtures/community-test.pub").lines().nth(1).unwrap().trim().to_string()
}

fn temp_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("rigbrew-catalog-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn one(entry: serde_json::Value) -> Option<catalog::Entry> {
    catalog::parse(&json!({"version": 1, "models": [entry]}).to_string()).unwrap().models.into_iter().next()
}

fn model(url: &str) -> serde_json::Value {
    json!({"id": "m", "name": "M", "kind": "chat", "by": "Someone", "url": url, "license": "MIT", "about": "Short.", "status": "add"})
}

#[test]
fn reads_the_file_and_leaves_out_what_points_elsewhere() {
    let c = catalog::parse(CATALOG).unwrap();
    assert_eq!(c.updated, "2026-11-25");
    let ids: Vec<&str> = c.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["example-chat", "example-video", "example-new-kind"], "the entry on another site is dropped");

    let chat = &c.models[0];
    assert_eq!((chat.name.as_str(), chat.kind.as_str(), chat.by.as_str()), ("Example Chat", "chat", "Example Lab"));
    assert_eq!(chat.url, "https://huggingface.co/example-owner/Example-Chat-GGUF");
    assert_eq!(chat.status, ADD);
    assert_eq!(chat.note, None);
    // The extra link to another site is dropped, the one to Hugging Face stays.
    assert_eq!(chat.more.len(), 1);
    assert_eq!((chat.more[0].label.as_str(), chat.more[0].url.as_str()), ("Original", "https://huggingface.co/example-lab/Example-Chat"));

    let video = &c.models[1];
    assert_eq!((video.kind.as_str(), video.status.as_str()), ("video", LISTED));
    assert_eq!(video.note.as_deref(), Some("Non-commercial licence"));

    // A kind and a status this build has not heard of: kept, and only to look at.
    let new = &c.models[2];
    assert_eq!((new.kind.as_str(), new.status.as_str()), ("smell", LISTED));
}

#[test]
fn only_plain_links_to_the_two_sites_pass() {
    for url in [
        "https://huggingface.co/owner/model",
        "https://github.com/owner/project",
        "https://github.com/owner/project/releases/tag/v1",
        "https://huggingface.co/owner/model?library=llama-cpp",
    ] {
        assert!(one(model(url)).is_some(), "{url}");
    }
    for url in [
        "",
        "http://huggingface.co/owner/model",
        "https://example.com/owner/model",
        "https://huggingface.co.evil.example/owner/model",
        "https://evil.example/huggingface.co/owner/model",
        "https://user:pw@huggingface.co/owner/model",
        "https://huggingface.co@evil.example/owner/model",
        "https://huggingface.co:8443/owner/model",
        "https://huggingface.co\\@evil.example/owner/model",
        "https://raw.githubusercontent.com/owner/project/main/x",
        "https://gist.github.com/owner/abc",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "huggingface.co/owner/model",
        "https://huggingface.co/owner/mo del",
    ] {
        assert!(one(model(url)).is_none(), "{url}");
    }
    // A link that is not a string, and an entry that is not an object.
    assert!(one(json!({"id": "m", "name": "M", "url": 7})).is_none());
    assert!(one(json!("https://huggingface.co/owner/model")).is_none());
    // No id or no name: nothing to show.
    assert!(one(json!({"id": "m", "url": "https://huggingface.co/owner/model"})).is_none());
    assert!(one(json!({"name": "M", "url": "https://huggingface.co/owner/model"})).is_none());
}

#[test]
fn add_is_only_for_chat_models_the_lookup_accepts() {
    assert_eq!(one(model("https://huggingface.co/owner/model")).unwrap().status, ADD);
    let mut picture = model("https://huggingface.co/owner/model");
    picture["kind"] = json!("image");
    assert_eq!(one(picture).unwrap().status, LISTED);
    // Hugging Face pages that are not models cannot be looked up.
    assert_eq!(one(model("https://huggingface.co/spaces/owner/demo")).unwrap().status, LISTED);
    let mut missing = model("https://huggingface.co/owner/model");
    missing.as_object_mut().unwrap().remove("status");
    assert_eq!(one(missing).unwrap().status, LISTED);
    // Missing words are empty, never an error.
    let bare = one(json!({"id": "m", "name": "M", "url": "https://github.com/owner/project"})).unwrap();
    assert_eq!((bare.kind.as_str(), bare.by.as_str(), bare.license.as_str(), bare.about.as_str()), ("", "", "", ""));
    assert!(bare.more.is_empty());
}

#[test]
fn sizes_are_capped() {
    // Strings are cut, control characters go.
    let mut long = model("https://huggingface.co/owner/model");
    long["name"] = json!("N".repeat(5000));
    long["about"] = json!(format!("line one\nline two\u{0007} {}", "a".repeat(5000)));
    long["note"] = json!("n".repeat(5000));
    long["more"] = json!((0..50)
        .map(|i| json!({"label": format!("L{i} {}", "x".repeat(500)), "url": format!("https://github.com/owner/p{i}")}))
        .collect::<Vec<_>>());
    let e = one(long).unwrap();
    assert_eq!(e.name.chars().count(), 80);
    assert_eq!(e.about.chars().count(), 200);
    assert!(e.about.starts_with("line one line two"));
    assert!(!e.about.chars().any(char::is_control));
    assert_eq!(e.note.unwrap().chars().count(), 120);
    assert_eq!(e.more.len(), catalog::MAX_MORE);
    assert!(e.more.iter().all(|l| l.label.chars().count() <= 40));

    // No more than 500 entries, and an id is used once.
    let many: Vec<_> = (0..700).map(|i| json!({"id": format!("m{i}"), "name": "M", "url": "https://github.com/owner/project"})).collect();
    let c = catalog::parse(&json!({"models": many}).to_string()).unwrap();
    assert_eq!(c.models.len(), catalog::MAX_ENTRIES);
    let twice = json!({"models": [model("https://huggingface.co/owner/one"), model("https://huggingface.co/owner/two")]});
    let c = catalog::parse(&twice.to_string()).unwrap();
    assert_eq!(c.models.len(), 1);
    assert_eq!(c.models[0].url, "https://huggingface.co/owner/one");

    // A file over 512 KB is refused whole, before anything in it is read.
    let big = format!("{{\"models\": [], \"pad\": \"{}\"}}", "x".repeat(catalog::MAX_BYTES));
    assert!(matches!(catalog::parse(&big), Err(CatalogError::TooBig(_))));
    assert!(matches!(catalog::parse_signed(&big, SIG, &test_key()), Err(CatalogError::TooBig(_))));
}

#[test]
fn a_file_that_is_not_a_catalog_is_refused() {
    assert!(matches!(catalog::parse("not json"), Err(CatalogError::Invalid(_))));
    assert!(matches!(catalog::parse("[]"), Err(CatalogError::Invalid(_))));
    // The manifest is signed with the same key: it must not pass for a catalog.
    assert!(matches!(catalog::parse(r#"{"schema": 1, "packages": []}"#), Err(CatalogError::Invalid(_))));
    assert!(matches!(catalog::parse(r#"{"models": "none"}"#), Err(CatalogError::Invalid(_))));
    assert_eq!(catalog::parse(r#"{"models": []}"#).unwrap(), catalog::Catalog::default());
}

#[test]
fn the_signature_decides() {
    assert_eq!(catalog::parse_signed(CATALOG, SIG, &test_key()).unwrap().models.len(), 3);
    let tampered = CATALOG.replace("example-owner", "example-0wner");
    assert!(matches!(catalog::parse_signed(&tampered, SIG, &test_key()), Err(CatalogError::BadSignature(_))));
    // Rigbrew's real key did not sign the fixture.
    assert!(matches!(catalog::parse_signed(CATALOG, SIG, PUBLIC_KEY), Err(CatalogError::BadSignature(_))));
    for sig in ["", "not a signature", "untrusted comment: x\nAAAA\ntrusted comment: y\nBBBB\n"] {
        assert!(matches!(catalog::parse_signed(CATALOG, sig, &test_key()), Err(CatalogError::BadSignature(_))), "{sig:?}");
    }
}

#[test]
fn the_kept_copy_is_shown_when_the_fetch_fails() {
    let (root, key) = (temp_root(), test_key());
    let fetched = || Ok((CATALOG.to_string(), SIG.to_string()));

    // Nothing kept and no network: an empty list, not an error.
    let none = catalog::settle(&root, Err("no route to host".into()), &key);
    assert!(none.catalog.models.is_empty());
    assert!(none.stale);
    assert_eq!(none.problem.as_deref(), Some("no route to host"));
    assert!(!catalog::kept_path(&root).exists());

    // A file with a bad signature is not shown and not kept.
    let bad = catalog::settle(&root, Ok((CATALOG.replace("Example", "Evil"), SIG.to_string())), &key);
    assert!(bad.catalog.models.is_empty() && bad.stale);
    assert!(bad.problem.unwrap().contains("signature"));
    assert!(!catalog::kept_path(&root).exists());
    // Neither is one signed with another key.
    assert!(catalog::settle(&root, fetched(), PUBLIC_KEY).catalog.models.is_empty());
    assert!(!catalog::kept_path(&root).exists());

    // A good one is shown and kept.
    let good = catalog::settle(&root, fetched(), &key);
    assert_eq!((good.catalog.models.len(), good.stale, good.problem.clone()), (3, false, None));
    assert_eq!(std::fs::read_to_string(catalog::kept_path(&root)).unwrap(), CATALOG);

    // The network goes away, or serves something unsigned: the kept copy, marked stale.
    let offline = catalog::settle(&root, Err("timed out".into()), &key);
    assert_eq!(offline.catalog, good.catalog);
    assert!(offline.stale);
    let forged = catalog::settle(&root, Ok((r#"{"models": []}"#.to_string(), SIG.to_string())), &key);
    assert_eq!(forged.catalog, good.catalog);
    assert!(forged.stale);
    assert_eq!(std::fs::read_to_string(catalog::kept_path(&root)).unwrap(), CATALOG, "the kept copy is not replaced");

    // A kept copy that was edited on disk no longer passes its signature and is not shown.
    std::fs::write(catalog::kept_path(&root), CATALOG.replace("example-owner", "someone-else")).unwrap();
    let edited = catalog::settle(&root, Err("offline".into()), &key);
    assert!(edited.catalog.models.is_empty() && edited.stale);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_local_file_with_a_bad_signature_is_refused() {
    let root = temp_root();
    let file = root.join("community.json");
    std::fs::write(&file, CATALOG).unwrap();
    std::fs::write(root.join("community.json.minisig"), SIG).unwrap();
    assert_eq!(catalog::load_file(&file, &test_key()).unwrap().models.len(), 3);
    assert!(matches!(catalog::load_file(&file, PUBLIC_KEY), Err(CatalogError::BadSignature(_))));
    std::fs::write(&file, CATALOG.replace("Example", "Evil")).unwrap();
    assert!(matches!(catalog::load_file(&file, &test_key()), Err(CatalogError::BadSignature(_))));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_model_that_was_added_is_marked() {
    let c = catalog::parse(CATALOG).unwrap();
    let (chat, video) = (&c.models[0], &c.models[1]);
    let have = |owner: &str, repo: &str| {
        let target = Target { host: Host::HuggingFace, owner: owner.into(), repo: repo.into(), rev: None, file: None };
        let found = community::read_hf_model(
            &target,
            &json!({"sha": "abc123", "siblings": [{"rfilename": "model-Q4_K_M.gguf", "size": 1000, "lfs": {"sha256": "c".repeat(64)}}]}),
        )
        .unwrap();
        vec![community::package(&found, &found.files[0])]
    };
    assert!(!catalog::added(chat, &[]));
    assert!(catalog::added(chat, &have("example-owner", "Example-Chat-GGUF")));
    // The two sites do not tell capital letters apart in names.
    assert!(catalog::added(chat, &have("Example-Owner", "example-chat-gguf")));
    assert!(!catalog::added(chat, &have("example-owner", "Another-GGUF")));
    assert!(!catalog::added(chat, &have("someone-else", "Example-Chat-GGUF")));
    // A model that cannot be added is never "added", whatever is in the list.
    let mut same_repo = have("example-studio", "example-video");
    same_repo[0].host = Host::GitHub;
    assert!(!catalog::added(video, &same_repo));
}
