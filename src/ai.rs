use std::borrow::Cow;

use anyhow::{bail, Context, Result};
use arboard::{Clipboard, ImageData};
use image::DynamicImage;
use serde::Deserialize;

use crate::model::{CommandPreset, GroqSettings};

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
struct MemeReplyPlan {
    reaction: String,
    #[serde(default)]
    template_names: Vec<String>,
    #[serde(default)]
    queries: Vec<String>,
}

#[derive(Deserialize)]
struct DuckDuckGoImageSearchResponse {
    #[serde(default)]
    results: Vec<DuckDuckGoImageResult>,
}

#[derive(Deserialize)]
struct DuckDuckGoImageResult {
    #[serde(default)]
    title: String,
    image: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    encoding_format: String,
}

#[derive(Deserialize)]
struct ImgflipMemesResponse {
    success: bool,
    data: ImgflipMemesData,
}

#[derive(Deserialize)]
struct ImgflipMemesData {
    memes: Vec<ImgflipMeme>,
}

#[derive(Clone, Deserialize)]
struct ImgflipMeme {
    name: String,
    url: String,
    width: u32,
    height: u32,
}

pub fn copy_funny_meme_reply_to_clipboard(
    settings: &GroqSettings,
    source_text: &str,
) -> Result<String> {
    let plan = generate_meme_reply_plan_groq(settings, source_text)?;
    let image = fetch_best_meme_image(&plan)?;
    copy_image_to_clipboard(image)?;
    Ok(plan.reaction)
}

fn generate_meme_reply_plan_groq(
    settings: &GroqSettings,
    source_text: &str,
) -> Result<MemeReplyPlan> {
    let trimmed = source_text.trim();
    if trimmed.is_empty() {
        bail!("Funny Meme Reply input is empty");
    }

    let prompt = format!(
        "Turn this message into a meme reply search plan.\n\
Return JSON with exactly these fields: reaction, template_names, queries.\n\
Rules:\n\
- reaction: 2 to 5 lowercase English words describing the reaction intent\n\
- template_names: 0 to 4 well-known meme or reaction names\n\
- queries: 4 to 8 search queries ordered from best to fallback\n\
- every query must be plain English and optimized to find an actual meme image or reaction image\n\
- prefer reaction-image style queries like \"side eye reaction meme\", \"confused blinking guy meme\", \"bruh reaction image\"\n\
- do not explain the joke\n\
- do not include markdown\n\
- if the input is generic, still choose a specific reaction that would be funny to send back\n\
Message:\n{}",
        trimmed
    );

    let system_instruction =
        "You turn chat messages into strong meme reaction search plans for image search engines.";
    let text = groq_chat_completion_text(
        settings,
        &prompt,
        system_instruction,
        Some(serde_json::json!({"type": "json_object"})),
    )?;
    let mut parsed: MemeReplyPlan =
        serde_json::from_str(text.trim()).context("Groq did not return a valid meme plan JSON")?;
    parsed.reaction = parsed.reaction.trim().to_owned();
    parsed.template_names = parsed
        .template_names
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();
    parsed.queries = parsed
        .queries
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();

    if parsed.reaction.is_empty() {
        bail!("Groq returned an empty meme reaction");
    }
    if parsed.queries.is_empty() {
        parsed.queries = build_fallback_queries(&parsed.reaction, &parsed.template_names);
    }
    Ok(parsed)
}

fn build_fallback_queries(reaction: &str, template_names: &[String]) -> Vec<String> {
    let mut queries = Vec::new();
    let trimmed_reaction = reaction.trim();
    if !trimmed_reaction.is_empty() {
        queries.push(format!("{trimmed_reaction} reaction meme"));
        queries.push(format!("{trimmed_reaction} reaction image"));
        queries.push(format!("{trimmed_reaction} meme"));
    }
    for name in template_names {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            queries.push(trimmed.to_owned());
            queries.push(format!("{trimmed} meme"));
            queries.push(format!("{trimmed} reaction image"));
        }
    }
    dedupe_strings_preserve_order(queries)
}

fn fetch_best_meme_image(plan: &MemeReplyPlan) -> Result<DynamicImage> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("MacroNest/1.1")
        .build()
        .context("Failed to prepare the image search client")?;

    let queries = dedupe_strings_preserve_order({
        let mut combined = plan.queries.clone();
        combined.extend(build_fallback_queries(&plan.reaction, &plan.template_names));
        combined
    });

    let mut best_result = None;
    let mut best_score = i32::MIN;
    let mut last_error = None;

    for query in queries.iter().take(8) {
        let token = match fetch_duckduckgo_vqd(&client, query) {
            Ok(token) => token,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        let results = match fetch_duckduckgo_image_results(&client, query, &token) {
            Ok(results) => results,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        for result in results.into_iter().take(12) {
            let score =
                score_duckduckgo_image_result(query, &plan.reaction, &plan.template_names, &result);
            if score > best_score {
                best_score = score;
                best_result = Some(result);
            }
        }
    }

    if let Some(result) = best_result {
        match download_image(&client, result.image.trim()) {
            Ok(image) => return Ok(image),
            Err(error) => last_error = Some(error),
        }
    }

    if let Some(result) = match_imgflip_template(&client, plan)? {
        return download_image(&client, result.url.trim());
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Could not find a usable meme image")))
}

fn score_duckduckgo_image_result(
    query: &str,
    reaction: &str,
    template_names: &[String],
    result: &DuckDuckGoImageResult,
) -> i32 {
    let mut score = 0;
    let title = normalize_search_text(&result.title);
    let url = normalize_search_text(&result.url);
    let source = normalize_search_text(&result.source);
    let image = normalize_search_text(&result.image);
    let query_text = normalize_search_text(query);
    let reaction_text = normalize_search_text(reaction);

    for term in query_text.split_whitespace() {
        if title.contains(term) || url.contains(term) {
            score += 8;
        }
    }
    for term in reaction_text.split_whitespace() {
        if title.contains(term) || url.contains(term) {
            score += 10;
        }
    }
    for template in template_names {
        let template_text = normalize_search_text(template);
        if !template_text.is_empty()
            && (title.contains(&template_text) || url.contains(&template_text))
        {
            score += 30;
        }
    }

    let meme_hints = [
        "meme",
        "reaction",
        "imgflip",
        "knowyourmeme",
        "tenor",
        "giphy",
        "meme arsenal",
    ];
    for hint in meme_hints {
        if title.contains(hint)
            || url.contains(hint)
            || source.contains(hint)
            || image.contains(hint)
        {
            score += 18;
        }
    }

    match result.encoding_format.as_str() {
        "jpeg" | "jpg" | "png" | "webp" => score += 25,
        "gif" | "animatedgif" => score += 8,
        "svg" => score -= 200,
        _ => {}
    }

    if result.width >= 300 && result.height >= 300 {
        score += 10;
    }
    if result.width < 120 || result.height < 120 {
        score -= 40;
    }
    if image.ends_with(".svg") {
        score -= 200;
    }

    score
}

fn normalize_search_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect()
}

fn dedupe_strings_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            deduped.push(trimmed.to_owned());
        }
    }
    deduped
}

fn match_imgflip_template(
    client: &reqwest::blocking::Client,
    plan: &MemeReplyPlan,
) -> Result<Option<ImgflipMeme>> {
    let response = client
        .get("https://api.imgflip.com/get_memes")
        .send()
        .context("Failed to fetch Imgflip meme templates")?
        .error_for_status()
        .context("Imgflip meme template request failed")?;
    let payload: ImgflipMemesResponse = response
        .json()
        .context("Failed to parse Imgflip meme templates")?;
    if !payload.success {
        bail!("Imgflip did not return a successful meme template response");
    }

    let wanted_names = dedupe_strings_preserve_order({
        let mut names = plan.template_names.clone();
        names.push(plan.reaction.clone());
        names.extend(plan.queries.clone());
        names
    });

    let mut best_match: Option<ImgflipMeme> = None;
    let mut best_score = 0;
    for meme in payload.data.memes {
        if meme.url.trim().is_empty() {
            continue;
        }
        let meme_name = normalize_search_text(&meme.name);
        let mut score = 0;
        for wanted in &wanted_names {
            let wanted_name = normalize_search_text(wanted);
            if wanted_name.is_empty() {
                continue;
            }
            if meme_name == wanted_name {
                score += 200;
            } else if meme_name.contains(&wanted_name) || wanted_name.contains(&meme_name) {
                score += 80;
            } else {
                let wanted_terms = wanted_name.split_whitespace().collect::<Vec<_>>();
                let overlap = wanted_terms
                    .iter()
                    .filter(|term| meme_name.contains(**term))
                    .count() as i32;
                score += overlap * 14;
            }
        }
        if meme.width >= 300 && meme.height >= 300 {
            score += 8;
        }
        if score > best_score {
            best_score = score;
            best_match = Some(meme);
        }
    }

    Ok(if best_score >= 70 { best_match } else { None })
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
    let model = settings.model.trim();
    if model.is_empty() {
        bail!("The Groq model name is empty");
    }

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
