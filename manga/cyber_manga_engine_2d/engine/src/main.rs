mod director;
mod manga;
mod schedulers;
mod script_ai; // Round 8: LLM-based script parser
mod sd;
mod translator;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use candle_core::Device;
use director::Director;
use manga::Panel; // Keep Panel for response compatibility
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
    video: Option<String>,
}

#[derive(Clone)]
struct AppState {
    sd: Arc<StableDiffusion>,
    director: Arc<Director>,
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

    let director = match Director::new(sd.clone(), true) {
        // true = AI parser (casual text), false = markdown parser
        Ok(d) => Arc::new(d),
        Err(e) => {
            tracing::error!("Failed to initialize Director: {}", e);
            return;
        }
    };

    let state = AppState { sd, director };

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
    match state.sd.generate(&payload.prompt, 30, 7.5, None) {
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

    match state.director.produce(&payload.script).await {
        Ok((storyboard, video_path)) => {
            // Read video bytes
            let video_bytes = match std::fs::read(&video_path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("Failed to read generated video: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to read generated video: {}", e),
                    )
                        .into_response();
                }
            };

            let b64_vid = general_purpose::STANDARD.encode(&video_bytes);
            let video_uri = format!("data:video/mp4;base64,{}", b64_vid);

            // Convert Storyboard Shots to Panels for frontend compatibility
            let panels: Vec<Panel> = storyboard
                .shots
                .iter()
                .map(|s| Panel {
                    role: s.character.clone(),
                    dialogue: s.dialogue.clone(),
                    prompt: s.visual_prompt.clone(),
                })
                .collect();

            // For the main image, we can just return the first frame or a placeholder since it's a video now
            // But frontend expects an image. Let's try to load the first frame edited image.
            let first_image_uri = if let Some(first_shot) = storyboard.shots.first() {
                if first_shot.video_path.is_some() {
                    // Try to find the edited image corresponding to this shot
                    // It was saved as output/video/edited_{id}.png in Editor
                    // BUT we don't have the path here easily unless we reconstructed it or stored it.
                    // Shot doesn't store edited image path, only raw image path?
                    // Wait, Editor stored `video_path`.
                    // Let's use `image_path` (raw) for now from the Shot, or just an empty image.
                    // The user wants video.
                    // Use the raw image path.
                    if let Some(img_path) = first_shot.image_paths.first() {
                        if let Ok(img_bytes) = std::fs::read(img_path) {
                            let b64 = general_purpose::STANDARD.encode(&img_bytes);
                            format!("data:image/png;base64,{}", b64)
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new() // No shots
            };

            let response = MangaResponse {
                image: first_image_uri,
                panels,
                video: Some(video_uri),
            };

            tracing::info!("Manga generated with metadata (Video available)");
            (StatusCode::OK, Json(response)).into_response()
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
