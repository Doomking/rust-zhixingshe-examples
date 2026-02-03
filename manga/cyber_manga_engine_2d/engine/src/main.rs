mod manga;
mod schedulers;
mod sd;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use candle_core::Device;
use manga::{MangaGenerator, Panel};
use sd::StableDiffusion;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;

#[derive(Serialize)]
struct ImageResponse {
    image: String,
}

#[derive(Serialize)]
struct MangaResponse {
    image: String,
    panels: Vec<Panel>,
}

#[derive(Clone)]
struct AppState {
    sd: Arc<StableDiffusion>,
    manga: Arc<MangaGenerator>,
}

#[derive(Deserialize)]
struct GenerateImageRequest {
    prompt: String,
}

#[derive(Deserialize)]
struct GenerateMangaRequest {
    script: String,
}

#[tokio::main]
async fn main() {
    // 自动配置 Hugging Face 镜像 (适合国内网络环境)
    if std::env::var("HF_ENDPOINT").is_err() {
        std::env::set_var("HF_ENDPOINT", "https://hf-mirror.com");
    }

    // 1. 初始化日志系统 (体现专业感)
    tracing_subscriber::fmt::init();

    tracing::info!("Initializing Stable Diffusion models...");

    // 实例化设备
    let device = if candle_core::utils::metal_is_available() {
        tracing::info!("Using Metal (GPU) for inference");
        Device::new_metal(0).unwrap_or(Device::Cpu)
    } else if candle_core::utils::cuda_is_available() {
        tracing::info!("Using CUDA (GPU) for inference");
        Device::new_cuda(0).unwrap_or(Device::Cpu)
    } else {
        tracing::info!("Using CPU for inference");
        Device::Cpu
    };

    // 加载模型 (只需加载一次)
    let sd = match StableDiffusion::new(&device) {
        Ok(sd) => Arc::new(sd),
        Err(e) => {
            tracing::error!("Failed to load models: {}", e);
            return;
        }
    };

    let manga = match MangaGenerator::new(sd.clone()) {
        Ok(m) => Arc::new(m),
        Err(e) => {
            tracing::error!("Failed to initialize MangaGenerator (Check fonts?): {}", e);
            return;
        }
    };

    let state = AppState { sd, manga };

    tracing::info!("Models loaded successfully!");

    // 2. 构建路由
    let app = Router::new()
        .route("/", get(handler))
        .route("/api/generate", post(generate_image))
        .route("/api/generate_manga", post(generate_manga))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // 3. 启动服务器
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("CyberManga Engine 后端已在 {} 启动", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler() -> impl IntoResponse {
    "CyberManga-Engine engine is Running!"
}

async fn generate_image(
    State(state): State<AppState>,
    Json(payload): Json<GenerateImageRequest>,
) -> impl IntoResponse {
    tracing::info!("Received image prompt: {}", payload.prompt);

    // 使用共享的 SD 实例生成图片
    match state.sd.generate(&payload.prompt, 30, 7.5) {
        Ok(image) => {
            // 将 DynamicImage 转换为 PNG 字节流
            let mut bytes: Vec<u8> = Vec::new();
            match image.write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            ) {
                Ok(_) => {
                    let b64 = general_purpose::STANDARD.encode(&bytes);
                    let data_uri = format!("data:image/png;base64,{}", b64);

                    let response = ImageResponse { image: data_uri };

                    tracing::info!("Image generated successfully (JSON)");
                    (StatusCode::OK, Json(response)).into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to encode image: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to encode image").into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Generation failed: {}", e),
            )
                .into_response()
        }
    }
}

async fn generate_manga(
    State(state): State<AppState>,
    Json(payload): Json<GenerateMangaRequest>,
) -> impl IntoResponse {
    tracing::info!("Received manga script (length: {})", payload.script.len());

    match state.manga.generate_manga(&payload.script).await {
        Ok((image, panels)) => {
            let mut bytes: Vec<u8> = Vec::new();
            match image.write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            ) {
                Ok(_) => {
                    let b64 = general_purpose::STANDARD.encode(&bytes);
                    let data_uri = format!("data:image/png;base64,{}", b64);

                    let response = MangaResponse {
                        image: data_uri,
                        panels,
                    };

                    tracing::info!("Manga generated with metadata");
                    (StatusCode::OK, Json(response)).into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to encode manga image: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to encode image").into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Manga Generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Manga Generation failed: {}", e),
            )
                .into_response()
        }
    }
}
