#[derive(Debug, Clone)]
pub struct GlassConfig {
    pub font_family: String,
    pub font_size: f32,
    pub shell: Option<String>,
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self {
            font_family: "Consolas".into(),
            font_size: 14.0,
            shell: None,
        }
    }
}
