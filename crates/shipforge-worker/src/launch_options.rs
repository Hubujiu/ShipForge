use crate::config::{Config, Result, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub models: BTreeMap<String, Vec<String>>,
    pub default_model: Option<String>,
    pub model_flag: Option<String>,
    pub effort_flag: Option<String>,
    pub allowed_arguments: BTreeMap<String, Vec<String>>,
    pub default_arguments: BTreeMap<String, String>,
    pub environment: BTreeMap<String, String>,
    pub allowed_environment: BTreeSet<String>,
}
fn flag(s: &str) -> bool {
    s.len() > 2
        && s.len() <= 128
        && s.starts_with("--")
        && s[2..]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
}
fn env_key(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    s.len() <= 128
        && s.bytes()
            .next()
            .is_some_and(|c| c == b'_' || c.is_ascii_alphabetic())
        && s.bytes().all(|c| c == b'_' || c.is_ascii_alphanumeric())
        && ![
            "SHIPFORGE_",
            "LD_",
            "DYLD_",
            "GIT_CONFIG",
            "GIT_EXEC",
            "GIT_SSH",
            "BASH_FUNC_",
            "NPM_CONFIG_",
            "CARGO_",
            "RUSTC_",
            "COMPLUS_",
            "DOTNET_",
        ]
        .iter()
        .any(|p| u.starts_with(p))
        && !matches!(
            u.as_str(),
            "PATH"
                | "PATHEXT"
                | "SYSTEMROOT"
                | "WINDIR"
                | "HOME"
                | "USERPROFILE"
                | "TMP"
                | "TEMP"
                | "TMPDIR"
                | "COMSPEC"
                | "PYTHONPATH"
                | "PYTHONHOME"
                | "PYTHONSTARTUP"
                | "NODE_OPTIONS"
                | "NODE_PATH"
                | "RUBYOPT"
                | "PERL5OPT"
                | "PERL5LIB"
                | "BASH_ENV"
                | "ENV"
                | "SSLKEYLOGFILE"
        )
}
fn fixed<'a>(spec: &'a Runtime, flag: &str) -> Option<&'a str> {
    spec.args.iter().enumerate().find_map(|(i, a)| {
        a.strip_prefix(&format!("{flag}=")).or_else(|| {
            (a == flag)
                .then(|| spec.args.get(i + 1).map(String::as_str))
                .flatten()
        })
    })
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self
            .model_flag
            .iter()
            .chain(self.effort_flag.iter())
            .chain(self.allowed_arguments.keys())
            .any(|s| !flag(s))
            || self.model_flag.is_some() && self.model_flag == self.effort_flag
            || self.allowed_arguments.keys().any(|k| {
                Some(k) == self.model_flag.as_ref() || Some(k) == self.effort_flag.as_ref()
            })
        {
            return Err("invalid or duplicated runtime option flag".into());
        }
        if self
            .default_model
            .as_ref()
            .is_some_and(|s| !self.models.contains_key(s))
            || self.default_arguments.iter().any(|(k, v)| {
                !self
                    .allowed_arguments
                    .get(k)
                    .is_some_and(|vs| vs.contains(v))
            })
        {
            return Err("runtime defaults must use advertised choices".into());
        }
        if self
            .environment
            .keys()
            .chain(&self.allowed_environment)
            .any(|k| !env_key(k))
        {
            return Err("invalid or reserved environment name in runtime profile".into());
        }
        Ok(())
    }
    fn model_flag<'a>(&'a self, name: &str) -> Option<&'a str> {
        (!name.eq_ignore_ascii_case("openclaw"))
            .then_some(self.model_flag.as_deref())
            .flatten()
    }
    pub fn public(&self, name: &str, spec: &Runtime) -> Value {
        let fixed = self.model_flag(name).and_then(|f| fixed(spec, f));
        json!({"models":self.models,"default_model":self.default_model,"fixed_model":fixed,"argument_choices":self.allowed_arguments,"environment_names":self.allowed_environment})
    }
}
pub fn effective(
    config: &Config,
    name: &str,
    run: &Value,
    spec: &Runtime,
) -> Result<(Runtime, BTreeMap<String, String>)> {
    let mut spec = spec.clone();
    let Some(profile) = config.runtime_launch.get(name) else {
        return Ok((spec, BTreeMap::new()));
    };
    let fields = &run["fields"];
    let agent = &fields["config"];
    let selected = fields["model_override"]
        .as_str()
        .or_else(|| agent["model"].as_str())
        .filter(|s| !s.is_empty())
        .or(profile.default_model.as_deref());
    let model_flag = profile.model_flag(name);
    let locked = model_flag
        .and_then(|flag| fixed(&spec, flag))
        .map(str::to_owned);
    let model = locked.as_deref().or(selected);
    if let Some(model) = model
        && locked.is_none()
    {
        if !profile.models.contains_key(model) {
            return Err("model is not advertised by the configured runtime".into());
        }
        if let Some(flag) = model_flag {
            spec.args.extend([flag.into(), model.into()]);
        }
    }
    let same_model = fields["model_override"]
        .as_str()
        .is_none_or(|m| Some(m) == agent["model"].as_str().or(profile.default_model.as_deref()));
    if let Some(effort) = fields["effort_override"]
        .as_str()
        .or_else(|| {
            same_model
                .then(|| agent["reasoning_effort"].as_str())
                .flatten()
        })
        .filter(|s| !s.is_empty())
    {
        if !model
            .and_then(|m| profile.models.get(m))
            .is_some_and(|list| list.iter().any(|s| s == effort))
        {
            return Err("model does not advertise the selected reasoning effort".into());
        }
        if let Some(flag) = &profile.effort_flag
            && fixed(&spec, flag).is_none()
        {
            spec.args.extend([flag.clone(), effort.into()]);
        }
    }
    let mut options = profile.default_arguments.clone();
    let mut attrs = agent["arguments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    while let Some(option) = attrs.next() {
        let (key, value) = if let Some(pair) = option.split_once('=') {
            pair
        } else {
            (
                option,
                attrs
                    .next()
                    .ok_or("runtime option requires an approved value")?,
            )
        };
        if !profile
            .allowed_arguments
            .get(key)
            .is_some_and(|allowed| allowed.iter().any(|s| s == value))
        {
            return Err("runtime argument or value is not approved on this machine".into());
        }
        options.insert(key.into(), value.into());
    }
    for (k, v) in options {
        if let Some(i) = spec.args.iter().position(|a| a == &k) {
            if i + 1 >= spec.args.len() {
                return Err("incomplete machine runtime argument".into());
            }
            spec.args[i + 1] = v;
        } else if let Some(i) = spec
            .args
            .iter()
            .position(|a| a.starts_with(&format!("{k}=")))
        {
            spec.args[i] = format!("{k}={v}");
        } else {
            spec.args.extend([k, v]);
        }
    }
    let mut env = profile.environment.clone();
    for (k, v) in agent["environment"].as_object().into_iter().flatten() {
        if !env_key(k) || !profile.allowed_environment.contains(k) {
            return Err("environment name is not approved for this runtime".into());
        }
        env.insert(
            k.clone(),
            v.as_str().ok_or("invalid environment value")?.into(),
        );
    }
    Ok((spec, env))
}
