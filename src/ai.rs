use anyhow::{Context, Result, bail};
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
