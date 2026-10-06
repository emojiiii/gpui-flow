//! Optional canvas appearance stored in `data.flow_presentation.canvas`.
//!
//! Missing settings retain the legacy appearance. An existing canvas object
//! must use the supported schema version; invalid metadata is reported without
//! changing it. Hosts choose whether a failed read should display defaults.

use gpui_flow_core::{Graph, GraphError, ValidationIssue};
use serde_json::{Map, Value};

pub const DEFAULT_CANVAS_BACKGROUND_RGB: u32 = 0xf5f6f8;
pub const CANVAS_PREFERENCES_VERSION: u64 = 1;

const PRESENTATION_KEY: &str = "flow_presentation";
const CANVAS_KEY: &str = "canvas";
const CANVAS_PATH: &str = "data.flow_presentation.canvas";
const BACKGROUND_PATH: &str = "data.flow_presentation.canvas.background_rgb";

/// Effective preferences after resolving omitted fields to legacy defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasPreferences {
    pub background_rgb: u32,
    pub show_minimap: bool,
}

impl Default for CanvasPreferences {
    fn default() -> Self {
        Self {
            background_rgb: DEFAULT_CANVAS_BACKGROUND_RGB,
            show_minimap: false,
        }
    }
}

impl CanvasPreferences {
    /// Validate an in-memory preference before applying or persisting it.
    pub fn validate(&self) -> Result<(), GraphError> {
        if self.background_rgb > 0xffffff {
            return Err(metadata_error(
                BACKGROUND_PATH,
                "expected a 24-bit RGB integer between 0 and 0xffffff",
            ));
        }
        Ok(())
    }
}

/// Resolve optional settings without modifying the graph.
///
/// `data: null`, an absent presentation namespace, or an absent canvas object
/// use defaults. In a version 1 canvas object, omitted preference fields also
/// use defaults. Present fields must have their exact JSON types: colors and
/// versions are unsigned integers, and Mini Map visibility is a boolean.
/// Unknown fields are ignored and retained by [`write_canvas_preferences`].
pub fn read_canvas_preferences(graph: &Graph) -> Result<CanvasPreferences, GraphError> {
    let Some(canvas) = canvas_metadata(&graph.data)? else {
        return Ok(CanvasPreferences::default());
    };
    let version = canvas.get("version").and_then(Value::as_u64);
    if version != Some(CANVAS_PREFERENCES_VERSION) {
        return Err(metadata_error(
            "data.flow_presentation.canvas.version",
            "expected supported integer schema version 1",
        ));
    }
    let background_rgb = match canvas.get("background_rgb") {
        None => DEFAULT_CANVAS_BACKGROUND_RGB,
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= 0xffffff)
            .ok_or_else(|| {
                metadata_error(
                    BACKGROUND_PATH,
                    "expected a 24-bit RGB integer between 0 and 0xffffff",
                )
            })? as u32,
    };
    let show_minimap = match canvas.get("show_minimap") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| {
            metadata_error(
                "data.flow_presentation.canvas.show_minimap",
                "expected a boolean",
            )
        })?,
    };
    Ok(CanvasPreferences {
        background_rgb,
        show_minimap,
    })
}

/// Atomically persist effective preferences, returning whether data changed.
///
/// Validates both existing metadata and the replacement before changing any
/// graph data. Unsupported versions and malformed values cannot be overwritten
/// by this helper. Node order, hidden state, and all unknown fields at every
/// level are preserved. If effective preferences already match, even legacy
/// or partially specified metadata stays byte-for-byte unchanged as a value.
/// The caller owns the editor transaction and undo history.
pub fn write_canvas_preferences(
    graph: &mut Graph,
    preferences: CanvasPreferences,
) -> Result<bool, GraphError> {
    preferences.validate()?;
    if read_canvas_preferences(graph)? == preferences {
        return Ok(false);
    }
    let mut data = match &graph.data {
        Value::Null => Map::new(),
        Value::Object(data) => data.clone(),
        _ => unreachable!("existing metadata was validated before constructing the write"),
    };
    let mut presentation = data
        .get(PRESENTATION_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut canvas = presentation
        .get(CANVAS_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    canvas.insert("version".into(), CANVAS_PREFERENCES_VERSION.into());
    canvas.insert("background_rgb".into(), preferences.background_rgb.into());
    canvas.insert("show_minimap".into(), preferences.show_minimap.into());
    presentation.insert(CANVAS_KEY.into(), Value::Object(canvas));
    data.insert(PRESENTATION_KEY.into(), Value::Object(presentation));
    graph.data = Value::Object(data);
    Ok(true)
}

fn canvas_metadata(data: &Value) -> Result<Option<&Map<String, Value>>, GraphError> {
    let data = match data {
        Value::Null => return Ok(None),
        Value::Object(data) => data,
        _ => return Err(metadata_error("data", "expected an object or null")),
    };
    let Some(presentation) = data.get(PRESENTATION_KEY) else {
        return Ok(None);
    };
    let presentation = presentation
        .as_object()
        .ok_or_else(|| metadata_error("data.flow_presentation", "expected an object"))?;
    let Some(canvas) = presentation.get(CANVAS_KEY) else {
        return Ok(None);
    };
    canvas
        .as_object()
        .map(Some)
        .ok_or_else(|| metadata_error(CANVAS_PATH, "expected an object"))
}

fn metadata_error(path: &str, message: &str) -> GraphError {
    GraphError::Validation(vec![ValidationIssue {
        path: path.into(),
        message: message.into(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_defaults_and_no_ops_leave_metadata_unchanged() {
        for data in [
            Value::Null,
            json!({}),
            json!({"host": true}),
            json!({"flow_presentation": {"node_order": ["b", "a"], "hidden": ["a"]}}),
            json!({"flow_presentation": {"canvas": {"version": 1}}}),
        ] {
            let mut graph = Graph::new();
            graph.data = data;
            let before = graph.clone();
            assert_eq!(
                read_canvas_preferences(&graph).unwrap(),
                CanvasPreferences::default()
            );
            assert!(!write_canvas_preferences(&mut graph, CanvasPreferences::default()).unwrap());
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn preferences_round_trip_with_graph_and_reset_to_legacy_appearance() {
        let mut graph = Graph::new();
        let preferences = CanvasPreferences {
            background_rgb: 0x123abc,
            show_minimap: true,
        };
        assert!(write_canvas_preferences(&mut graph, preferences).unwrap());
        assert_eq!(
            graph.data,
            json!({"flow_presentation": {"canvas": {
                "version": 1, "background_rgb": 0x123abc, "show_minimap": true
            }}})
        );
        let mut restored = Graph::from_json(&graph.to_json().unwrap()).unwrap();
        assert_eq!(read_canvas_preferences(&restored).unwrap(), preferences);
        assert!(!write_canvas_preferences(&mut restored, preferences).unwrap());
        assert!(write_canvas_preferences(&mut restored, CanvasPreferences::default()).unwrap());
        assert_eq!(
            read_canvas_preferences(&restored).unwrap(),
            CanvasPreferences::default()
        );
    }

    #[test]
    fn edits_preserve_host_metadata_and_other_presentation_settings() {
        let mut graph = Graph::new();
        graph.data = json!({
            "background_rgb": "host-owned", "show_minimap": "host-owned",
            "application": {"nested": [1, true, "keep"]},
            "flow_presentation": {
                "node_order": ["z", "a"], "hidden": ["z"], "extension": null,
                "canvas": {"version": 1, "show_minimap": true, "extension": {"keep": 42}}
            }
        });
        let mut expected = graph.data.clone();
        expected["flow_presentation"]["canvas"]["background_rgb"] = json!(0);
        expected["flow_presentation"]["canvas"]["show_minimap"] = json!(false);
        assert!(
            write_canvas_preferences(
                &mut graph,
                CanvasPreferences {
                    background_rgb: 0,
                    show_minimap: false,
                }
            )
            .unwrap()
        );
        assert_eq!(graph.data, expected);
    }

    #[test]
    fn malformed_existing_metadata_rejects_reads_and_writes_without_mutation() {
        for data in [
            json!("host payload"),
            json!(42),
            json!(false),
            json!([]),
            json!({"flow_presentation": null}),
            json!({"flow_presentation": []}),
            json!({"flow_presentation": "host payload"}),
            json!({"flow_presentation": {"canvas": null}}),
            json!({"flow_presentation": {"canvas": []}}),
            json!({"flow_presentation": {"canvas": false}}),
        ] {
            assert_rejected_without_mutation(data);
        }
        for color in [
            json!(-1),
            json!(0x1000000),
            json!(u64::MAX),
            json!(1.0),
            json!("#ffffff"),
            json!(true),
            Value::Null,
        ] {
            assert_rejected_without_mutation(json!({"flow_presentation": {"canvas": {
                "version": 1, "background_rgb": color
            }}}));
        }
        for visibility in [json!(0), json!(1), json!("true"), Value::Null] {
            assert_rejected_without_mutation(json!({"flow_presentation": {"canvas": {
                "version": 1, "show_minimap": visibility
            }}}));
        }
    }

    #[test]
    fn schema_version_is_required_and_never_silently_downgraded() {
        assert_rejected_without_mutation(json!({"flow_presentation": {"canvas": {}}}));
        for version in [
            Value::Null,
            json!(0),
            json!(2),
            json!(-1),
            json!(1.0),
            json!("1"),
            json!(true),
        ] {
            assert_rejected_without_mutation(json!({"flow_presentation": {"canvas": {
                "version": version, "future_data": {"keep": true}
            }}}));
        }
        let mut graph = Graph::new();
        graph.data = json!({"flow_presentation": {"canvas": {
            "version": 1, "background_rgb": 0xffffff
        }}});
        assert_eq!(
            read_canvas_preferences(&graph).unwrap(),
            CanvasPreferences {
                background_rgb: 0xffffff,
                show_minimap: false,
            }
        );
    }

    #[test]
    fn invalid_in_memory_color_cannot_replace_valid_graph_data() {
        let mut graph = Graph::new();
        write_canvas_preferences(
            &mut graph,
            CanvasPreferences {
                background_rgb: 0,
                show_minimap: true,
            },
        )
        .unwrap();
        let before = graph.clone();
        for color in [0x1000000, u32::MAX] {
            assert!(matches!(
                write_canvas_preferences(
                    &mut graph,
                    CanvasPreferences {
                        background_rgb: color,
                        show_minimap: false,
                    }
                ),
                Err(GraphError::Validation(_))
            ));
            assert_eq!(graph, before);
        }
    }

    fn assert_rejected_without_mutation(data: Value) {
        let mut graph = Graph::new();
        graph.data = data;
        let before = graph.clone();
        assert!(matches!(
            read_canvas_preferences(&graph),
            Err(GraphError::Validation(_))
        ));
        // Even a write equal to fallback defaults must validate existing data.
        assert!(matches!(
            write_canvas_preferences(&mut graph, CanvasPreferences::default()),
            Err(GraphError::Validation(_))
        ));
        assert_eq!(graph, before);
    }
}
