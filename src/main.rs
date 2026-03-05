use glass_core::event::AppEvent;
use glass_core::config::GlassConfig;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = GlassConfig::default();
    tracing::info!("Glass starting with font: {} size: {}", config.font_family, config.font_size);
    tracing::info!("AppEvent type registered: {:?}", std::any::type_name::<AppEvent>());

    // Phase 1 Plan 02 replaces this with winit event loop
    tracing::info!("Scaffold complete — no event loop yet");
}
