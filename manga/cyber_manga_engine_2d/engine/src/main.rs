mod director;
mod manga;
mod models;
mod schedulers;
mod script_ai;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use candle_core::Device;
use director::Director;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Serialize)]
struct MangaResponse {
    image: String,
    panels: Vec<Panel>,
    video: Option<String>,
}

#[derive(Serialize)]
struct Panel {
    id: usize,
    image: String,
    prompt: String,
}

#[derive(Clone)]
struct AppState {
    device: Device,
    director: Arc<RwLock<Director>>,
}

#[derive(Deserialize)]
struct GenerateMangaRequest {
    script: String,
    style: Option<String>, // e.g., "ghibli", "manga", "anime"
}

#[tokio::main]
async fn main() {
    // Auto-configure HF mirror for China network
    if std::env::var("HF_ENDPOINT").is_err() {
        std::env::set_var("HF_ENDPOINT", "https://hf-mirror.com");
    }

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 1. Select Device (Metal/CUDA/CPU)
    // FORCE CPU for stability (User reported brown images/NaNs on Metal)
    let device = {
        tracing::info!("Forcing CPU backend for stable generation...");
        Device::Cpu
    };

    // 2. Initialize Director (Load Models)
    tracing::info!("Initializing Director...");
    let director = match Director::new(&device, true) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to initialize Director: {}", e);
            return;
        }
    };

    // 3. Initialize VideoClient (Legacy removed, using internal SD15)
    tracing::info!("Starting Engine with internal Ghibli-Diffusion...");

    let state = AppState {
        device: device.clone(),
        director: Arc::new(RwLock::new(director)),
    };

    tracing::info!("Engine initialized successfully!");

    let app = Router::new()
        .route("/", get(handler))
        .route("/api/generate_manga", post(generate_manga))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("CyberManga Engine 后端已在 {} 启动", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler() -> &'static str {
    "CyberManga Engine is running!"
}

async fn generate_manga(
    State(state): State<AppState>,
    Json(req): Json<GenerateMangaRequest>,
) -> impl IntoResponse {
    tracing::info!(
        "Received Manga request: {} (Style: {:?})",
        req.script,
        req.style
    );

    // Generate Manga
    // Use read lock for generation to allow concurrent reads if scalable,
    // but sd15 mutex inside director will lock anyway.
    let (storyboard, final_path) = {
        let director = state.director.read().await;
        // Pass req.style to produce
        match director.produce(&req.script, req.style.clone()).await {
            Ok(res) => res,
            Err(e) => {
                tracing::error!("Production failed: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Generation failed").into_response();
            }
        }
    };

    // Read final image bytes to base64 for updated frontend
    let image_bytes = match std::fs::read(&final_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read generated image: {}", e),
            )
                .into_response();
        }
    };

    let b64_img = general_purpose::STANDARD.encode(&image_bytes);
    let image_uri = format!("data:image/png;base64,{}", b64_img);

    Json(MangaResponse {
        image: image_uri,
        panels: storyboard
            .shots
            .into_iter()
            .map(|s| Panel {
                id: s.id,
                image: s
                    .image_paths
                    .first()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                prompt: s.visual_prompt,
            })
            .collect(),
        video: None, // No video generated
    })
    .into_response()
}
