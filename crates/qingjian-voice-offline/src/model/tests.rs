use std::time::{Duration, SystemTime};

use super::{ModelFile, SpeechModel};

fn temp_root(name: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("mengren-voice-model-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn small_model(root: &std::path::Path) -> SpeechModel {
    SpeechModel::with_source(
        root,
        "https://example.invalid/",
        vec![
            ModelFile {
                name: "model.int8.onnx",
                size: 3,
                sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            },
            ModelFile {
                name: "tokens.txt",
                size: 1,
                sha256: "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
            },
        ],
    )
}

#[test]
fn installed_needs_marker_and_sizes() {
    let root = temp_root("installed");
    let model = small_model(&root);
    assert!(!model.installed());
    std::fs::create_dir_all(model.dir()).unwrap();
    std::fs::write(model.model_path(), b"abc").unwrap();
    std::fs::write(model.tokens_path(), b"a").unwrap();
    assert!(!model.installed(), "no marker yet");
    std::fs::write(model.dir().join(SpeechModel::VERIFIED), b"ok").unwrap();
    assert!(model.installed());
    std::fs::write(model.tokens_path(), b"ab").unwrap();
    assert!(!model.installed(), "size changed");
    assert_eq!(model.size(), 3 + 2 + 2);
    model.delete().unwrap();
    assert!(!root.exists());
    model.delete().unwrap();
}

#[test]
fn cleanup_drops_other_versions_and_stale_partials() {
    let root = temp_root("cleanup");
    let model = small_model(&root);
    std::fs::create_dir_all(model.dir()).unwrap();
    std::fs::create_dir_all(root.join("sensevoice-old")).unwrap();
    std::fs::write(root.join("sensevoice-old").join("x"), b"x").unwrap();
    std::fs::write(model.dir().join("model.int8.onnx.part"), b"ab").unwrap();
    std::fs::write(model.tokens_path(), b"a").unwrap();
    model.cleanup(SystemTime::now());
    assert!(!root.join("sensevoice-old").exists());
    assert!(
        model.dir().join("model.int8.onnx.part").exists(),
        "fresh partial kept"
    );
    model.cleanup(SystemTime::now() + SpeechModel::PARTIAL_RETENTION + Duration::from_secs(60));
    assert!(
        !model.dir().join("model.int8.onnx.part").exists(),
        "stale partial removed"
    );
    assert!(model.tokens_path().exists(), "finished files untouched");
    model.delete().unwrap();
}

#[test]
fn published_files_add_up() {
    let model = SpeechModel::new("unused");
    assert_eq!(model.total_bytes(), 239_233_841 + 315_894);
    assert!(
        model
            .model_path()
            .ends_with("sensevoice-int8-2024-07-17/model.int8.onnx")
    );
}
