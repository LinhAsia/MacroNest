use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::model::{AppState, ProfileRecord, VietnameseInputMode, VisionPreset};

const BUNDLED_ARDUINO_RAWHID_FIRMWARE: &[u8] =
    include_bytes!("../assets/firmware-rawhid.hex");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateLoadStatus {
    Loaded,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub state_file: PathBuf,
    pub profiles_dir: PathBuf,
    pub asset_dir: PathBuf,
    pub icon_file: PathBuf,
    pub icon_file_disabled: PathBuf,
    pub vision_dir: PathBuf,
    pub vision_template_file: PathBuf,
    pub bin_dir: PathBuf,
    pub interception_zip: PathBuf,
    pub interception_package_dir: PathBuf,
    pub interception_installer_exe: PathBuf,
    pub opencv_dll: PathBuf,
    pub opencv_videoio_ffmpeg_dll: PathBuf,
    pub interception_dll: PathBuf,
    pub arduino_tools_zip: PathBuf,
    pub avrdude_exe: PathBuf,
    pub avrdude_conf: PathBuf,
    pub arduino_firmware_hex: PathBuf,
    pub arduino_rawhid_firmware_hex: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("com", "", "MacroNest")
            .context("Failed to locate the application data folder")?;
        let root = dirs.data_local_dir().to_path_buf();

        // Migrate from old Crosshair/Crosshair directory to new single MacroNest directory
        if let Some(old_dirs) = ProjectDirs::from("com", "Crosshair", "Crosshair") {
            let old_root = old_dirs.data_local_dir().to_path_buf();
            if old_root.exists() && !root.exists() {
                let _ = fs::create_dir_all(root.parent().unwrap());
                let _ = fs::rename(&old_root, &root);
            }
        }
        let state_file = root.join("state.json");
        let profiles_dir = root.join("profiles");
        let asset_dir = root.join("custom-crosshairs");
        let icon_file = root.join("app-icon.ico");
        let icon_file_disabled = root.join("app-icon-disabled.ico");
        let vision_dir = root.join("vision");
        let vision_template_file = vision_dir.join("template.png");
        let bin_dir = root.join("bin");
        let interception_zip = bin_dir.join("Interception.zip");
        let interception_package_dir = bin_dir.join("Interception");
        let interception_installer_exe = interception_package_dir
            .join("command line installer")
            .join("install-interception.exe");
        let opencv_dll = bin_dir.join("opencv_world4100.dll");
        let opencv_videoio_ffmpeg_dll = bin_dir.join("opencv_videoio_ffmpeg4100_64.dll");
        let interception_dll = bin_dir.join("interception.dll");
        let arduino_tools_zip = bin_dir.join("arduino_tools.zip");
        let avrdude_exe = bin_dir.join("avrdude.exe");
        let avrdude_conf = bin_dir.join("avrdude.conf");
        let arduino_firmware_hex = bin_dir.join("firmware.hex");
        let arduino_rawhid_firmware_hex = bin_dir.join("firmware_rawhid.hex");

        fs::create_dir_all(&root)?;
        fs::create_dir_all(&profiles_dir)?;
        fs::create_dir_all(&asset_dir)?;
        fs::create_dir_all(&vision_dir)?;
        fs::create_dir_all(&bin_dir)?;
        ensure_opencv_videoio_ffmpeg_plugin(&opencv_videoio_ffmpeg_dll);
        ensure_bundled_file(
            &arduino_rawhid_firmware_hex,
            BUNDLED_ARDUINO_RAWHID_FIRMWARE,
        )?;

        Ok(Self {
            root,
            state_file,
            profiles_dir,
            asset_dir,
            icon_file,
            icon_file_disabled,
            vision_dir,
            vision_template_file,
            bin_dir,
            interception_zip,
            interception_package_dir,
            interception_installer_exe,
            opencv_dll,
            opencv_videoio_ffmpeg_dll,
            interception_dll,
            arduino_tools_zip,
            avrdude_exe,
            avrdude_conf,
            arduino_firmware_hex,
            arduino_rawhid_firmware_hex,
        })
    }

    pub fn ensure_arduino_runtime_files(&self) -> Result<()> {
        ensure_bundled_file(
            &self.arduino_rawhid_firmware_hex,
            BUNDLED_ARDUINO_RAWHID_FIRMWARE,
        )
    }

    pub fn vision_template_file_for(&self, preset_id: u32) -> PathBuf {
        self.vision_dir.join(format!("preset-{preset_id}.png"))
    }

    pub fn load_state(&self) -> Result<(AppState, StateLoadStatus)> {
        // Fallback: Copy interception.dll from local assets folder if not present in bin directory
        if !self.interception_dll.exists() {
            let local_asset = std::env::current_dir()
                .unwrap_or_default()
                .join("assets")
                .join("interception.dll");
            if local_asset.exists() {
                let _ = fs::copy(&local_asset, &self.interception_dll);
            }
        }

        let (mut state, status) = if !self.state_file.exists() {
            (AppState::default(), StateLoadStatus::Loaded)
        } else {
            let content = fs::read_to_string(&self.state_file)?;
            let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
            match serde_json::from_str(content) {
                Ok(state) => (state, StateLoadStatus::Loaded),
                Err(error) => {
                    if content.trim().is_empty() {
                        eprintln!("state.json was empty; recreating defaults.");
                        (AppState::default(), StateLoadStatus::Loaded)
                    } else {
                        anyhow::bail!(
                            "state.json is invalid: {error}. Please fix the file or restore from a backup to prevent data loss. The application will not start in this state."
                        );
                    }
                }
            }
        };

        let mut disk_profiles = self.load_profiles().unwrap_or_default();
        let mut migrated = false;
        if !state.profiles.is_empty() {
            for sp in &state.profiles {
                if !disk_profiles.iter().any(|dp| dp.name == sp.name) {
                    disk_profiles.push(sp.clone());
                    migrated = true;
                }
            }
        }
        if migrated {
            let _ = self.save_profiles(&disk_profiles);
        }

        state.profiles = disk_profiles;

        if state.selected_profile.is_none() {
            state.selected_profile = state.profiles.first().map(|p| p.name.clone());
        }
        if let Some(selected_name) = state.selected_profile.as_deref() {
            if let Some(profile) = state
                .profiles
                .iter()
                .find(|profile| profile.name == selected_name)
            {
                state.active_style = profile.style.clone();
                state.active_style.enabled = profile.enabled;
            }
        }
        if matches!(state.vietnamese_input_mode, VietnameseInputMode::Off) {
            state.vietnamese_input_mode = VietnameseInputMode::Telex;
        }
        for profile in &mut state.profiles {
            profile.collapsed = true;
        }
        let next_preset_id = state
            .window_presets
            .iter()
            .map(|preset| preset.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_preset_id < next_preset_id {
            state.next_preset_id = next_preset_id;
        }
        for preset in &mut state.window_presets {
            preset.collapsed = true;
        }
        state.window_expand_controls.enabled = false;
        state.window_focus_presets.clear();
        state.next_window_focus_preset_id = 1;
        for preset in &mut state.master_presets {
            preset.window_focus_presets.clear();
        }
        if state.vision_presets.is_empty() {
            let mut preset = VisionPreset::default();
            preset.enabled = state.vision_settings.enabled || self.vision_template_file.exists();
            preset.hotkey = state.vision_settings.trigger_hotkey.clone();
            preset.click_after_move = state.vision_settings.click_after_move;
            state.vision_presets.push(preset);
        }
        let next_vision_preset_id = state
            .vision_presets
            .iter()
            .map(|preset| preset.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_vision_preset_id < next_vision_preset_id {
            state.next_vision_preset_id = next_vision_preset_id;
        }
        for preset in &mut state.vision_presets {
            preset.collapsed = true;
            preset.click_after_move = false;
        }
        state.active_panel = crate::model::AppPanel::Macros;
        if state.groq_settings.model.trim().is_empty()
            || state.groq_settings.model.trim() == "llama-3.1-8b-instant"
        {
            state.groq_settings.model = "openai/gpt-oss-120b".to_owned();
        }
        let legacy_vision_dir = self.root.join("image-search");
        if legacy_vision_dir.exists() {
            let _ = fs::create_dir_all(&self.vision_dir);
            if let Ok(entries) = fs::read_dir(&legacy_vision_dir) {
                for entry in entries.flatten() {
                    let old_path = entry.path();
                    if old_path.is_file() {
                        if let Some(file_name) = old_path.file_name() {
                            let new_path = self.vision_dir.join(file_name);
                            if !new_path.exists() {
                                let _ = fs::rename(&old_path, &new_path);
                            }
                        }
                    }
                }
            }
            let _ = fs::remove_dir_all(&legacy_vision_dir);
        }

        let legacy_vision_template = self.vision_template_file.exists();
        if legacy_vision_template {
            let first_template = state
                .vision_presets
                .first()
                .map(|preset| self.vision_template_file_for(preset.id));
            if let Some(first_template) = first_template
                && !first_template.exists()
            {
                let _ = fs::copy(&self.vision_template_file, &first_template);
            }
            let _ = fs::remove_file(&self.vision_template_file);
        }
        if !state.macro_presets.is_empty() {
            let mut used_preset_ids = state
                .macro_groups
                .iter()
                .flat_map(|group| group.presets.iter().map(|preset| preset.id))
                .collect::<std::collections::HashSet<_>>();
            let mut next_generated_preset_id =
                used_preset_ids.iter().copied().max().unwrap_or(0) + 1;
            let migrated_presets = state
                .macro_presets
                .clone()
                .into_iter()
                .map(|legacy| {
                    let mut preset_id = legacy.id;
                    if !used_preset_ids.insert(preset_id) {
                        while used_preset_ids.contains(&next_generated_preset_id) {
                            next_generated_preset_id += 1;
                        }
                        preset_id = next_generated_preset_id;
                        used_preset_ids.insert(preset_id);
                        next_generated_preset_id += 1;
                    }
                    crate::model::MacroPreset {
                        id: preset_id,
                        enabled: legacy.enabled,
                        collapsed: legacy.collapsed,
                        trigger_mode: crate::model::MacroTriggerMode::Press,
                        pass_through_press: false,
                        pass_through_hold: false,
                        stop_on_retrigger_immediate: false,
                        release_requires_all_inputs_released: false,
                        release_wait_key: String::new(),
                        trigger_keys: String::new(),
                        hotkey: legacy.hotkey,
                        hold_stop_step_enabled: false,
                        hold_stop_step: crate::model::MacroStep::default(),
                        steps: legacy.steps,
                        record_hotkey: None,
                        acknowledged_infinite_loop: false,
                    }
                })
                .collect();
            let migrated_group_id = state
                .macro_groups
                .iter()
                .map(|group| group.id)
                .max()
                .unwrap_or(0)
                + 1;
            state.macro_groups.push(crate::model::MacroGroup {
                id: migrated_group_id,
                name: "Migrated Macros".to_owned(),
                enabled: true,
                collapsed: false,
                favorite: false,
                folder_id: None,
                target_window_title: None,
                extra_target_window_titles: Vec::new(),
                match_duplicate_window_titles: false,
                presets: migrated_presets,
            });
            state.macro_presets.clear();
        }
        if state.macro_folders.len() == 1 {
            let folder = &state.macro_folders[0];
            let is_auto_default_folder = folder.name == format!("Folder {}", folder.id)
                && state
                    .macro_groups
                    .iter()
                    .all(|group| group.folder_id == Some(folder.id));
            if is_auto_default_folder {
                for group in &mut state.macro_groups {
                    group.folder_id = None;
                }
                state.macro_folders.clear();
            }
        }
        let valid_folder_ids = state
            .macro_folders
            .iter()
            .map(|folder| folder.id)
            .collect::<std::collections::HashSet<_>>();
        for group in &mut state.macro_groups {
            if group
                .folder_id
                .is_some_and(|folder_id| !valid_folder_ids.contains(&folder_id))
            {
                group.folder_id = None;
            }
        }
        let next_macro_folder_id = state
            .macro_folders
            .iter()
            .map(|folder| folder.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_macro_folder_id < next_macro_folder_id {
            state.next_macro_folder_id = next_macro_folder_id;
        }
        let next_macro_group_id = state
            .macro_groups
            .iter()
            .map(|group| group.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_macro_group_id < next_macro_group_id {
            state.next_macro_group_id = next_macro_group_id;
        }
        let next_macro_preset_id = state
            .macro_groups
            .iter()
            .flat_map(|group| group.presets.iter().map(|preset| preset.id))
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_macro_preset_id < next_macro_preset_id {
            state.next_macro_preset_id = next_macro_preset_id;
        }
        for group in &mut state.macro_groups {
            for preset in &mut group.presets {
                preset.collapsed = true;
            }
        }
        let next_sound_preset_id = state
            .audio_settings
            .presets
            .iter()
            .map(|preset| preset.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.audio_settings.next_preset_id < next_sound_preset_id {
            state.audio_settings.next_preset_id = next_sound_preset_id;
        }
        let next_sound_library_id = state
            .audio_settings
            .library
            .iter()
            .map(|item| item.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.audio_settings.next_library_item_id < next_sound_library_id {
            state.audio_settings.next_library_item_id = next_sound_library_id;
        }
        for item in &mut state.audio_settings.library {
            item.collapsed = true;
        }
        let next_zoom_preset_id = state
            .zoom_presets
            .iter()
            .map(|preset| preset.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_zoom_preset_id < next_zoom_preset_id {
            state.next_zoom_preset_id = next_zoom_preset_id;
        }
        let next_master_preset_id = state
            .master_presets
            .iter()
            .map(|preset| preset.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_master_preset_id < next_master_preset_id {
            state.next_master_preset_id = next_master_preset_id;
        }
        for preset in &mut state.master_presets {
            preset.collapsed = true;
        }
        if state.selected_master_preset_id.is_none() {
            state.selected_master_preset_id = state.master_presets.first().map(|preset| preset.id);
        }
        for preset in &mut state.pin_presets {
            preset.collapsed = true;
        }
        for preset in &mut state.mouse_path_presets {
            preset.collapsed = true;
        }
        for preset in &mut state.mouse_sensitivity_presets {
            preset.collapsed = true;
        }
        for preset in &mut state.hud_presets {
            preset.collapsed = true;
        }
        for preset in &mut state.zoom_presets {
            preset.collapsed = true;
        }
        for group in &mut state.macro_groups {
            group.collapsed = true;
            for preset in &mut group.presets {
                preset.collapsed = true;
                if preset.hold_stop_step.if_operator.is_empty()
                    || preset.hold_stop_step.if_operator == "="
                {
                    preset.hold_stop_step.if_operator = "==".to_string();
                }
                for cond in &mut preset.hold_stop_step.extra_conditions {
                    if cond.operator.is_empty() || cond.operator == "=" {
                        cond.operator = "==".to_string();
                    }
                }
                for step in &mut preset.steps {
                    if step.if_operator.is_empty() || step.if_operator == "=" {
                        step.if_operator = "==".to_string();
                    }
                    for cond in &mut step.extra_conditions {
                        if cond.operator.is_empty() || cond.operator == "=" {
                            cond.operator = "==".to_string();
                        }
                    }
                }
            }
        }

        // Dọn dẹp triệt để các group bị lỗi deserialize cũ (chứa IfConditionType::Unknown) khỏi database
        state.macro_groups.retain(|group| {
            let has_unknown = group.presets.iter().any(|preset| {
                preset.hold_stop_step.if_condition_type == crate::model::IfConditionType::Unknown
                    || preset
                        .hold_stop_step
                        .extra_conditions
                        .iter()
                        .any(|c| c.condition_type == crate::model::IfConditionType::Unknown)
                    || preset.steps.iter().any(|step| {
                        step.if_condition_type == crate::model::IfConditionType::Unknown
                            || step
                                .extra_conditions
                                .iter()
                                .any(|c| c.condition_type == crate::model::IfConditionType::Unknown)
                    })
            });
            !has_unknown
        });
        for preset in &mut state.audio_settings.presets {
            preset.collapsed = true;
        }
        for item in &mut state.audio_settings.library {
            item.collapsed = true;
        }

        if !self.state_file.exists() {
            self.save_state(&state)?;
        } else {
            self.save_state(&state)?;
        }

        Ok((state, status))
    }

    pub fn save_state(&self, state: &AppState) -> Result<()> {
        let mut state = state.clone();
        state.macro_presets.clear();
        state.profiles.clear();
        state.window_focus_presets.clear();
        for preset in &mut state.master_presets {
            preset.window_focus_presets.clear();
        }
        let content = serde_json::to_string_pretty(&state)?;
        fs::write(&self.state_file, content)?;
        Ok(())
    }

    pub fn load_profiles(&self) -> Result<Vec<ProfileRecord>> {
        let mut profiles = Vec::new();
        for entry in fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read profile {}", path.display()))?;
            let profile: ProfileRecord = serde_json::from_str(&content)
                .with_context(|| format!("Profile is invalid: {}", path.display()))?;
            profiles.push(profile);
        }
        profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(profiles)
    }

    pub fn save_profiles(&self, profiles: &[ProfileRecord]) -> Result<()> {
        fs::create_dir_all(&self.profiles_dir)?;
        for entry in fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                let _ = fs::remove_file(path);
            }
        }
        for profile in profiles {
            let file = self
                .profiles_dir
                .join(format!("{}.json", sanitize_name(&profile.name)));
            let content = serde_json::to_string_pretty(profile)?;
            fs::write(file, content)?;
        }
        Ok(())
    }

    pub fn list_crosshair_assets(&self) -> Result<Vec<String>> {
        let mut assets = Vec::new();
        for entry in fs::read_dir(&self.asset_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if is_supported_asset(&path) {
                if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                    assets.push(name.to_owned());
                }
            }
        }
        assets.sort_by_key(|name| name.to_lowercase());
        Ok(assets)
    }

    pub fn asset_path(&self, asset_name: &str) -> PathBuf {
        self.asset_dir.join(asset_name)
    }
}

fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect();

    if cleaned.trim_matches('_').is_empty() {
        "profile".to_owned()
    } else {
        cleaned
    }
}

fn is_supported_asset(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("svg" | "png" | "jpg" | "jpeg" | "bmp" | "webp" | "ico")
    )
}

fn ensure_opencv_videoio_ffmpeg_plugin(target_path: &Path) {
    if target_path.exists() {
        return;
    }
    let Some(source_path) = find_local_opencv_videoio_ffmpeg_plugin() else {
        return;
    };
    if let Some(parent) = target_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::copy(source_path, target_path);
}

fn ensure_bundled_file(target_path: &Path, bytes: &[u8]) -> Result<()> {
    let needs_write = match fs::read(target_path) {
        Ok(existing) => existing != bytes,
        Err(_) => true,
    };

    if needs_write {
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target_path, bytes)?;
    }

    Ok(())
}

fn find_local_opencv_videoio_ffmpeg_plugin() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(current_dir) = env::current_dir() {
        candidates.push(
            current_dir
                .join("target")
                .join("tmp")
                .join("python_pkgs")
                .join("cv2")
                .join("opencv_videoio_ffmpeg4100_64.dll"),
        );
        candidates.push(
            current_dir
                .join("target")
                .join("tmp")
                .join("python_pkgs")
                .join("cv2")
                .join("opencv_videoio_ffmpeg4130_64.dll"),
        );
    }

    if let Ok(virtual_env) = env::var("VIRTUAL_ENV") {
        candidates.push(
            PathBuf::from(&virtual_env)
                .join("Lib")
                .join("site-packages")
                .join("cv2")
                .join("opencv_videoio_ffmpeg4100_64.dll"),
        );
        candidates.push(
            PathBuf::from(virtual_env)
                .join("Lib")
                .join("site-packages")
                .join("cv2")
                .join("opencv_videoio_ffmpeg4130_64.dll"),
        );
    }

    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let python_root = PathBuf::from(local_app_data)
            .join("Programs")
            .join("Python");
        if let Ok(entries) = fs::read_dir(&python_root) {
            for entry in entries.flatten() {
                let base = entry.path().join("Lib").join("site-packages").join("cv2");
                candidates.push(base.join("opencv_videoio_ffmpeg4100_64.dll"));
                candidates.push(base.join("opencv_videoio_ffmpeg4130_64.dll"));
            }
        }
    }

    candidates.into_iter().find(|path| path.exists())
}
