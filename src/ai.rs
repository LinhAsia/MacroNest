use std::borrow::Cow;

use anyhow::{Context, Result, bail};
use arboard::{Clipboard, ImageData};
use image::DynamicImage;
use serde::Deserialize;

use crate::model::{AiResponseProvider, CommandPreset, GroqSettings};
use native_tls::TlsConnector;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use tungstenite::{client, Message};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommandPresetPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub collapsed: Option<bool>,
    #[serde(default)]
    pub target_window_title: Option<String>,
    #[serde(default)]
    pub extra_target_window_titles: Option<Vec<String>>,
    #[serde(default)]
    pub match_duplicate_window_titles: Option<bool>,
    #[serde(default)]
    pub use_powershell: Option<bool>,
}

impl CommandPresetPatch {
    pub fn apply_to(&self, preset: &mut CommandPreset) {
        if let Some(name) = self.name.as_ref() {
            let next = name.trim();
            if !next.is_empty() {
                preset.name = next.to_owned();
            }
        }
        if let Some(command) = self.command.as_ref() {
            preset.command = normalize_command_text(command);
        }
        if let Some(enabled) = self.enabled {
            preset.enabled = enabled;
        }
        if let Some(collapsed) = self.collapsed {
            preset.collapsed = collapsed;
        }
        if let Some(target_window_title) = self.target_window_title.as_ref() {
            let next = target_window_title.trim();
            preset.target_window_title = if next.is_empty() {
                None
            } else {
                Some(next.to_owned())
            };
        }
        if let Some(extra_target_window_titles) = self.extra_target_window_titles.as_ref() {
            preset.extra_target_window_titles = extra_target_window_titles
                .iter()
                .map(|title| title.trim())
                .filter(|title| !title.is_empty())
                .map(|title| title.to_owned())
                .collect();
        }
        if let Some(match_duplicate_window_titles) = self.match_duplicate_window_titles {
            preset.match_duplicate_window_titles = match_duplicate_window_titles;
        }
        preset.use_powershell = false;
    }
}

pub fn generate_command_preset_patch_groq(
    settings: &GroqSettings,
    prompt: &str,
    system_instruction: &str,
) -> Result<CommandPresetPatch> {
    let text = groq_chat_completion_text(
        settings,
        prompt,
        system_instruction,
        Some(serde_json::json!({"type": "json_object"})),
    )?;
    parse_custom_preset_patch_json(&text, prompt)
}

pub fn normalize_command_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let lines = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if lines.len() > 1 {
        return lines.join(" ");
    }

    let single_line = lines.first().copied().unwrap_or(trimmed);
    if single_line.len() >= 2 {
        let mut chars = single_line.chars();
        let first = chars.next().unwrap_or_default();
        let last = single_line.chars().last().unwrap_or_default();
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            let inner = single_line[1..single_line.len() - 1].trim();
            return inner.to_owned();
        }
    }

    single_line.to_owned()
}

fn parse_custom_preset_patch_json(text: &str, _prompt: &str) -> Result<CommandPresetPatch> {
    let mut attempts = Vec::new();
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        attempts.push(trimmed.to_owned());
    }

    if let Some(block) = extract_fenced_json_block(trimmed) {
        attempts.push(block);
    }

    if let Some(candidate) = extract_json_span(trimmed) {
        attempts.push(candidate);
    }

    let mut last_error = None;
    for candidate in attempts {
        match serde_json::from_str::<CommandPresetPatch>(&candidate) {
            Ok(patch) => return Ok(patch),
            Err(error) => last_error = Some(error.into()),
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("AI output did not contain valid custom preset JSON")))
}

fn extract_fenced_json_block(text: &str) -> Option<String> {
    let start = text.find("```")?;
    let rest = &text[start + 3..];
    let rest = rest.strip_prefix("json").unwrap_or(rest).trim_start();
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_owned())
}

fn extract_json_span(text: &str) -> Option<String> {
    let array_start = text.find('[');
    let object_start = text.find('{');
    let start = match (array_start, object_start) {
        (Some(a), Some(o)) => a.min(o),
        (Some(a), None) => a,
        (None, Some(o)) => o,
        (None, None) => return None,
    };
    let tail = &text[start..];
    let end = if tail.starts_with('[') {
        text.rfind(']')?
    } else {
        text.rfind('}')?
    };
    if end < start {
        return None;
    }
    Some(text[start..=end].trim().to_owned())
}

#[derive(Deserialize)]
struct GroqChatResponse {
    choices: Vec<GroqChoice>,
}

#[derive(Deserialize)]
struct GroqChoice {
    message: GroqMessage,
}

#[derive(Deserialize)]
struct GroqMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct MemeReplyQuery {
    query: String,
}

#[derive(Deserialize)]
struct DuckDuckGoImageSearchResponse {
    #[serde(default)]
    results: Vec<DuckDuckGoImageResult>,
}

#[derive(Deserialize)]
struct DuckDuckGoImageResult {
    image: String,
}

pub fn copy_funny_meme_reply_to_clipboard(
    settings: &GroqSettings,
    source_text: &str,
) -> Result<String> {
    let query = generate_meme_reply_query_groq(settings, source_text)?;
    let image = fetch_first_meme_image(&query)?;
    copy_image_to_clipboard(image)?;
    Ok(query)
}

fn generate_meme_reply_query_groq(settings: &GroqSettings, source_text: &str) -> Result<String> {
    let trimmed = source_text.trim();
    if trimmed.is_empty() {
        bail!("Funny Meme Reply input is empty");
    }

    let prompt = format!(
        "Turn this message into one short meme image search query.\n\
Return JSON with exactly one field: query.\n\
Rules:\n\
- 2 to 6 words only\n\
- plain lowercase English\n\
- no punctuation except spaces\n\
- optimize for finding a funny reaction meme image fast\n\
- no explanation\n\
Message:\n{}",
        trimmed
    );

    let system_instruction = "You turn chat messages into one strong meme image search query.";
    let text = groq_chat_completion_text(
        settings,
        &prompt,
        system_instruction,
        Some(serde_json::json!({"type": "json_object"})),
    )?;
    let parsed: MemeReplyQuery =
        serde_json::from_str(text.trim()).context("Groq did not return a valid meme query JSON")?;
    let query = parsed.query.trim().to_owned();
    if query.is_empty() {
        bail!("Groq returned an empty meme search query");
    }
    Ok(query)
}

fn fetch_first_meme_image(query: &str) -> Result<DynamicImage> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("MacroNest/1.1")
        .build()
        .context("Failed to prepare the image search client")?;
    let token = fetch_duckduckgo_vqd(&client, query)?;
    let results = fetch_duckduckgo_image_results(&client, query, &token)?;
    if results.is_empty() {
        bail!("No meme image results were found");
    }

    let mut last_error = None;
    for result in results.into_iter().take(4) {
        let image_url = result.image.trim();
        if image_url.is_empty() || image_url.ends_with(".svg") {
            continue;
        }
        match download_image(&client, image_url) {
            Ok(image) => return Ok(image),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Could not download a usable meme image")))
}

fn fetch_duckduckgo_vqd(client: &reqwest::blocking::Client, query: &str) -> Result<String> {
    let html = client
        .get("https://duckduckgo.com/")
        .query(&[("q", query), ("iax", "images"), ("ia", "images")])
        .send()
        .context("Failed to open DuckDuckGo image search")?
        .error_for_status()
        .context("DuckDuckGo image search returned an error")?
        .text()
        .context("Failed to read DuckDuckGo image search response")?;

    extract_duckduckgo_vqd(&html).context("DuckDuckGo did not return an image search token")
}

fn extract_duckduckgo_vqd(html: &str) -> Option<String> {
    for marker in ["vqd=\"", "vqd='", "vqd="] {
        let start = html.find(marker)?;
        let tail = &html[start + marker.len()..];
        if marker == "vqd=" {
            let value = tail
                .split(['&', '\'', '"', '\n', '\r', ';'])
                .next()
                .unwrap_or_default()
                .trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        } else {
            let quote = if marker.ends_with('"') { '"' } else { '\'' };
            let end = tail.find(quote)?;
            let value = tail[..end].trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn fetch_duckduckgo_image_results(
    client: &reqwest::blocking::Client,
    query: &str,
    vqd: &str,
) -> Result<Vec<DuckDuckGoImageResult>> {
    let response = client
        .get("https://duckduckgo.com/i.js")
        .query(&[
            ("l", "us-en"),
            ("o", "json"),
            ("q", query),
            ("vqd", vqd),
            ("f", ",,,"),
            ("p", "1"),
        ])
        .header("Referer", "https://duckduckgo.com/")
        .send()
        .context("Failed to fetch DuckDuckGo image results")?
        .error_for_status()
        .context("DuckDuckGo image result request failed")?;

    let payload: DuckDuckGoImageSearchResponse = response
        .json()
        .context("Failed to parse DuckDuckGo image results")?;
    Ok(payload.results)
}

fn download_image(client: &reqwest::blocking::Client, image_url: &str) -> Result<DynamicImage> {
    let bytes = client
        .get(image_url)
        .header("Referer", "https://duckduckgo.com/")
        .send()
        .with_context(|| format!("Failed to download meme image: {image_url}"))?
        .error_for_status()
        .with_context(|| format!("Meme image request failed: {image_url}"))?
        .bytes()
        .context("Failed to read meme image bytes")?;

    image::load_from_memory(&bytes).context("Downloaded meme image format is not supported")
}

fn copy_image_to_clipboard(image: DynamicImage) -> Result<()> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut clipboard = Clipboard::new().context("Failed to open the clipboard")?;
    clipboard
        .set_image(ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Owned(rgba.into_raw()),
        })
        .context("Failed to copy the meme image to the clipboard")
}

fn groq_chat_completion_text(
    settings: &GroqSettings,
    prompt: &str,
    system_instruction: &str,
    response_format: Option<serde_json::Value>,
) -> Result<String> {
    if settings.api_key.trim().is_empty() {
        bail!("Enter a Groq API key first");
    }
    let model = "openai/gpt-oss-120b";

    let url = "https://api.groq.com/openai/v1/chat/completions";
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_instruction.trim()},
            {"role": "user", "content": prompt.trim()}
        ],
        "temperature": 0.0,
    });
    if let Some(response_format) = response_format {
        body["response_format"] = response_format;
    }

    let response = reqwest::blocking::Client::new()
        .post(url)
        .bearer_auth(settings.api_key.trim())
        .json(&body)
        .send()
        .context("Failed to call the Groq API")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().unwrap_or_default();
        let error_message = if error_body.trim().is_empty() {
            format!("Groq returned an error response: HTTP {status}")
        } else {
            format!("Groq returned an error response: HTTP {status}\n{error_body}")
        };
        bail!(error_message);
    }

    let payload: GroqChatResponse = response
        .json()
        .context("Failed to parse the Groq response")?;

    payload
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .context("Groq did not return any text")
}

pub fn generate_groq_ai_response(
    settings: &GroqSettings,
    prompt: &str,
) -> Result<String> {
    groq_chat_completion_text(
        settings,
        prompt,
        "You are a helpful AI assistant. Provide a concise, direct response to the user's request. Do not include conversational filler, greetings, or markdown formatting unless requested.",
        None,
    )
}

pub fn generate_ai_response(settings: &GroqSettings, prompt: &str) -> Result<String> {
    match settings.provider {
        AiResponseProvider::Groq => generate_groq_ai_response(settings, prompt),
        AiResponseProvider::GeminiLive => {
            generate_gemini_live_ai_response(&settings.gemini_api_key, prompt)
        }
    }
}

fn generate_gemini_live_ai_response(api_key: &str, prompt: &str) -> Result<String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("Gemini API key is empty");
    }
    if prompt.trim().is_empty() {
        bail!("AI Response request prompt is empty");
    }

    let host = "generativelanguage.googleapis.com";
    let addr = format!("{host}:443")
        .to_socket_addrs()
        .context("Failed to resolve Gemini")?
        .next()
        .context("Gemini address was not found")?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(15)))?;
    let tls = TlsConnector::new()?.connect(host, tcp)?;
    let url = format!(
        "wss://{host}/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={api_key}"
    );
    let (mut socket, _) = client(url, tls).context("Gemini WebSocket handshake failed")?;

    socket.write(Message::Text(
        serde_json::json!({
            "setup": {
                "model": "models/gemini-3.1-flash-live-preview",
                "generationConfig": { "responseModalities": ["AUDIO"] },
                "outputAudioTranscription": {},
                "tools": [{ "googleSearch": {} }],
                "systemInstruction": {
                    "parts": [{ "text": "Provide a concise, direct answer in the user's language. Do not add greetings or markdown unless requested." }]
                }
            }
        })
        .to_string()
        .into(),
    ))?;
    socket.flush()?;

    let setup_started = Instant::now();
    loop {
        if setup_started.elapsed() > Duration::from_secs(10) {
            bail!("Gemini Live setup timed out");
        }
        let Message::Text(text) = socket.read()? else { continue };
        let value: serde_json::Value = serde_json::from_str(&text)?;
        if value.get("setupComplete").is_some() {
            break;
        }
        if let Some(error) = value.get("error") {
            bail!("Gemini Live error: {error}");
        }
    }

    socket.write(Message::Text(
        serde_json::json!({
            "clientContent": {
                "turns": [{ "role": "user", "parts": [{ "text": prompt }] }],
                "turnComplete": true
            }
        })
        .to_string()
        .into(),
    ))?;
    socket.flush()?;

    let mut response = String::new();
    let turn_started = Instant::now();
    loop {
        if turn_started.elapsed() > Duration::from_secs(30) {
            bail!("Gemini Live response timed out");
        }
        let Message::Text(text) = socket.read()? else { continue };
        let value: serde_json::Value = serde_json::from_str(&text)?;
        if let Some(error) = value.get("error") {
            bail!("Gemini Live error: {error}");
        }
        if let Some(text) = value
            .pointer("/serverContent/outputTranscription/text")
            .and_then(|value| value.as_str())
        {
            response.push_str(text);
        }
        if value
            .pointer("/serverContent/turnComplete")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            break;
        }
    }

    let response = response.trim().to_owned();
    if response.is_empty() {
        bail!("Gemini Live returned no text transcription");
    }
    Ok(response)
}
