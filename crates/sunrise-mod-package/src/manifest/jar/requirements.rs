/// One exact file and every Sunrise mod requiring it, in load order.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "RequirementFields")]
pub struct JarRequirement {
    pub owners: Vec<String>,
    pub jar: JarRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequirementFields {
    owners: Vec<String>,
    jar: JarRef,
}

impl TryFrom<RequirementFields> for JarRequirement {
    type Error = String;

    fn try_from(fields: RequirementFields) -> Result<Self, Self::Error> {
        let unique: std::collections::BTreeSet<_> = fields.owners.iter().collect();
        if fields.owners.is_empty()
            || fields.owners.iter().any(|id| !super::valid_mod_id(id))
            || unique.len() != fields.owners.len()
        {
            return Err(format!("invalid jar owners: {:?}", fields.owners));
        }
        Ok(Self {
            owners: fields.owners,
            jar: fields.jar,
        })
    }
}
