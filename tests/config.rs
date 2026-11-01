// Configuration as the Node.js build loads it (tests/vectors/config.json).

mod common;

use vizrs_cg::config::Config;
use vizrs_cg::json::Value;

use common::{assert_same, vectors};

#[test]
fn defaults_match_node() {
    let v = vectors("config");
    assert_same(&Config::default().to_json(), v.get("defaults").unwrap(), "defaults");
}

#[test]
fn file_and_environment_match_node() {
    let v = vectors("config");
    for case in v.get("cases").unwrap().as_array().unwrap() {
        let env = case.get("env").unwrap().clone();
        let lookup = move |name: &str| env.get(name).and_then(Value::as_str).map(str::to_owned);
        let file = case.get("file").filter(|f| !f.is_null());
        let (config, _) = Config::from_values(file, &lookup).unwrap();
        assert_same(
            &config.to_json(),
            case.get("config").unwrap(),
            &format!("file {:?}, env {}", file.map(|f| f.to_string()), case.get("env").unwrap()),
        );
    }
}

#[test]
fn file_on_disk() {
    let dir = std::env::temp_dir().join(format!("vizrs-cg-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.json");
    std::fs::write(&path, r#"{"on_clip":"TAKE 1","vizrs":{"address":"vizrs:6100"}}"#).unwrap();
    let (config, _) = Config::load(Some(&path)).unwrap();
    assert_eq!((config.on_clip.as_str(), config.vizrs.address.as_str()), ("TAKE 1", "vizrs:6100"));

    std::fs::write(&path, "{").unwrap();
    assert!(Config::load(Some(&path)).unwrap_err().contains("config.json"));
    assert!(Config::load(Some(&dir.join("missing.json"))).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
