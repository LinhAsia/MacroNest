use std::collections::HashMap;

use once_cell::sync::Lazy;

use crate::model::UiLanguage;

static VI_JSON: &str = include_str!("../assets/lang/vi.json");

static VI_TRANSLATIONS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let parsed: HashMap<String, String> = serde_json::from_str(VI_JSON).unwrap_or_default();
    parsed
        .into_iter()
        .map(|(key, value)| {
            (
                Box::leak(key.into_boxed_str()) as &'static str,
                Box::leak(value.into_boxed_str()) as &'static str,
            )
        })
        .collect()
});

pub fn translate(language: UiLanguage, english: &'static str) -> Option<&'static str> {
    match language {
        UiLanguage::Vietnamese => VI_TRANSLATIONS.get(english).copied(),
        UiLanguage::English | UiLanguage::Icon => None,
    }
}
