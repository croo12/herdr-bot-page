use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use syn::{
    visit::{self, Visit},
    ItemMod, ItemUse, UseTree,
};

// Each pair is one reviewed dependency direction. Removing the last use also
// requires removing that allowance, so a former cycle can't silently return.
const DEPENDENCIES: &str = include_str!("architecture/dependencies.txt");

// Outer to inner. Feature folders belong to the modules layer.
const LAYERS: &[&[&str]] = &[
    &["runtime"],
    &["controller"],
    &["application"],
    &["modules"],
    &["storage", "infrastructure"],
    &["domain"],
];

fn layer(root: &str) -> Option<usize> {
    LAYERS.iter().position(|names| names.contains(&root))
}

fn points_inward(owner: &str, target: &str) -> bool {
    let root = |boundary: &str| boundary.split("::").next().and_then(layer);
    let Some(target_layer) = root(target) else {
        return false;
    };
    owner == "lib" || root(owner).is_some_and(|owner_layer| owner_layer < target_layer)
}

fn boundary(module: &[String]) -> String {
    match module {
        [root, feature, ..] if root == "modules" => format!("modules::{feature}"),
        [root, ..] => root.clone(),
        [] => "lib".into(),
    }
}

#[derive(Default)]
struct References {
    module: Vec<String>,
    root_modules: BTreeSet<String>,
    targets: BTreeSet<String>,
    exposed_modules: Vec<String>,
}

impl References {
    fn record(&mut self, segments: &[String]) {
        let Some(first) = segments.first() else {
            return;
        };
        let mut resolved = self.module.clone();
        match first.as_str() {
            "crate" => resolved.clear(),
            "self" | "super" => {}
            _ if resolved.is_empty() && self.root_modules.contains(first) => {}
            _ => return, // local paths or names already accounted for by a use declaration
        }
        for segment in segments {
            match segment.as_str() {
                "super" => {
                    resolved.pop();
                }
                "crate" | "self" | "*" => {}
                _ => resolved.push(segment.clone()),
            }
        }
        let target = match resolved.first().map(String::as_str) {
            Some("Engine") => "runtime".into(),
            Some("ipc" | "wire_error") => "controller".into(),
            Some("default_home") => "infrastructure".into(),
            _ => boundary(&resolved),
        };
        if target != boundary(&self.module) {
            self.targets.insert(target);
        }
    }

    fn use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        let leaf = match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.use_tree(&path.tree, prefix);
                prefix.pop();
                return;
            }
            UseTree::Group(group) => {
                for child in &group.items {
                    self.use_tree(child, prefix);
                }
                return;
            }
            UseTree::Glob(_) => "*".into(),
            UseTree::Name(name) => name.ident.to_string(),
            UseTree::Rename(rename) => rename.ident.to_string(),
        };
        prefix.push(leaf);
        self.record(prefix);
        prefix.pop();
    }
}

impl<'ast> Visit<'ast> for References {
    fn visit_visibility(&mut self, _: &'ast syn::Visibility) {} // pub(crate) is not a dependency.

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.use_tree(&item.tree, &mut Vec::new());
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.record(
            &path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
        );
        visit::visit_path(self, path);
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if matches!(self.module.as_slice(), [layer, _] if layer == "modules")
            && !matches!(item.vis, syn::Visibility::Inherited)
        {
            self.exposed_modules.push(item.ident.to_string());
        }
        self.module.push(item.ident.to_string());
        visit::visit_item_mod(self, item);
        self.module.pop();
    }
}

type Edges = BTreeMap<(String, String), BTreeSet<String>>;

fn inspect(directory: &Path, root: &Path, edges: &mut Edges, errors: &mut Vec<String>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let relative = path.strip_prefix(root).unwrap();
        if relative == Path::new("bin") && path.is_dir() {
            continue;
        } // CLI is a separate crate target.
        if directory == root && relative != Path::new("lib.rs") {
            let name = if path.is_dir() {
                path.file_name()
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                path.file_stem()
            } else {
                continue;
            };
            if name
                .and_then(|name| name.to_str())
                .and_then(layer)
                .is_none()
            {
                errors.push(format!(
                    "unknown engine layer at native/src/{}",
                    relative.display()
                ));
            }
        }
        if path.is_dir() {
            inspect(&path, root, edges, errors);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let mut module: Vec<_> = relative
                .with_extension("")
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect();
            if matches!(module.last().map(String::as_str), Some("lib" | "mod")) {
                module.pop();
            }
            let owner = boundary(&module);
            let mut refs = References {
                module,
                ..References::default()
            };
            let source = fs::read_to_string(&path).unwrap();
            let file =
                syn::parse_file(&source).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            if relative == Path::new("lib.rs") {
                for item in &file.items {
                    let syn::Item::Mod(module) = item else {
                        continue;
                    };
                    let name = module.ident.to_string();
                    if layer(&name).is_none() {
                        errors.push(format!(
                            "unknown engine layer declared in native/src/lib.rs: {name}"
                        ));
                    } else if module.content.is_some()
                        || module
                            .attrs
                            .iter()
                            .any(|attribute| attribute.path().is_ident("path"))
                    {
                        errors.push(format!("declare layer {name} in native/src/lib.rs with an external mod {name}; so its code is checked under its own layer"));
                    }
                    refs.root_modules.insert(name);
                }
            }
            refs.visit_file(&file);
            assert!(
                refs.exposed_modules.is_empty(),
                "{} exposes {:?}; keep feature submodules private and re-export individual items",
                path.display(),
                refs.exposed_modules
            );
            for target in refs.targets {
                edges
                    .entry((owner.clone(), target))
                    .or_default()
                    .insert(format!("native/src/{}", relative.display()));
            }
        }
    }
}

fn parse_edges(policy: &str) -> BTreeSet<(String, String)> {
    let mut allowed = BTreeSet::new();
    for line in policy.lines() {
        let (owner, targets) = line.split_once(" -> ").expect("owner -> target ...");
        for target in targets.split_whitespace() {
            allowed.insert((owner.to_owned(), target.to_owned()));
        }
    }
    allowed
}

fn dependency_errors(root: &Path, policy: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let mut actual = Edges::new();
    inspect(root, root, &mut actual, &mut errors);
    let mut allowed = parse_edges(policy);
    for ((owner, target), files) in actual {
        let pair = (owner.clone(), target.clone());
        let problem = if points_inward(&owner, &target) {
            if allowed.remove(&pair) {
                continue;
            }
            "unreviewed"
        } else {
            "forbidden layer dependency"
        };
        errors.push(format!(
            "{problem} {owner} -> {target} in {}",
            files.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    for (owner, target) in allowed {
        errors.push(format!("remove unused allowance {owner} -> {target}"));
    }
    errors
}

#[test]
fn engine_module_dependencies_match_reviewed_edges() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let errors = dependency_errors(&root, DEPENDENCIES);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn a_feature_cannot_import_another_feature_even_with_an_allowance() {
    let root = tempfile::tempdir().unwrap();
    let owner = root.path().join("modules/turns");
    fs::create_dir_all(&owner).unwrap();
    fs::write(
        owner.join("service.rs"),
        "use crate::modules::tasks as delegated;",
    )
    .unwrap();
    let policy = "modules::turns -> modules::tasks";
    let errors = dependency_errors(root.path(), policy);
    assert!(errors.iter().any(|error| error
        .contains("forbidden layer dependency modules::turns -> modules::tasks")
        && error.ends_with("service.rs")));
    assert!(errors.contains(&format!("remove unused allowance {policy}")));
}

#[test]
fn runtime_and_storage_reverse_edges_are_forbidden() {
    let root = tempfile::tempdir().unwrap();
    let feature = root.path().join("modules/chats");
    let storage = root.path().join("storage");
    fs::create_dir_all(&feature).unwrap();
    fs::create_dir(&storage).unwrap();
    fs::write(feature.join("mod.rs"), "use crate::Engine;").unwrap();
    fs::write(storage.join("mod.rs"), "use crate::modules::chats::Chats;").unwrap();
    let policy = "modules::chats -> runtime\nstorage -> modules::chats";
    let errors = dependency_errors(root.path(), policy);
    for edge in policy.lines() {
        assert!(errors
            .iter()
            .any(|error| error.starts_with(&format!("forbidden layer dependency {edge} in "))));
    }
}

#[test]
fn unknown_roots_fail_even_without_dependencies() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("managers")).unwrap();
    fs::create_dir_all(root.path().join("modules/tasks/scheduling")).unwrap();
    fs::write(root.path().join("helpers.rs"), "").unwrap();
    fs::write(root.path().join("lib.rs"), "mod workflows {} mod domain {}").unwrap();
    let errors = dependency_errors(root.path(), "");
    assert_eq!(errors.len(), 4, "{errors:?}");
    for name in ["managers", "helpers.rs", "workflows"] {
        assert!(errors.iter().any(|error| error.ends_with(name)));
    }
    assert!(errors
        .iter()
        .any(|error| error.contains("external mod domain")));
}

#[test]
fn a_reviewed_edge_cannot_override_the_layer_order() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("domain")).unwrap();
    fs::write(
        root.path().join("domain/mod.rs"),
        "use crate::application::post_reply;",
    )
    .unwrap();
    let errors = dependency_errors(root.path(), "domain -> application");
    assert!(errors
        .iter()
        .any(|error| error.contains("forbidden layer dependency domain -> application")));
    assert!(points_inward("controller", "modules::chats"));
    assert!(!points_inward("infrastructure", "storage"));
}

#[test]
fn recognizes_nested_imports_aliases_and_paths_in_function_bodies() {
    let mut refs = References {
        module: vec!["modules".into(), "tasks".into(), "worker".into()],
        ..References::default()
    };
    refs.visit_file(
        &syn::parse_file(
            r#"
        use crate::{storage::Repositories, modules::{turns as queue, bots::connect_for_turn}};
        fn work() { crate::modules::chats::change(); }
        mod nested { fn other() { super::super::super::reviews::check(); } }
        // crate::modules::fake::ignored()
    "#,
        )
        .unwrap(),
    );
    assert_eq!(
        refs.targets.into_iter().collect::<Vec<_>>(),
        [
            "modules::bots",
            "modules::chats",
            "modules::reviews",
            "modules::turns",
            "storage"
        ]
    );
}
