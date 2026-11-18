//! The machine settings a NixOS flake declares under `options.my`, read in
//! one nix eval, and the file Nix App Center writes the changed ones to.

use crate::config_layout::{detect, local_overrides, Layout, LayoutKind};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The generated file holding the settings changed in Nix App Center.
pub(crate) const SETTINGS_FILE: &str = "nix-app-center-settings.nix";

/// Applied to the host: every visible, writable `my.*` option, with its
/// kind, value, default, enum values and who defines it. Submodule options
/// are split into their own fields, three levels deep.
const EVAL: &str = r#"c:
let
  lib = c.pkgs.lib;
  own = "nix-app-center-settings.nix";
  try = v: let r = builtins.tryEval (builtins.deepSeq v v); in if r.success then r.value else null;
  shallow = v: let r = builtins.tryEval v; in if r.success then r.value else null;
  text = v: if v == null then null else if builtins.isString v then v else v.text or (try (lib.generators.toPretty { } v));
  enumValues = t: t.functor.payload.values or t.functor.payload;
  # nullOr X edits as X, empty meaning null.
  base = t: if t.name == "nullOr" then t.nestedTypes.elemType else t;
  kind = t': let t = base t'; in
    if t.name == "bool" then "bool"
    else if t.name == "enum" && lib.all builtins.isString (enumValues t) then "enum"
    else if lib.elem t.name [ "str" "nonEmptyStr" "singleLineStr" ] then "str"
    else if t.name == "float" then "float"
    else if t.name == "int" || lib.hasSuffix "Int" t.name || t.name == "intBetween" then "int"
    else if t.name == "listOf" && t.nestedTypes.elemType.name == "str" then "list"
    else "other";
  submodule = t:
    if t.name == "submodule" then t
    else if t.name == "nullOr" && t.nestedTypes.elemType.name == "submodule" then t.nestedTypes.elemType
    else null;
  shown = o: (o.visible or true) == true && !(o.readOnly or false) && !(lib.any (lib.hasPrefix "_") o.loc);
  defaultOf = o:
    if o ? defaultText then text o.defaultText
    else if o ? default then try (lib.generators.toPretty { } o.default)
    else null;
  entry = depth: elsewhere: priority: o: value: mine:
    let
      k = kind o.type;
      sub = submodule o.type;
      at = path: v: if builtins.isAttrs v then lib.attrByPath path null v else null;
      child = c': let rel = lib.drop (builtins.length o.loc) c'.loc; in
        entry (depth + 1) elsewhere priority c' (shallow (at rel value)) (at rel mine);
    in
    if sub != null && depth < 3 then
      lib.concatMap child (lib.filter shown (lib.collect lib.isOption (sub.getSubOptions o.loc)))
    else [{
      name = lib.showOption o.loc;
      description = text (o.description or null);
      kind = k;
      value = if k == "other" then try (lib.generators.toPretty { } value) else try value;
      default = defaultOf o;
      values = if k == "enum" then enumValues (base o.type) else [ ];
      nullable = o.type.name == "nullOr";
      own = if k == "other" then null else try mine;
      inherit elsewhere priority;
    }];
  top = o:
    let
      defs = lib.defaultTo [ ] (shallow (map (x: { file = baseNameOf x.file; inherit (x) value; }) o.definitionsWithLocations));
      mine = lib.findFirst (x: x.file == own) null defs;
      priority = o.highestPrio or 1500;
      elsewhere = if priority > 100 then [ ] else map (x: x.file) (lib.filter (x: x.file != own) defs);
    in
    entry 0 elsewhere priority o (shallow o.value) (if mine == null then null else mine.value);
in
lib.concatMap top (lib.filter shown (lib.collect lib.isOption (c.options.my or { })))
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    Bool,
    Enum,
    Str,
    Int,
    Float,
    List,
    /// Anything else: shown, not edited.
    Other,
}

/// One `my.*` option of the configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Setting {
    pub name: String,
    pub description: Option<String>,
    pub kind: SettingKind,
    /// The merged value; for [`SettingKind::Other`], its Nix rendering.
    pub value: Option<Value>,
    /// The default, as Nix text.
    pub default: Option<String>,
    /// The choices of an enum.
    pub values: Vec<String>,
    /// Null is a valid value: an empty field.
    #[serde(default)]
    pub nullable: bool,
    /// The value Nix App Center's file sets.
    pub own: Option<Value>,
    /// Other files setting it at a priority that would conflict.
    pub elsewhere: Vec<String>,
    /// The priority of the definitions that win: 100 for a plain one.
    #[serde(default = "default_priority")]
    pub priority: i64,
    /// The files whose value Nix App Center's file replaces.
    #[serde(default)]
    pub replaces: Vec<String>,
}

fn default_priority() -> i64 {
    1500
}

impl Setting {
    /// True when the page can change it: a plain definition in another
    /// file is replaced, a forced one is left alone.
    pub fn editable(&self) -> bool {
        self.kind != SettingKind::Other
            && (self.elsewhere.is_empty() || self.priority >= 100)
            && plain_name(&self.name)
    }

    /// The files whose value a change here replaces.
    pub fn overridden(&self) -> &[String] {
        if self.elsewhere.is_empty() {
            &self.replaces
        } else {
            &self.elsewhere
        }
    }

    /// The description, without the "Whether to enable" of enable options.
    pub fn help(&self) -> String {
        let text = self.description.as_deref().unwrap_or_default().trim();
        match text.strip_prefix("Whether to enable ") {
            Some(rest) => {
                let mut chars = rest.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }
            None => text.to_string(),
        }
    }
}

/// True for a dotted name of plain identifiers, safe to write as is.
fn plain_name(name: &str) -> bool {
    name.split('.').all(|part| {
        part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-'".contains(c))
    })
}

/// The settings file of a NixOS flake layout.
pub(crate) fn settings_file(layout: &Layout) -> Option<PathBuf> {
    match layout.kind {
        LayoutKind::NixosFlake { .. } => layout
            .system
            .as_ref()
            .map(|s| s.nix_app_center_file.with_file_name(SETTINGS_FILE)),
        _ => None,
    }
}

/// The `my.*` settings of the configuration. Blocking, several seconds.
///
/// # Errors
/// Nix's message, as printed.
pub fn load_settings() -> Result<Vec<Setting>, String> {
    let layout = detect();
    let LayoutKind::NixosFlake { dir, host } = &layout.kind else {
        return Err(
            crate::util::n_("System settings need a NixOS configuration with a flake.").into(),
        );
    };
    let out = crate::util::nix()
        .args(["eval", "--json"])
        .arg(format!("{}#nixosConfigurations.\"{host}\"", dir.display()))
        .args(["--apply", EVAL])
        .args(local_overrides(dir))
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(nix_error(&out.stderr));
    }
    let mut settings: Vec<Setting> =
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let file = settings_file(&layout)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default();
    let replaced = replaced_files(&file);
    for s in &mut settings {
        s.replaces = replaced.get(&s.name).cloned().unwrap_or_default();
    }
    Ok(settings)
}

/// The settings `file` forces, with the files they replace, as
/// [`render`] wrote them.
fn replaced_files(file: &str) -> BTreeMap<String, Vec<String>> {
    file.lines()
        .filter_map(|l| {
            let (name, rest) = l.trim().split_once(" = lib.mkForce ")?;
            let (_, files) = rest.rsplit_once("; # replaces ")?;
            Some((
                name.to_string(),
                files.split(", ").map(String::from).collect(),
            ))
        })
        .collect()
}

/// Nix's last error, whole: it spans several lines.
fn nix_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let start = text.rfind("error:").unwrap_or(0);
    text[start..].trim().to_string()
}

/// The generated file setting `values`; those in `replaces` are forced
/// over the files listed.
fn render(values: &BTreeMap<String, Value>, replaces: &BTreeMap<String, Vec<String>>) -> String {
    let mut out =
        String::from("# Generated by Nix App Center, do not edit by hand.\n{ lib, ... }:\n{\n");
    for (name, value) in values {
        let Some(v) = nix_value(value).filter(|_| plain_name(name)) else {
            continue;
        };
        match replaces.get(name).filter(|f| !f.is_empty()) {
            Some(files) => out.push_str(&format!(
                "  {name} = lib.mkForce {v}; # replaces {}\n",
                files.join(", ")
            )),
            None => out.push_str(&format!("  {name} = {v};\n")),
        }
    }
    out.push_str("}\n");
    out
}

/// `v` as Nix; `None` for objects.
fn nix_value(v: &Value) -> Option<String> {
    match v {
        Value::Null => Some("null".into()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace("${", "\\${")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t")
        )),
        Value::Array(items) => {
            let items = items.iter().map(nix_value).collect::<Option<Vec<_>>>()?;
            Some(format!("[ {} ]", items.join(" ")).replace("[  ]", "[ ]"))
        }
        _ => None,
    }
}

/// Writes the settings file with `values`, the settings changed in Nix App
/// Center, forcing those in `replaces` over the files listed. The system
/// file imports it. Asks for rights once when needed.
pub fn write_settings(
    values: &BTreeMap<String, Value>,
    replaces: &BTreeMap<String, Vec<String>>,
) -> std::io::Result<()> {
    let layout = detect();
    let (Some(path), Some(system)) = (settings_file(&layout), layout.system.as_ref()) else {
        return Err(std::io::Error::other(crate::util::n_(
            "System settings need a NixOS configuration with a flake.",
        )));
    };
    let kept = crate::manifest::load_manifest().kept;
    let mut files = vec![
        (path, render(values, replaces).into_bytes()),
        (
            system.nix_app_center_file.clone(),
            crate::config_gen::render_system(&kept, true).into_bytes(),
        ),
    ];
    files.retain(|(p, body)| std::fs::read(p).ok().as_ref() != Some(body));
    if files.is_empty() {
        return Ok(());
    }
    crate::paths::write_files(&files)?;
    for (p, _) in &files {
        crate::paths::track_in_git(p);
    }
    crate::manifest::mark_pending()
}

/// Steps updating the inputs of the configuration, without applying.
pub fn update_inputs_steps() -> Option<Vec<Vec<String>>> {
    let inputs = crate::updates::flake_inputs(&crate::updates::current_source());
    let mut steps = crate::updates::inputs_update_command(&inputs, &detect())?;
    // The last step applies.
    steps.pop();
    Some(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_only_the_given_values() {
        let values = BTreeMap::from([
            ("my.desktop".to_string(), json!("hyprland")),
            ("my.gaming.enable".to_string(), json!(true)),
            ("my.monitor.scale".to_string(), json!(1.5)),
            ("my.ai.models".to_string(), json!(["a \"b\"", "${x}"])),
            ("my.zerotier.networks".to_string(), json!([])),
            ("my.bad name".to_string(), json!(1)),
        ]);
        assert_eq!(
            render(&values, &BTreeMap::new()),
            "# Generated by Nix App Center, do not edit by hand.\n{ lib, ... }:\n{\n  my.ai.models = [ \"a \\\"b\\\"\" \"\\${x}\" ];\n  my.desktop = \"hyprland\";\n  my.gaming.enable = true;\n  my.monitor.scale = 1.5;\n  my.zerotier.networks = [ ];\n}\n"
        );
    }

    #[test]
    fn reads_the_eval_output() {
        let out = json!([
            { "name": "my.desktop", "description": "Desktop session.", "kind": "enum",
              "value": "niri", "default": "\"niri\"", "values": ["gnome", "niri"],
              "own": null, "elsewhere": [] },
            { "name": "my.rgb.enable", "description": "Whether to enable RGB control.",
              "kind": "bool", "value": true, "default": "false", "values": [],
              "own": null, "elsewhere": ["custom.nix"], "priority": 50 },
            { "name": "my.monitor.above", "description": null, "kind": "other",
              "value": "null", "default": null, "values": [], "own": null, "elsewhere": [] }
        ]);
        let settings: Vec<Setting> = serde_json::from_value(out).unwrap();
        assert!(settings[0].editable());
        assert_eq!(settings[0].values, ["gnome", "niri"]);
        assert!(!settings[1].editable(), "custom.nix forces it");
        assert_eq!(settings[1].help(), "RGB control.");
        assert!(!settings[2].editable());
        assert_eq!(settings[2].help(), "");
    }

    #[test]
    fn a_value_from_another_file_is_forced_and_read_back() {
        let values = BTreeMap::from([
            ("my.desktop".to_string(), json!("gnome; # replaces x")),
            ("my.gaming.enable".to_string(), json!(true)),
        ]);
        let replaces = BTreeMap::from([(
            "my.desktop".to_string(),
            vec!["custom.nix".to_string(), "hosts.nix".to_string()],
        )]);
        let file = render(&values, &replaces);
        assert!(
            file.contains("  my.desktop = lib.mkForce \"gnome; # replaces x\"; # replaces custom.nix, hosts.nix\n"),
            "{file}"
        );
        assert!(file.contains("  my.gaming.enable = true;\n"), "{file}");
        assert_eq!(
            replaced_files(&file),
            BTreeMap::from([(
                "my.desktop".to_string(),
                vec!["custom.nix".to_string(), "hosts.nix".to_string()]
            )])
        );
        let plain: Setting = serde_json::from_value(json!({
            "name": "my.desktop", "description": null, "kind": "enum", "value": "niri",
            "default": null, "values": ["gnome", "niri"], "own": null,
            "elsewhere": ["custom.nix"], "priority": 100
        }))
        .unwrap();
        assert!(plain.editable(), "a plain definition is replaced");
        assert_eq!(plain.overridden(), ["custom.nix"]);
    }

    #[test]
    fn keeps_the_whole_nix_error() {
        let stderr = b"warning: dirty\nerror:\n       ... while x\n\n       error: The option `my.ai.models' does not exist.\n       Did you mean `my.ai.enable'?\n";
        assert_eq!(
            nix_error(stderr),
            "error: The option `my.ai.models' does not exist.\n       Did you mean `my.ai.enable'?"
        );
    }

    #[test]
    fn an_empty_nullable_field_writes_null() {
        let values = BTreeMap::from([
            ("my.share.device".to_string(), Value::Null),
            ("my.monitor.above.height".to_string(), json!(1440)),
        ]);
        let file = render(&values, &BTreeMap::new());
        assert!(file.contains("  my.share.device = null;\n"), "{file}");
        assert!(
            file.contains("  my.monitor.above.height = 1440;\n"),
            "{file}"
        );
    }

    #[test]
    fn only_plain_names_are_written() {
        assert!(plain_name("my.gaming.enable"));
        assert!(!plain_name("my.\"a b\".x"));
        assert!(!plain_name("my.1x"));
        assert!(!plain_name("my..x"));
    }
}
