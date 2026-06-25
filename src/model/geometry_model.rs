use serde::{Deserialize, Serialize};

use super::overlay_model::RgbaColor;
use super::{
    default_geometry_arrow_head_size, default_geometry_fill_color,
    default_geometry_fill_color_expr, default_geometry_font_size, default_geometry_opacity,
    default_geometry_point_radius, default_geometry_stroke_color,
    default_geometry_stroke_color_expr, default_geometry_thickness, default_true,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum GeometryShapeKind {
    #[default]
    Point,
    Line,
    Circle,
    Rectangle,
    Label,
    Ellipse,
    Arrow,
    Polyline,
    Polygon,
    Arc,
    Svg,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeometrySpec {
    pub shape: GeometryShapeKind,
    pub x1_expr: String,
    pub y1_expr: String,
    pub x2_expr: String,
    pub y2_expr: String,
    pub x3_expr: String,
    pub y3_expr: String,
    pub x4_expr: String,
    pub y4_expr: String,
    pub width_expr: String,
    pub height_expr: String,
    pub radius_expr: String,
    pub radius_x_expr: String,
    pub radius_y_expr: String,
    pub start_angle_expr: String,
    pub end_angle_expr: String,
    pub rotation_expr: String,
    pub arrow_head_size_expr: String,
    pub font_size_expr: String,
    pub thickness_expr: String,
    pub opacity_expr: String,
    pub fill_opacity_expr: String,
    pub points_expr: String,
    pub text: String,
    pub stroke_color_expr: String,
    pub fill_color_expr: String,
    #[serde(default = "default_geometry_stroke_color")]
    pub stroke_color: RgbaColor,
    #[serde(default = "default_geometry_fill_color")]
    pub fill_color: RgbaColor,
    pub filled: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_geometry_thickness")]
    pub thickness: f32,
    #[serde(default = "default_geometry_opacity")]
    pub opacity: f32,
    #[serde(default = "default_geometry_opacity")]
    pub fill_opacity: f32,
    #[serde(default = "default_geometry_font_size")]
    pub font_size: f32,
    #[serde(default = "default_geometry_point_radius")]
    pub point_radius: f32,
    #[serde(default = "default_geometry_arrow_head_size")]
    pub arrow_head_size: f32,
}

impl Default for GeometrySpec {
    fn default() -> Self {
        Self {
            shape: GeometryShapeKind::Point,
            x1_expr: "960".to_owned(),
            y1_expr: "540".to_owned(),
            x2_expr: "1120".to_owned(),
            y2_expr: "540".to_owned(),
            x3_expr: String::new(),
            y3_expr: String::new(),
            x4_expr: String::new(),
            y4_expr: String::new(),
            width_expr: "0".to_owned(),
            height_expr: "0".to_owned(),
            radius_expr: "60".to_owned(),
            radius_x_expr: "90".to_owned(),
            radius_y_expr: "60".to_owned(),
            start_angle_expr: "0".to_owned(),
            end_angle_expr: "180".to_owned(),
            rotation_expr: "0".to_owned(),
            arrow_head_size_expr: "16".to_owned(),
            font_size_expr: "18".to_owned(),
            thickness_expr: "2".to_owned(),
            opacity_expr: "1".to_owned(),
            fill_opacity_expr: "0.3".to_owned(),
            points_expr: "960,540;1120,540;1120,660".to_owned(),
            text: "Label".to_owned(),
            stroke_color_expr: default_geometry_stroke_color_expr(),
            fill_color_expr: default_geometry_fill_color_expr(),
            stroke_color: default_geometry_stroke_color(),
            fill_color: default_geometry_fill_color(),
            filled: false,
            visible: true,
            thickness: default_geometry_thickness(),
            opacity: default_geometry_opacity(),
            fill_opacity: 0.3,
            font_size: default_geometry_font_size(),
            point_radius: default_geometry_point_radius(),
            arrow_head_size: default_geometry_arrow_head_size(),
        }
    }
}

impl GeometrySpec {
    pub fn apply_shape_defaults(&mut self) {
        if self.shape == GeometryShapeKind::Svg {
            if self.text == "Label" {
                self.text.clear();
            }
            if self.opacity_expr == "1" {
                self.opacity_expr = "100".to_owned();
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeometryObject {
    pub id: u32,
    pub spec: GeometrySpec,
}

impl GeometryObject {
    pub fn new(id: u32, shape: GeometryShapeKind) -> Self {
        let mut spec = GeometrySpec::default();
        spec.shape = shape;
        spec.apply_shape_defaults();
        Self { id, spec }
    }
}

impl Default for GeometryObject {
    fn default() -> Self {
        Self::new(1, GeometryShapeKind::Point)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeometryPreset {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub collapsed: bool,
    pub objects: Vec<GeometryObject>,
}

impl GeometryPreset {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: format!("Geometry {id}"),
            enabled: true,
            collapsed: true,
            objects: vec![GeometryObject::new(id, GeometryShapeKind::Point)],
        }
    }

    pub fn object_mut(&mut self) -> Option<&mut GeometryObject> {
        self.objects.first_mut()
    }
}

impl Default for GeometryPreset {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum SetVariableSource {
    #[default]
    Expression,
    TimeHour,
    TimeMinute,
    TimeSecond,
    TimeMillisecond,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum HideGeometryMode {
    #[default]
    Newest,
    Oldest,
    AllShown,
}
