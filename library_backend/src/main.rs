use axum::{
    Json, Router,
    extract::{Query, State},
    http::{Method, StatusCode},
    routing::get,
};
use clap::Parser;
use hyphenation::{Hyphenator, Load};
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, path::PathBuf, str::FromStr, sync::Arc};
use textwrap::WordSeparator;
use tokio::{fs, signal};
use tower_http::{
    self,
    cors::{Any, CorsLayer},
};

struct AppState {
    texts_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct TextDefinition {
    title: String,
    text: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexListResponse {
    file_name: String,
    title: String,
}

#[derive(Deserialize)]
struct WordReaderParams {
    story: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WordReaderResponse {
    paragraph_list: Vec<Paragraph>,
}
#[derive(Debug, Serialize, Deserialize)]
struct Paragraph(Vec<Word>);
#[derive(Debug, Serialize, Deserialize)]
struct Word(Vec<Syllable>);
#[derive(Debug, Serialize, Deserialize)]
struct Syllable(String);

/// App to serve texts for reading practice
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the folder containing the json file with texts and metadata
    #[clap(short, long, default_value = "./texts")]
    texts_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let texts_path = PathBuf::from(&args.texts_path);

    // pass env to handlers via state
    let app_state = Arc::new(AppState { texts_path });

    let cors_layer = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET]);

    let app = Router::new()
        .route("/", get(list_all))
        .route("/word_reader", get(word_reader))
        .layer(cors_layer)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8888").await?;
    println!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn list_all(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<IndexListResponse>>, StatusCode> {
    println!("Search for texts in {:?}", state.texts_path);
    let mut text_index_list = Vec::new();
    let mut text_files = tokio::fs::read_dir(&state.texts_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Ok(Some(file)) = text_files.next_entry().await {
        let file_path = file.path();
        if file_path.extension().and_then(OsStr::to_str) == Some("json") {
            let file_contents = fs::read_to_string(file_path).await.unwrap();
            if let Ok(t) = serde_json::from_str::<TextDefinition>(file_contents.as_str()) {
                if let Some(file_name) = file.path().file_stem().unwrap().to_str() {
                    println!("Text tile: {}", t.title);

                    text_index_list.push(IndexListResponse {
                        file_name: file_name.to_string(),
                        title: t.title,
                    });
                }
            }
        }
    }

    Ok(Json(text_index_list))
}

async fn word_reader(
    Query(params): Query<WordReaderParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<WordReaderResponse>, StatusCode> {
    println!("Going to read story {}", params.story);
    let story_file = PathBuf::from_str(&format!("{}.json", params.story))
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let file_path = state.texts_path.join(story_file);
    println!("File path {file_path:?}");
    let file_contents = fs::read_to_string(file_path)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let text_definition = serde_json::from_str::<TextDefinition>(file_contents.as_str())
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let dictionary = hyphenation::Standard::from_embedded(hyphenation::Language::Dutch).unwrap();

    let mut paragraph_list = Vec::new();
    for p in &text_definition.text {
        let mut paragraph = Paragraph(Vec::new());
        for separated_word in WordSeparator::AsciiSpace.find_words(p) {
            let mut word = Word(Vec::new());
            let hyphenated = dictionary.hyphenate(&separated_word);
            for syllable in hyphenated.into_iter() {
                let syllable_clean = syllable.trim_end_matches("-");
                word.0.push(Syllable(syllable_clean.to_owned()));
            }
            paragraph.0.push(word);
        }
        paragraph_list.push(paragraph);
    }

    Ok(Json(WordReaderResponse { paragraph_list }))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
