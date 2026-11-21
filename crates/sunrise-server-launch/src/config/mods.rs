//! The managed mod input copied verbatim in `mods.lock`.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mods {
    pub pack: Option<String>,
    pub list: Vec<String>,
    pub off: Vec<String>,
    pub trust_native_code: bool,
}

impl Mods {
    /// Check mod input read from `server.toml` or from a lock's copy of it.
    pub fn check(&self) -> Result<(), String> {
        use sunrise_mod_package::manifest::{parse_version, valid_mod_id};

        if let Some(pack) = &self.pack
            && !(pack.ends_with(".spack") && !pack.contains(['\0', '\n', '\r'])
                || pack.split_once('@').is_some_and(|(id, version)| {
                    valid_mod_id(id) && parse_version(version).is_ok()
                }))
        {
            return Err(format!(
                "mods.pack {pack:?}: use id@version or a .spack path"
            ));
        }
        for row in &self.list {
            let (id, range) = row
                .split_once('@')
                .map_or((row.as_str(), None), |(id, range)| (id, Some(range)));
            if !valid_mod_id(id)
                || range.is_some_and(|range| semver::VersionReq::parse(range).is_err())
            {
                return Err(format!("mods.list {row:?}: use id or id@semver-range"));
            }
        }
        if let Some(id) = self.off.iter().find(|id| !valid_mod_id(id)) {
            return Err(format!("mods.off {id:?}: not a mod ID"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn pack_can_have_extras_and_disabled_members() {
        let config = Config::parse(
            r#"
[mods]
pack = "cozy@1.0.0"
list = ["rails@^2", "windows"]
off = ["pack_member"]
trust_native_code = true
"#,
        )
        .unwrap();
        let mods = config.mods.unwrap();
        assert_eq!(mods.pack.as_deref(), Some("cozy@1.0.0"));
        assert_eq!(mods.list, ["rails@^2", "windows"]);
        assert_eq!(mods.off, ["pack_member"]);
        assert!(mods.trust_native_code);
        assert!(Config::parse("[mods]\npack = './local.sp aces.spack'").is_ok());
        assert!(Config::parse("[mods]").unwrap().mods.is_some());
        assert!(Config::parse("").unwrap().mods.is_none());
    }

    #[test]
    fn rejects_invalid_mod_settings() {
        for field in [
            "unknown = true",
            "pack = 'cozy@bad'",
            "list = ['bad/id']",
            "list = ['rails@']",
            "list = ['rails@not-a-range']",
            "off = ['rails@2']",
            "trust_native_code = 'true'",
        ] {
            assert!(
                Config::parse(&format!("[mods]\n{field}")).is_err(),
                "{field}"
            );
        }
    }
}
