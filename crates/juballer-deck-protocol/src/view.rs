use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewNode {
    Vstack {
        #[serde(default = "default_gap")]
        gap: f32,
        #[serde(default)]
        align: Align,
        children: Vec<ViewNode>,
    },
    Hstack {
        #[serde(default = "default_gap")]
        gap: f32,
        #[serde(default)]
        align: Align,
        children: Vec<ViewNode>,
    },
    Text {
        value: String,
        #[serde(default)]
        size: Option<f32>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        weight: Option<String>,
    },
    Icon {
        #[serde(flatten)]
        src: IconSrc,
        #[serde(default)]
        size: Option<f32>,
    },
    Bar {
        value: f32,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        label: Option<String>,
    },
    Spacer {
        #[serde(default = "default_gap")]
        size: f32,
    },
    Divider,
    Image {
        #[serde(flatten)]
        src: ImageSrc,
        #[serde(default)]
        width: Option<f32>,
        #[serde(default)]
        height: Option<f32>,
        #[serde(default)]
        fit: Option<ImageFit>,
    },
    Button {
        label: String,
        action: String,
        #[serde(default)]
        args: Option<serde_json::Value>,
        #[serde(default)]
        color: Option<String>,
    },
    Plot {
        values: Vec<f32>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        height: Option<f32>,
        #[serde(default)]
        label: Option<String>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default)]
        header_color: Option<String>,
    },
    Scroll {
        child: Box<ViewNode>,
        #[serde(default)]
        height: Option<f32>,
    },
    Padding {
        child: Box<ViewNode>,
        #[serde(default)]
        all: Option<f32>,
        #[serde(default)]
        top: Option<f32>,
        #[serde(default)]
        right: Option<f32>,
        #[serde(default)]
        bottom: Option<f32>,
        #[serde(default)]
        left: Option<f32>,
    },
    Bg {
        child: Box<ViewNode>,
        color: String,
        #[serde(default)]
        rounding: Option<f32>,
    },
    Progress {
        value: f32,
        #[serde(default)]
        max: Option<f32>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        show_percent: Option<bool>,
    },
    Kpi {
        value: String,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        delta: Option<String>,
        #[serde(default)]
        delta_positive: Option<bool>,
        #[serde(default)]
        color: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IconSrc {
    Emoji { emoji: String },
    Path { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ImageSrc {
    Url { url: String },
    DataUrl { data_url: String },
    Path { path: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    #[default]
    Contain,
    Cover,
    Fill,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

fn default_gap() -> f32 {
    4.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(node: &ViewNode) -> ViewNode {
        let s = serde_json::to_string(node).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn vstack_roundtrip() {
        let n = ViewNode::Vstack {
            gap: 4.0,
            align: Align::Start,
            children: vec![ViewNode::Divider],
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn hstack_roundtrip_with_align() {
        let n = ViewNode::Hstack {
            gap: 6.0,
            align: Align::Center,
            children: vec![ViewNode::Spacer { size: 8.0 }],
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn text_roundtrip_partial() {
        let n = ViewNode::Text {
            value: "hi".into(),
            size: Some(18.0),
            color: Some("#cdd6f4".into()),
            weight: Some("bold".into()),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn text_roundtrip_minimal() {
        let json = r#"{"kind":"text","value":"hi"}"#;
        let n: ViewNode = serde_json::from_str(json).unwrap();
        match n {
            ViewNode::Text {
                value,
                size,
                color,
                weight,
            } => {
                assert_eq!(value, "hi");
                assert!(size.is_none());
                assert!(color.is_none());
                assert!(weight.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn icon_emoji_roundtrip() {
        let n = ViewNode::Icon {
            src: IconSrc::Emoji {
                emoji: "🎤".into()
            },
            size: Some(24.0),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"emoji\":\"🎤\""));
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn icon_path_roundtrip() {
        let n = ViewNode::Icon {
            src: IconSrc::Path {
                path: "/tmp/x.png".into(),
            },
            size: None,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"path\":\"/tmp/x.png\""));
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn bar_roundtrip() {
        let n = ViewNode::Bar {
            value: 0.42,
            color: Some("#a6e3a1".into()),
            label: Some("speaking".into()),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn spacer_default_size() {
        let json = r#"{"kind":"spacer"}"#;
        let n: ViewNode = serde_json::from_str(json).unwrap();
        match n {
            ViewNode::Spacer { size } => assert!((size - 4.0).abs() < f32::EPSILON),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn divider_roundtrip() {
        let n = ViewNode::Divider;
        let s = serde_json::to_string(&n).unwrap();
        assert_eq!(s, r#"{"kind":"divider"}"#);
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn align_default_start() {
        let json = r#"{"kind":"vstack","children":[]}"#;
        let n: ViewNode = serde_json::from_str(json).unwrap();
        match n {
            ViewNode::Vstack { align, gap, .. } => {
                assert_eq!(align, Align::Start);
                assert!((gap - 4.0).abs() < f32::EPSILON);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn nested_tree_roundtrip() {
        let n = ViewNode::Vstack {
            gap: 4.0,
            align: Align::Start,
            children: vec![
                ViewNode::Text {
                    value: "🔊 General".into(),
                    size: Some(18.0),
                    color: Some("#cdd6f4".into()),
                    weight: Some("bold".into()),
                },
                ViewNode::Hstack {
                    gap: 6.0,
                    align: Align::End,
                    children: vec![
                        ViewNode::Icon {
                            src: IconSrc::Emoji {
                                emoji: "🎤".into()
                            },
                            size: Some(24.0),
                        },
                        ViewNode::Bar {
                            value: 0.42,
                            color: None,
                            label: None,
                        },
                    ],
                },
                ViewNode::Divider,
                ViewNode::Spacer { size: 8.0 },
            ],
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn image_url_roundtrip() {
        let n = ViewNode::Image {
            src: ImageSrc::Url {
                url: "https://example.com/x.png".into(),
            },
            width: Some(64.0),
            height: Some(64.0),
            fit: Some(ImageFit::Cover),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"url\":\"https://example.com/x.png\""));
        assert!(s.contains("\"fit\":\"cover\""));
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn image_data_url_roundtrip() {
        let n = ViewNode::Image {
            src: ImageSrc::DataUrl {
                data_url: "data:image/png;base64,AAAA".into(),
            },
            width: None,
            height: None,
            fit: None,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"data_url\":"));
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn image_path_roundtrip() {
        let n = ViewNode::Image {
            src: ImageSrc::Path {
                path: "/tmp/x.png".into(),
            },
            width: Some(32.0),
            height: None,
            fit: Some(ImageFit::Contain),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn image_fit_default_is_contain() {
        let f = ImageFit::default();
        assert_eq!(f, ImageFit::Contain);
    }

    #[test]
    fn button_roundtrip() {
        let n = ViewNode::Button {
            label: "Mute".into(),
            action: "discord.mute".into(),
            args: Some(serde_json::json!({"toggle": true})),
            color: Some("#a6e3a1".into()),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn button_minimal() {
        let json = r#"{"kind":"button","label":"Go","action":"deck.page_goto"}"#;
        let n: ViewNode = serde_json::from_str(json).unwrap();
        match n {
            ViewNode::Button {
                label,
                action,
                args,
                color,
            } => {
                assert_eq!(label, "Go");
                assert_eq!(action, "deck.page_goto");
                assert!(args.is_none());
                assert!(color.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn plot_roundtrip() {
        let n = ViewNode::Plot {
            values: vec![1.0, 2.0, 1.5, 3.0],
            color: Some("#89b4fa".into()),
            height: Some(40.0),
            label: Some("cpu".into()),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn table_roundtrip() {
        let n = ViewNode::Table {
            headers: vec!["name".into(), "status".into()],
            rows: vec![
                vec!["foo".into(), "ok".into()],
                vec!["bar".into(), "down".into()],
            ],
            header_color: Some("#89b4fa".into()),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn scroll_roundtrip() {
        let n = ViewNode::Scroll {
            child: Box::new(ViewNode::Text {
                value: "hi".into(),
                size: None,
                color: None,
                weight: None,
            }),
            height: Some(200.0),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn padding_roundtrip() {
        let n = ViewNode::Padding {
            child: Box::new(ViewNode::Divider),
            all: None,
            top: Some(4.0),
            right: Some(8.0),
            bottom: Some(4.0),
            left: Some(8.0),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn padding_all_shorthand() {
        let json = r#"{"kind":"padding","child":{"kind":"divider"},"all":8}"#;
        let n: ViewNode = serde_json::from_str(json).unwrap();
        match n {
            ViewNode::Padding { all, .. } => assert_eq!(all, Some(8.0)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn bg_roundtrip() {
        let n = ViewNode::Bg {
            child: Box::new(ViewNode::Divider),
            color: "#1e1e2e".into(),
            rounding: Some(6.0),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn progress_roundtrip() {
        let n = ViewNode::Progress {
            value: 42.0,
            max: Some(100.0),
            color: Some("green".into()),
            label: Some("CPU".into()),
            show_percent: Some(true),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn progress_minimal() {
        let json = r#"{"kind":"progress","value":0.5}"#;
        let n: ViewNode = serde_json::from_str(json).unwrap();
        match n {
            ViewNode::Progress { value, max, .. } => {
                assert!((value - 0.5).abs() < f32::EPSILON);
                assert!(max.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn kpi_roundtrip() {
        let n = ViewNode::Kpi {
            value: "1,234".into(),
            label: Some("users".into()),
            delta: Some("+42".into()),
            delta_positive: Some(true),
            color: Some("mauve".into()),
        };
        assert_eq!(rt(&n), n);
    }

    #[test]
    fn kpi_minimal() {
        let json = r#"{"kind":"kpi","value":"99"}"#;
        let n: ViewNode = serde_json::from_str(json).unwrap();
        match n {
            ViewNode::Kpi {
                value,
                label,
                delta,
                ..
            } => {
                assert_eq!(value, "99");
                assert!(label.is_none());
                assert!(delta.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }
}
