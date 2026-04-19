use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A controller+monitor specific calibration. Persisted as TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub profile: ProfileMeta,
    pub grid: GridGeometry,
    pub top: TopGeometry,
    #[serde(default)]
    pub keymap: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub controller_id: String,
    pub monitor_id: String,
}

/// Grid geometry: placement of the 4x4 cell grid on the monitor.
///
/// **Schema (current)**: Single-cell reference. `origin_px` is the top-left pixel of
/// the top-left cell. `cell_size_px` is the pixel size of a single cell (same for all
/// 16 cells). `gap_x_px` / `gap_y_px` are the pixels between adjacent cell rects.
/// Other cells are derived as:
/// `(x,y) = origin_px + (col * (cell_w + gap_x), row * (cell_h + gap_y))`.
///
/// **Back-compat**: TOML with the older whole-grid schema (`size_px` describing the
/// outer rect of all 16 cells + gaps, plus a single `gap_px`) is auto-migrated on
/// load to the new single-cell schema. Written profiles always use the new schema.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GridGeometry {
    /// Top-left pixel of the TL cell.
    pub origin_px: PointPx,
    /// Pixel size of a single cell (all 16 cells are this size).
    pub cell_size_px: SizePx,
    /// Horizontal gap between adjacent cells (in pixels).
    pub gap_x_px: u16,
    /// Vertical gap between adjacent cells (in pixels).
    pub gap_y_px: u16,
    /// Visual border inset rendered inside each cell.
    pub border_px: u16,
    #[serde(default)]
    pub rotation_deg: f32,
}

/// Raw deserialized view of GridGeometry. Accepts both the new schema and the
/// legacy `size_px` (whole-grid outer rect) + `gap_px` fields; post-processed into
/// the canonical `GridGeometry` on load. Unknown combinations fall back to
/// sensible defaults rather than erroring.
#[derive(Debug, Deserialize)]
struct GridGeometryRaw {
    #[serde(default)]
    origin_px: Option<PointPx>,
    /// New schema: per-cell size.
    #[serde(default)]
    cell_size_px: Option<SizePx>,
    /// Legacy schema: whole-grid outer size.
    #[serde(default)]
    size_px: Option<SizePx>,
    #[serde(default)]
    gap_x_px: Option<u16>,
    #[serde(default)]
    gap_y_px: Option<u16>,
    /// Legacy combined gap; copied into both axes when the per-axis fields are absent.
    #[serde(default)]
    gap_px: Option<u16>,
    #[serde(default)]
    border_px: Option<u16>,
    #[serde(default)]
    rotation_deg: Option<f32>,
}

impl<'de> Deserialize<'de> for GridGeometry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = GridGeometryRaw::deserialize(deserializer)?;
        let origin_px = raw.origin_px.unwrap_or(PointPx { x: 0, y: 0 });
        let gap_px = raw.gap_px.unwrap_or(12);
        let gap_x_px = raw.gap_x_px.unwrap_or(gap_px);
        let gap_y_px = raw.gap_y_px.unwrap_or(gap_px);
        let cell_size_px = match (raw.cell_size_px, raw.size_px) {
            (Some(c), _) => c,
            // Legacy migration: whole-grid → per-cell.
            // cell = (outer - 3*gap) / 4, using the relevant per-axis gap.
            (None, Some(outer)) => SizePx {
                w: outer.w.saturating_sub(3 * gap_x_px as u32) / 4,
                h: outer.h.saturating_sub(3 * gap_y_px as u32) / 4,
            },
            (None, None) => SizePx { w: 240, h: 240 },
        };
        Ok(GridGeometry {
            origin_px,
            cell_size_px,
            gap_x_px,
            gap_y_px,
            border_px: raw.border_px.unwrap_or(4),
            rotation_deg: raw.rotation_deg.unwrap_or(0.0),
        })
    }
}

/// Top-region padding + cutoff: the pixel region above the grid that egui overlays
/// are allowed to render into. The GAMO2 FB9 controller sits on top of the
/// monitor, and its plastic frame / bezel covers some pixels above the button grid;
/// `cutoff_bottom` pulls the usable region up so widgets don't render under it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TopGeometry {
    /// Pixels between the top edge of the screen and the top of the top-region.
    pub edge_padding_top: u16,
    /// Pixels of left+right inset inside the top-region rect.
    pub edge_padding_x: u16,
    /// Pixels above the grid's origin_y where the top-region bottom is clipped.
    /// Accounts for the physical controller's bezel/frame above the buttons.
    pub cutoff_bottom: u16,
}

#[derive(Debug, Deserialize)]
struct TopGeometryRaw {
    #[serde(default)]
    edge_padding_top: Option<u16>,
    #[serde(default)]
    edge_padding_x: Option<u16>,
    #[serde(default)]
    cutoff_bottom: Option<u16>,
    /// Legacy field: old profiles had only a scalar gap between the top-region and
    /// the grid. Treated as `cutoff_bottom` when the new fields are missing.
    #[serde(default)]
    margin_above_grid_px: Option<u16>,
}

impl<'de> Deserialize<'de> for TopGeometry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = TopGeometryRaw::deserialize(deserializer)?;
        Ok(TopGeometry {
            edge_padding_top: raw.edge_padding_top.unwrap_or(24),
            edge_padding_x: raw.edge_padding_x.unwrap_or(16),
            cutoff_bottom: raw.cutoff_bottom.or(raw.margin_above_grid_px).unwrap_or(40),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointPx {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SizePx {
    pub w: u32,
    pub h: u32,
}

impl Profile {
    /// Build a default profile centered for a given monitor resolution.
    /// Grid = square fitting the lower 60% of the screen height.
    pub fn default_for(
        controller_id: impl Into<String>,
        monitor_id: impl Into<String>,
        monitor_w: u32,
        monitor_h: u32,
    ) -> Self {
        let grid_h = (monitor_h as f32 * 0.6) as u32;
        let grid_w = grid_h.min(monitor_w);
        let origin_x = ((monitor_w - grid_w) / 2) as i32;
        let origin_y = (monitor_h - grid_h) as i32;
        let gap = 12u16;
        let cell_w = grid_w.saturating_sub(3 * gap as u32) / 4;
        let cell_h = grid_h.saturating_sub(3 * gap as u32) / 4;
        Self {
            profile: ProfileMeta {
                controller_id: controller_id.into(),
                monitor_id: monitor_id.into(),
            },
            grid: GridGeometry {
                origin_px: PointPx {
                    x: origin_x,
                    y: origin_y,
                },
                cell_size_px: SizePx {
                    w: cell_w,
                    h: cell_h,
                },
                gap_x_px: gap,
                gap_y_px: gap,
                border_px: 4,
                rotation_deg: 0.0,
            },
            top: TopGeometry {
                edge_padding_top: 24,
                edge_padding_x: 16,
                cutoff_bottom: 40,
            },
            keymap: HashMap::new(),
        }
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let toml = self.to_toml().map_err(std::io::Error::other)?;
        // Atomic write: tempfile in the same dir + rename. Never leaves a partial
        // file behind on power loss or crash mid-write.
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let mut tmp = tempfile::Builder::new()
            .prefix(".profile.toml.")
            .suffix(".tmp")
            .tempfile_in(dir)?;
        use std::io::Write as _;
        tmp.write_all(toml.as_bytes())?;
        tmp.as_file_mut().sync_all()?;
        tmp.persist(path)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        let s = std::fs::read_to_string(path)?;
        Self::from_toml(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Returns true if every cell (0,0)..(3,3) has a keycode mapping.
    pub fn keymap_complete(&self) -> bool {
        for r in 0..4 {
            for c in 0..4 {
                if !self.keymap.contains_key(&format!("{},{}", r, c)) {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_for_is_centered_lower_60_percent() {
        let p = Profile::default_for("vid:pid/sn", "MON / 1920x1080", 1920, 1080);
        let grid_h = (1080.0 * 0.6) as u32;
        let grid_w = grid_h; // min(grid_h, 1920) == grid_h
        let expected_cell = grid_w.saturating_sub(3 * 12) / 4;
        assert_eq!(p.grid.cell_size_px.w, expected_cell);
        assert_eq!(p.grid.cell_size_px.h, expected_cell);
        let expected_x = ((1920 - grid_w) / 2) as i32;
        assert_eq!(p.grid.origin_px.x, expected_x);
        assert_eq!(p.grid.origin_px.y, 1080 - grid_h as i32);
        assert_eq!(p.grid.rotation_deg, 0.0);
        assert_eq!(p.grid.gap_x_px, 12);
        assert_eq!(p.grid.gap_y_px, 12);
        assert_eq!(p.grid.border_px, 4);
        assert_eq!(p.top.edge_padding_top, 24);
        assert_eq!(p.top.edge_padding_x, 16);
        assert_eq!(p.top.cutoff_bottom, 40);
        assert!(p.keymap.is_empty());
    }

    #[test]
    fn toml_roundtrip() {
        let mut p = Profile::default_for("a", "b", 1920, 1080);
        p.keymap.insert("0,0".into(), "KEY_W".into());
        let s = p.to_toml().unwrap();
        let back = Profile::from_toml(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/profile.toml");
        let p = Profile::default_for("a", "b", 2560, 1440);
        p.save(&path).unwrap();
        let back = Profile::load(&path).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn keymap_complete_requires_all_16() {
        let mut p = Profile::default_for("a", "b", 1920, 1080);
        assert!(!p.keymap_complete());
        for r in 0..4 {
            for c in 0..4 {
                p.keymap
                    .insert(format!("{},{}", r, c), format!("KEY_{}", r * 4 + c));
            }
        }
        assert!(p.keymap_complete());
    }

    /// Back-compat: legacy profile with whole-grid `size_px` + single `gap_px` +
    /// `margin_above_grid_px`. Reproduces the shape of the user's live profile.toml.
    #[test]
    fn legacy_schema_migrates_on_load() {
        let legacy = r#"
[profile]
controller_id = "1973:0011"
monitor_id = "unknown / 0x0"

[grid]
gap_px = 12
border_px = 4
rotation_deg = 0.0

[grid.origin_px]
x = 27
y = 795

[grid.size_px]
w = 997
h = 997

[top]
margin_above_grid_px = 8

[keymap]
"0,0" = "KEY_A"
"#;
        let p = Profile::from_toml(legacy).expect("legacy profile parses");
        assert_eq!(p.grid.origin_px.x, 27);
        assert_eq!(p.grid.origin_px.y, 795);
        // Legacy whole-grid 997 with gap 12 → per-cell (997 - 36) / 4 = 240.25 → 240.
        assert_eq!(p.grid.cell_size_px.w, 240);
        assert_eq!(p.grid.cell_size_px.h, 240);
        assert_eq!(p.grid.gap_x_px, 12);
        assert_eq!(p.grid.gap_y_px, 12);
        assert_eq!(p.grid.border_px, 4);
        // Legacy margin_above_grid_px bleeds into cutoff_bottom; other top fields default.
        assert_eq!(p.top.cutoff_bottom, 8);
        assert_eq!(p.top.edge_padding_top, 24);
        assert_eq!(p.top.edge_padding_x, 16);
        assert_eq!(p.keymap.get("0,0").unwrap(), "KEY_A");
    }

    #[test]
    fn new_schema_roundtrips() {
        let src = r#"
[profile]
controller_id = "a"
monitor_id = "b"

[grid]
gap_x_px = 10
gap_y_px = 14
border_px = 4
rotation_deg = 0.0

[grid.origin_px]
x = 27
y = 795

[grid.cell_size_px]
w = 240
h = 240

[top]
edge_padding_top = 24
edge_padding_x = 16
cutoff_bottom = 40

[keymap]
"#;
        let p = Profile::from_toml(src).expect("new profile parses");
        assert_eq!(p.grid.gap_x_px, 10);
        assert_eq!(p.grid.gap_y_px, 14);
        assert_eq!(p.grid.cell_size_px.w, 240);
    }
}
