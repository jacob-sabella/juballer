//! Parse Carla single-plugin preset files (`*.carxs`).
//!
//! Carla saves two kinds of preset state in this format:
//!
//! 1. Native plugin types (LV2 / INTERNAL / SFZ / SF2): serialised as
//!    a list of `<Parameter><Index>...<Value>...</Parameter>` blocks
//!    inside `<Data>`. We translate these into juballer
//!    [`PresetParam`] entries directly.
//! 2. Opaque plugin types (VST2 / VST3 / proprietary): serialised as
//!    one base64 `<Chunk>` blob inside `<Data>`. juballer keeps the
//!    chunk verbatim and sends it back to Carla via
//!    `/Carla/<id>/set_chunk` when the preset is applied.
//!
//! Either form is enough to round-trip the user's saved sound through
//! the deck's preset library.

use crate::carla::config::{ParamRef, Preset, PresetParam};
use crate::Result;
use std::path::Path;

/// Parsed `*.carxs` file. The only consumer today is the conversion
/// to a juballer [`Preset`] via [`From`]; the struct is exposed so
/// callers (and tests) can inspect the discriminating fields when
/// needed.
#[derive(Debug, Clone, PartialEq)]
pub struct CarlaXsPreset {
    pub plugin_type: Option<String>,
    pub plugin_name: String,
    /// LV2 URI (`<Info><URI>...</URI></Info>`).
    pub uri: Option<String>,
    /// VST2 / VST3 binary path.
    pub binary: Option<String>,
    /// VST2 unique id (decimal string).
    pub unique_id: Option<String>,
    /// INTERNAL plugin label.
    pub label: Option<String>,
    /// `<Data><Parameter>...</Parameter></Data>` entries when present.
    pub params: Vec<CarxsParam>,
    /// Base64 chunk for VST2/VST3 state. Whitespace is stripped on
    /// parse so the `String` is a single contiguous base64 payload
    /// ready to feed into `set_chunk`.
    pub chunk: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CarxsParam {
    pub index: u32,
    pub name: String,
    pub symbol: Option<String>,
    pub value: f32,
}

impl CarlaXsPreset {
    /// Read + parse a `*.carxs` file from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let body = std::fs::read_to_string(path)?;
        Self::parse(&body)
            .map_err(|e| crate::Error::Config(format!("carla preset {}: {e}", path.display())))
    }

    /// Parse from an in-memory XML string.
    pub fn parse(body: &str) -> std::result::Result<Self, ParseError> {
        let opts = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let doc = roxmltree::Document::parse_with_options(body, opts)
            .map_err(|e| ParseError::Xml(e.to_string()))?;
        let root = doc.root_element();
        if root.tag_name().name() != "CARLA-PRESET" {
            return Err(ParseError::WrongRoot(root.tag_name().name().to_string()));
        }
        let info = root
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "Info");
        let plugin_name = info
            .and_then(|i| child_text(i, "Name"))
            .ok_or(ParseError::MissingPluginName)?;
        let plugin_type = info.and_then(|i| child_text(i, "Type"));
        let uri = info.and_then(|i| child_text(i, "URI"));
        let binary = info.and_then(|i| child_text(i, "Binary"));
        let unique_id = info.and_then(|i| child_text(i, "UniqueID"));
        let label = info.and_then(|i| child_text(i, "Label"));

        let mut params = Vec::new();
        let mut chunk = None;
        if let Some(data) = root
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "Data")
        {
            for node in data.children().filter(|n| n.is_element()) {
                match node.tag_name().name() {
                    "Parameter" => params.push(parse_param(node)?),
                    "Chunk" => {
                        if let Some(text) = node.text() {
                            chunk = Some(strip_whitespace(text));
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(CarlaXsPreset {
            plugin_type,
            plugin_name,
            uri,
            binary,
            unique_id,
            label,
            params,
            chunk,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("XML: {0}")]
    Xml(String),
    #[error("expected <CARLA-PRESET> root, got <{0}>")]
    WrongRoot(String),
    #[error("missing <Info>/<Name>")]
    MissingPluginName,
    #[error("parameter: missing or unparseable <Index>")]
    BadParamIndex,
    #[error("parameter index {0}: missing <Name>")]
    MissingParamName(u32),
    #[error("parameter {0:?}: missing or unparseable <Value>")]
    BadParamValue(String),
}

fn parse_param(node: roxmltree::Node<'_, '_>) -> std::result::Result<CarxsParam, ParseError> {
    let index = child_text(node, "Index")
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or(ParseError::BadParamIndex)?;
    let name = child_text(node, "Name").ok_or(ParseError::MissingParamName(index))?;
    let symbol = child_text(node, "Symbol");
    let value = child_text(node, "Value")
        .and_then(|s| s.parse::<f32>().ok())
        .ok_or_else(|| ParseError::BadParamValue(name.clone()))?;
    Ok(CarxsParam {
        index,
        name,
        symbol,
        value,
    })
}

fn child_text(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
        .and_then(|n| n.text())
        .map(str::trim)
        .map(str::to_owned)
}

fn strip_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

impl From<CarlaXsPreset> for Preset {
    /// Convert a parsed `*.carxs` into a juballer [`Preset`]. The
    /// preset's `target_plugin` field carries the carxs's
    /// `<Info><Name>` so the operator can match it against a Carla
    /// slot via the name map. Parameters become friendly named refs;
    /// chunks ride along verbatim.
    ///
    /// `name` is intentionally `None` so the preset library's
    /// [`super::preset::PresetEntry::name`] falls back to the file
    /// stem — every Neural-DSP `.carxs` shares the same `plugin_name`
    /// (e.g. "thall amp" across the whole Factory bank) so deriving
    /// the entry name from `<Info><Name>` would collide every preset
    /// onto a single slot. The filename is the meaningful identifier.
    fn from(x: CarlaXsPreset) -> Self {
        let params = x
            .params
            .into_iter()
            .map(|p| PresetParam {
                name: ParamRef::Name(p.name),
                value: p.value,
            })
            .collect();
        Preset {
            name: None,
            description: None,
            target_plugin: x.plugin_name,
            params,
            files: Vec::new(),
            chunk: x.chunk,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VST2_CHUNK_SAMPLE: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<!DOCTYPE CARLA-PRESET>
<CARLA-PRESET VERSION="2.0">
  <Info>
   <Type>VST2</Type>
   <Name>Archetype Gojira X</Name>
   <Binary>/home/x/.wine/drive_c/Program Files/VstPlugins/Archetype Gojira X.dll</Binary>
   <UniqueID>1313294936</UniqueID>
  </Info>
  <Data>
   <Active>Yes</Active>
   <ControlChannel>1</ControlChannel>
   <Options>0x3f9</Options>
   <Chunk>
    VkMyIagRAAA8YXBwTW9kZWwgc2VsZWN0ZWRTZWN0aW9uPSIyIiBzZWxlY3RlZENhYj0iMCI=
    </Chunk>
  </Data>
</CARLA-PRESET>"#;

    const LV2_PARAM_SAMPLE: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<!DOCTYPE CARLA-PRESET>
<CARLA-PRESET VERSION="2.0">
  <Info>
   <Type>LV2</Type>
   <Name>GxTuner</Name>
   <URI>http://guitarix.sourceforge.net/plugins/gxtuner#tuner</URI>
  </Info>
  <Data>
   <Active>Yes</Active>
   <ControlChannel>1</ControlChannel>
   <Options>0x1</Options>
   <Parameter>
    <Index>0</Index>
    <Name>FREQ</Name>
    <Symbol>FREQ</Symbol>
    <Value>0</Value>
   </Parameter>
   <Parameter>
    <Index>5</Index>
    <Name>THRESHOLD</Name>
    <Symbol>THRESHOLD</Symbol>
    <Value>-50</Value>
   </Parameter>
  </Data>
</CARLA-PRESET>"#;

    #[test]
    fn parse_extracts_vst2_info_and_strips_chunk_whitespace() {
        let p = CarlaXsPreset::parse(VST2_CHUNK_SAMPLE).unwrap();
        assert_eq!(p.plugin_name, "Archetype Gojira X");
        assert_eq!(p.plugin_type.as_deref(), Some("VST2"));
        assert_eq!(p.unique_id.as_deref(), Some("1313294936"));
        assert!(p.binary.unwrap().contains("Gojira"));
        let chunk = p.chunk.expect("VST2 sample carries a chunk");
        assert!(!chunk.contains(char::is_whitespace), "stripped");
        assert!(chunk.starts_with("VkMyIag"));
        assert!(p.params.is_empty(), "chunk-based preset has no params");
    }

    #[test]
    fn parse_extracts_lv2_parameters_with_index_name_value() {
        let p = CarlaXsPreset::parse(LV2_PARAM_SAMPLE).unwrap();
        assert_eq!(p.plugin_name, "GxTuner");
        assert_eq!(p.plugin_type.as_deref(), Some("LV2"));
        assert!(p.uri.unwrap().contains("gxtuner"));
        assert!(p.chunk.is_none());
        assert_eq!(p.params.len(), 2);
        assert_eq!(p.params[0].index, 0);
        assert_eq!(p.params[0].name, "FREQ");
        assert!((p.params[0].value - 0.0).abs() < 1e-6);
        assert!((p.params[1].value - -50.0).abs() < 1e-6);
    }

    #[test]
    fn parse_rejects_wrong_root_element() {
        let body = "<NotAPreset/>";
        let err = CarlaXsPreset::parse(body).unwrap_err();
        assert!(matches!(err, ParseError::WrongRoot(s) if s == "NotAPreset"));
    }

    #[test]
    fn parse_rejects_preset_with_missing_name() {
        let body = "<CARLA-PRESET><Info><Type>LV2</Type></Info></CARLA-PRESET>";
        assert!(matches!(
            CarlaXsPreset::parse(body).unwrap_err(),
            ParseError::MissingPluginName
        ));
    }

    #[test]
    fn carxs_into_preset_carries_chunk_for_vst2_round_trip() {
        let xs = CarlaXsPreset::parse(VST2_CHUNK_SAMPLE).unwrap();
        let preset: Preset = xs.into();
        assert_eq!(preset.target_plugin, "Archetype Gojira X");
        assert!(preset.params.is_empty());
        assert!(preset.chunk.is_some(), "chunk should ride along");
    }

    #[test]
    fn carxs_into_preset_translates_lv2_params_to_named_refs() {
        let xs = CarlaXsPreset::parse(LV2_PARAM_SAMPLE).unwrap();
        let preset: Preset = xs.into();
        assert_eq!(preset.params.len(), 2);
        match &preset.params[0].name {
            ParamRef::Name(n) => assert_eq!(n, "FREQ"),
            _ => panic!("expected named ref for friendly carxs param"),
        }
        assert!(preset.chunk.is_none());
    }

    /// Smoke-test against one of the user's actual presets if it
    /// exists. Skipped silently in CI / on developer machines without
    /// the file — this is opt-in by virtue of being a fixed path.
    #[test]
    fn parse_real_world_gojira_preset_when_present() {
        let path =
            std::path::Path::new("/home/jsabella/ndsp-presets/gojira/Gojira/Clean Dec.carxs");
        if !path.exists() {
            return;
        }
        let p = CarlaXsPreset::load(path).expect("real world preset should parse");
        assert!(p.plugin_name.to_lowercase().contains("gojira"));
        assert!(
            p.chunk.is_some(),
            "Archetype Gojira X is a VST2 chunk preset"
        );
    }
}
