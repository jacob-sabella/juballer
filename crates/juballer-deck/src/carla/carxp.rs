//! Parse Carla project files (`*.carxp`) into the bare metadata we
//! need for name → index resolution.
//!
//! Phase 2.1 reads the project file at carla-mode startup so cell
//! bindings can use friendlier `plugin = "ZynReverb"` and
//! `param = "Wet"` strings instead of brittle numeric slot indices.
//!
//! ## Format we care about
//!
//! ```xml
//! <CARLA-PROJECT VERSION='2.x'>
//!   <Plugin>
//!     <Info>
//!       <Type>LV2</Type>
//!       <Name>GxTuner</Name>
//!       <URI>...</URI>
//!     </Info>
//!     <Data>
//!       <Parameter>
//!         <Index>0</Index>
//!         <Name>FREQ</Name>
//!         <Symbol>FREQ</Symbol>
//!         <Value>0</Value>
//!       </Parameter>
//!       …
//!     </Data>
//!   </Plugin>
//!   …
//! </CARLA-PROJECT>
//! ```
//!
//! Plugins occur in document order — that order is also their slot
//! index in Carla's runtime. Parameters carry their own `<Index>` so
//! we don't have to count them ourselves; that field is what shows up
//! over OSC.

use crate::Result;
use std::path::Path;

/// One slot of a parsed Carla project.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPlugin {
    /// Slot index Carla assigns at load time (= position in the
    /// project file's `<Plugin>` order).
    pub slot: u32,
    /// `<Info><Name>...</Name></Info>` — the user-facing name. Free
    /// text; uniqueness inside a single project is by convention.
    pub name: String,
    /// `<Info><Type>...</Type></Info>` — `LV2` / `INTERNAL` / `VST2`
    /// / `SFZ` / etc.
    pub plugin_type: Option<String>,
    /// `<Info><URI>...</URI></Info>` for LV2 plugins.
    pub uri: Option<String>,
    /// `<Info><Label>...</Label></Info>` for INTERNAL plugins.
    pub label: Option<String>,
    /// Parameters declared in `<Data>`. Order matches the project file
    /// (which is itself ordered by `<Index>`).
    pub params: Vec<ProjectParam>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectParam {
    /// Numeric index Carla uses over OSC.
    pub index: u32,
    /// `<Name>...</Name>` — user-facing parameter name.
    pub name: String,
    /// `<Symbol>...</Symbol>` — short identifier used by LV2; absent
    /// on INTERNAL / VST2 plugins.
    pub symbol: Option<String>,
}

/// Top-level parsed project. Just the slots we need; the rest of the
/// XML (engine settings, MIDI routing, custom-data blobs) is ignored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CarlaProject {
    pub plugins: Vec<ProjectPlugin>,
}

impl CarlaProject {
    /// Read + parse a `*.carxp` file from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let body = std::fs::read_to_string(path)?;
        Self::parse(&body)
            .map_err(|e| crate::Error::Config(format!("carla project {}: {e}", path.display())))
    }

    /// Parse from an in-memory XML string. Pulled out so the tests can
    /// drive small literal fixtures without touching the filesystem.
    pub fn parse(body: &str) -> std::result::Result<Self, ParseError> {
        let opts = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let doc = roxmltree::Document::parse_with_options(body, opts)
            .map_err(|e| ParseError::Xml(e.to_string()))?;
        let root = doc.root_element();
        if root.tag_name().name() != "CARLA-PROJECT" {
            return Err(ParseError::WrongRoot(root.tag_name().name().to_string()));
        }
        let mut plugins = Vec::new();
        for (slot, node) in root
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Plugin")
            .enumerate()
        {
            plugins.push(parse_plugin(slot as u32, node)?);
        }
        Ok(Self { plugins })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("XML: {0}")]
    Xml(String),
    #[error("expected <CARLA-PROJECT> root, got <{0}>")]
    WrongRoot(String),
    #[error("plugin slot {slot}: missing <Info>/<Name>")]
    MissingPluginName { slot: u32 },
    #[error("plugin slot {slot} parameter: <Index> missing or unparseable")]
    BadParamIndex { slot: u32 },
    #[error("plugin slot {slot} parameter at index {index}: missing <Name>")]
    MissingParamName { slot: u32, index: u32 },
}

fn parse_plugin(
    slot: u32,
    plugin: roxmltree::Node<'_, '_>,
) -> std::result::Result<ProjectPlugin, ParseError> {
    let info = plugin
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Info");
    let name = info
        .and_then(|i| child_text(i, "Name"))
        .ok_or(ParseError::MissingPluginName { slot })?;
    let plugin_type = info.and_then(|i| child_text(i, "Type"));
    let uri = info.and_then(|i| child_text(i, "URI"));
    let label = info.and_then(|i| child_text(i, "Label"));

    let mut params = Vec::new();
    if let Some(data) = plugin
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Data")
    {
        for param_node in data
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Parameter")
        {
            params.push(parse_param(slot, param_node)?);
        }
    }

    Ok(ProjectPlugin {
        slot,
        name,
        plugin_type,
        uri,
        label,
        params,
    })
}

fn parse_param(
    slot: u32,
    param: roxmltree::Node<'_, '_>,
) -> std::result::Result<ProjectParam, ParseError> {
    let index = child_text(param, "Index")
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or(ParseError::BadParamIndex { slot })?;
    let name = child_text(param, "Name").ok_or(ParseError::MissingParamName { slot, index })?;
    let symbol = child_text(param, "Symbol");
    Ok(ProjectParam {
        index,
        name,
        symbol,
    })
}

fn child_text(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
        .and_then(|n| n.text())
        .map(str::trim)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<!DOCTYPE CARLA-PROJECT>
<CARLA-PROJECT VERSION='2.6'>
  <Plugin>
    <Info>
      <Type>INTERNAL</Type>
      <Name>ZynReverb</Name>
      <Label>zynreverb</Label>
    </Info>
    <Data>
      <Parameter>
        <Index>0</Index>
        <Name>Time</Name>
        <Value>93</Value>
      </Parameter>
      <Parameter>
        <Index>5</Index>
        <Name>Low-Pass Filter</Name>
        <Value>114</Value>
      </Parameter>
    </Data>
  </Plugin>
  <Plugin>
    <Info>
      <Type>LV2</Type>
      <Name>GxTuner</Name>
      <URI>http://guitarix.sourceforge.net/plugins/gxtuner#tuner</URI>
    </Info>
    <Data>
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
  </Plugin>
</CARLA-PROJECT>"#;

    #[test]
    fn parse_extracts_plugins_in_document_order_with_slot_indices() {
        let proj = CarlaProject::parse(SAMPLE).unwrap();
        assert_eq!(proj.plugins.len(), 2);
        assert_eq!(proj.plugins[0].slot, 0);
        assert_eq!(proj.plugins[0].name, "ZynReverb");
        assert_eq!(proj.plugins[1].slot, 1);
        assert_eq!(proj.plugins[1].name, "GxTuner");
    }

    #[test]
    fn parse_records_plugin_type_and_uri_when_present() {
        let proj = CarlaProject::parse(SAMPLE).unwrap();
        assert_eq!(proj.plugins[0].plugin_type.as_deref(), Some("INTERNAL"));
        assert_eq!(proj.plugins[0].label.as_deref(), Some("zynreverb"));
        assert_eq!(proj.plugins[0].uri, None);
        assert_eq!(proj.plugins[1].plugin_type.as_deref(), Some("LV2"));
        assert_eq!(
            proj.plugins[1].uri.as_deref(),
            Some("http://guitarix.sourceforge.net/plugins/gxtuner#tuner")
        );
    }

    #[test]
    fn parse_walks_parameters_with_index_and_optional_symbol() {
        let proj = CarlaProject::parse(SAMPLE).unwrap();
        let zr = &proj.plugins[0];
        assert_eq!(zr.params.len(), 2);
        assert_eq!(zr.params[0].index, 0);
        assert_eq!(zr.params[0].name, "Time");
        assert!(
            zr.params[0].symbol.is_none(),
            "INTERNAL plugin has no Symbol"
        );
        let gx = &proj.plugins[1];
        assert_eq!(gx.params[0].name, "FREQ");
        assert_eq!(gx.params[0].symbol.as_deref(), Some("FREQ"));
    }

    #[test]
    fn parse_handles_a_plugin_with_no_data_block() {
        let body = r#"<?xml version='1.0'?>
            <CARLA-PROJECT VERSION='2.6'>
              <Plugin>
                <Info><Type>SFZ</Type><Name>Drums</Name></Info>
              </Plugin>
            </CARLA-PROJECT>"#;
        let proj = CarlaProject::parse(body).unwrap();
        assert_eq!(proj.plugins.len(), 1);
        assert!(proj.plugins[0].params.is_empty());
    }

    #[test]
    fn parse_rejects_wrong_root_element() {
        let body = "<NotACarlaProject><Plugin/></NotACarlaProject>";
        let err = CarlaProject::parse(body).unwrap_err();
        match err {
            ParseError::WrongRoot(name) => assert_eq!(name, "NotACarlaProject"),
            other => panic!("wrong error variant: {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_plugin_with_missing_name() {
        let body = r#"<?xml version='1.0'?>
            <CARLA-PROJECT VERSION='2.6'>
              <Plugin>
                <Info><Type>SFZ</Type></Info>
              </Plugin>
            </CARLA-PROJECT>"#;
        let err = CarlaProject::parse(body).unwrap_err();
        assert!(matches!(err, ParseError::MissingPluginName { slot: 0 }));
    }

    #[test]
    fn parse_rejects_param_with_unparseable_index() {
        let body = r#"<?xml version='1.0'?>
            <CARLA-PROJECT VERSION='2.6'>
              <Plugin>
                <Info><Type>LV2</Type><Name>X</Name></Info>
                <Data>
                  <Parameter>
                    <Index>notanint</Index>
                    <Name>Bad</Name>
                  </Parameter>
                </Data>
              </Plugin>
            </CARLA-PROJECT>"#;
        let err = CarlaProject::parse(body).unwrap_err();
        assert!(matches!(err, ParseError::BadParamIndex { slot: 0 }));
    }

    #[test]
    fn parse_handles_xml_with_doctype_and_attributes() {
        // The sample we have in /home/jsabella/thall.carxp uses a DOCTYPE
        // and `VERSION` attributes. roxmltree should handle these.
        let proj = CarlaProject::parse(SAMPLE).unwrap();
        assert_eq!(proj.plugins.len(), 2);
    }
}
