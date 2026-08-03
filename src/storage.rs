use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::model::{AppState, CrosshairStyle, ProfileRecord, VietnameseInputMode, VisionPreset};

const BUNDLED_ARDUINO_SERIAL_FIRMWARE: &[u8] = include_bytes!("../assets/firmware-serial.hex");

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
    pub ocr_dir: PathBuf,
    pub interception_zip: PathBuf,
    pub interception_package_dir: PathBuf,
    pub interception_installer_exe: PathBuf,
    pub opencv_dll: PathBuf,
    pub opencv_videoio_ffmpeg_dll: PathBuf,
    pub ffmpeg_exe: PathBuf,
    pub ffmpeg_zip: PathBuf,
    pub frida_helper_exe: PathBuf,
    pub frida_helper_zip: PathBuf,
    pub interception_dll: PathBuf,
    pub arduino_tools_zip: PathBuf,
    pub avrdude_exe: PathBuf,
    pub avrdude_conf: PathBuf,
    pub arduino_firmware_hex: PathBuf,
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
        let ocr_dir = root.join("ocr-models");
        let interception_zip = bin_dir.join("Interception.zip");
        let interception_package_dir = bin_dir.join("Interception");
        let interception_installer_exe = interception_package_dir
            .join("command line installer")
            .join("install-interception.exe");
        let opencv_dll = bin_dir.join("opencv_world4100.dll");
        let opencv_videoio_ffmpeg_dll = bin_dir.join("opencv_videoio_ffmpeg4100_64.dll");
        let ffmpeg_exe = bin_dir.join("ffmpeg.exe");
        let ffmpeg_zip = bin_dir.join("ffmpeg.exe.zip");
        let frida_helper_exe = bin_dir.join("frida-helper.exe");
        let frida_helper_zip = bin_dir.join("frida-helper.exe.zip");
        let interception_dll = bin_dir.join("interception.dll");
        let arduino_tools_zip = bin_dir.join("arduino_tools.zip");
        let avrdude_exe = bin_dir.join("avrdude.exe");
        let avrdude_conf = bin_dir.join("avrdude.conf");
        let arduino_firmware_hex = bin_dir.join("firmware.hex");

        fs::create_dir_all(&root)?;
        fs::create_dir_all(&profiles_dir)?;

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
            ocr_dir,
            interception_zip,
            interception_package_dir,
            interception_installer_exe,
            opencv_dll,
            opencv_videoio_ffmpeg_dll,
            ffmpeg_exe,
            ffmpeg_zip,
            frida_helper_exe,
            frida_helper_zip,
            interception_dll,
            arduino_tools_zip,
            avrdude_exe,
            avrdude_conf,
            arduino_firmware_hex,
        })
    }

    pub fn ensure_dirs_and_assets(&self) -> Result<()> {
        fs::create_dir_all(&self.asset_dir)?;
        fs::create_dir_all(&self.vision_dir)?;
        fs::create_dir_all(&self.bin_dir)?;
        fs::create_dir_all(&self.ocr_dir)?;
        ensure_opencv_videoio_ffmpeg_plugin(&self.opencv_videoio_ffmpeg_dll);
        ensure_bundled_file(&self.arduino_firmware_hex, BUNDLED_ARDUINO_SERIAL_FIRMWARE)?;
        Ok(())
    }

    pub fn ensure_arduino_runtime_files(&self) -> Result<()> {
        ensure_bundled_file(&self.arduino_firmware_hex, BUNDLED_ARDUINO_SERIAL_FIRMWARE)?;
        Ok(())
    }

    pub fn vision_template_file_for(&self, preset_id: u32) -> PathBuf {
        self.vision_dir.join(format!("preset-{preset_id}.png"))
    }

    fn state_backup_file(&self) -> PathBuf {
        self.state_file.with_extension("json.bak")
    }

    fn state_temp_file(&self) -> PathBuf {
        self.state_file.with_extension("json.tmp")
    }

    fn state_recovery_file(&self) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.root.join(format!("state-recovery-{ts}.json"))
    }

    fn read_state_file(path: &Path) -> Result<AppState> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
        if content.trim().is_empty() {
            anyhow::bail!("{} is empty", path.display());
        }
        serde_json::from_str(content)
            .with_context(|| format!("{} contains invalid JSON", path.display()))
    }

    fn latest_state_recovery_file(&self) -> Option<PathBuf> {
        let mut candidates = self.state_recovery_files();
        candidates.sort_by(|a, b| b.0.cmp(&a.0));
        candidates.into_iter().map(|(_, path)| path).next()
    }

    fn state_restore_sources(&self) -> Vec<PathBuf> {
        let mut restore_sources = Vec::new();
        let backup_file = self.state_backup_file();
        if backup_file.exists() {
            restore_sources.push(backup_file);
        }
        if let Some(recovery_file) = self.latest_state_recovery_file()
            && !restore_sources.iter().any(|path| path == &recovery_file)
        {
            restore_sources.push(recovery_file);
        }
        restore_sources
    }

    fn restore_state_from_backup(&self) -> Result<Option<AppState>> {
        for source in self.state_restore_sources() {
            let Ok(state) = Self::read_state_file(&source) else {
                continue;
            };
            let temp_file = self.write_state_temp_file(&state)?;
            if self.state_file.exists() {
                let _ = fs::remove_file(&self.state_file);
            }
            fs::rename(&temp_file, &self.state_file).with_context(|| {
                format!(
                    "Failed to restore {} from backup {}",
                    self.state_file.display(),
                    source.display()
                )
            })?;
            return Ok(Some(state));
        }

        Ok(None)
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
            if let Some(restored) = self.restore_state_from_backup()? {
                (restored, StateLoadStatus::Loaded)
            } else {
                (AppState::default(), StateLoadStatus::Loaded)
            }
        } else {
            match Self::read_state_file(&self.state_file) {
                Ok(state) => (state, StateLoadStatus::Loaded),
                Err(primary_error) => {
                    if let Some(restored) = self.restore_state_from_backup()? {
                        (restored, StateLoadStatus::Loaded)
                    } else {
                        anyhow::bail!(
                            "state.json could not be loaded: {primary_error}. No valid backup could be restored."
                        );
                    }
                }
            }
        };

        let disk_profiles = self.load_profiles().unwrap_or_default();
        if state.profiles.is_empty() {
            // Legacy fallback for installations that only have individual profile files.
            state.profiles = disk_profiles;
        } else {
            // The state snapshot is written together with every edit and therefore owns the
            // current X/Y offsets. Individual files can be stale after an interrupted save.
            for profile in disk_profiles {
                if !state
                    .profiles
                    .iter()
                    .any(|saved| saved.name == profile.name)
                {
                    state.profiles.push(profile);
                }
            }
        }

        if state.profiles.is_empty() {
            state.selected_profile = None;
            state.active_style = CrosshairStyle {
                enabled: false,
                ..CrosshairStyle::default()
            };
        } else {
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
        let next_layout_id = state
            .window_layouts
            .iter()
            .map(|layout| layout.id)
            .max()
            .unwrap_or(0)
            + 1;
        if state.next_window_layout_id < next_layout_id {
            state.next_window_layout_id = next_layout_id;
        }
        for layout in &mut state.window_layouts {
            layout.collapsed = true;
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

        self.migrate_legacy_vision_assets();

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
                        event_target_window_title: None,
                        event_extra_target_window_titles: Vec::new(),
                        event_match_duplicate_window_titles: true,
                        hold_stop_step_enabled: false,
                        hold_stop_step: crate::model::LazyMacroStep::default(),
                        press_stop_step_enabled: false,
                        press_stop_step: crate::model::LazyMacroStep::default(),
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
                if preset.press_stop_step.if_operator.is_empty()
                    || preset.press_stop_step.if_operator == "="
                {
                    preset.press_stop_step.if_operator = "==".to_string();
                }
                for cond in &mut preset.press_stop_step.extra_conditions {
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

        // Fully remove legacy groups that failed old deserialization (IfConditionType::Unknown) from the database
        state.macro_groups.retain(|group| {
            let has_unknown = group.presets.iter().any(|preset| {
                preset.hold_stop_step.if_condition_type == crate::model::IfConditionType::Unknown
                    || preset
                        .hold_stop_step
                        .extra_conditions
                        .iter()
                        .any(|c| c.condition_type == crate::model::IfConditionType::Unknown)
                    || preset.press_stop_step.if_condition_type
                        == crate::model::IfConditionType::Unknown
                    || preset
                        .press_stop_step
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

        Ok((state, status))
    }

    pub fn save_state(&self, state: &AppState) -> Result<()> {
        let state = state_snapshot_for_save(state);
        let temp_file = self.write_state_temp_file(&state)?;
        let backup_file = self.state_backup_file();
        if self.state_file.exists() {
            for (_, path) in self.state_recovery_files() {
                let _ = fs::remove_file(path);
            }
            let _ = fs::copy(&self.state_file, self.state_recovery_file());
            if backup_file.exists() {
                let _ = fs::remove_file(&backup_file);
            }
            fs::rename(&self.state_file, &backup_file).with_context(|| {
                format!(
                    "Failed to move {} to backup {}",
                    self.state_file.display(),
                    backup_file.display()
                )
            })?;
        }
        if let Err(error) = fs::rename(&temp_file, &self.state_file) {
            if !self.state_file.exists() && backup_file.exists() {
                let _ = fs::copy(&backup_file, &self.state_file);
            }
            let _ = fs::remove_file(&temp_file);
            return Err(error).with_context(|| {
                format!(
                    "Failed to promote {} to {}",
                    temp_file.display(),
                    self.state_file.display()
                )
            });
        }
        Ok(())
    }

    pub fn load_profiles(&self) -> Result<Vec<ProfileRecord>> {
        let mut profiles = Vec::new();
        for path in self.profile_json_paths()? {
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
        for path in self.profile_json_paths()? {
            let _ = fs::remove_file(path);
        }
        for profile in profiles {
            let file = self.profile_record_path(&profile.name);
            let content = serde_json::to_string_pretty(profile)?;
            fs::write(file, content)?;
        }
        Ok(())
    }

    pub fn asset_path(&self, asset_name: &str) -> PathBuf {
        self.asset_dir.join(asset_name)
    }

    fn profile_record_path(&self, profile_name: &str) -> PathBuf {
        self.profiles_dir
            .join(format!("{}.json", sanitize_name(profile_name)))
    }

    fn profile_json_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let path = entry.path();
            if is_json_file_path(&path) {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn write_state_temp_file(&self, state: &AppState) -> Result<PathBuf> {
        let temp_file = self.state_temp_file();
        let content = serde_json::to_string_pretty(state)?;
        write_text_file(&temp_file, &content)?;
        Ok(temp_file)
    }

    fn state_recovery_files(&self) -> Vec<(SystemTime, PathBuf)> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };

        entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if !path.is_file() {
                    return None;
                }
                let file_name = path.file_name().and_then(|n| n.to_str())?;
                if !is_state_recovery_file_name(file_name) {
                    return None;
                }
                let modified = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH);
                Some((modified, path))
            })
            .collect()
    }

    fn migrate_legacy_vision_assets(&self) {
        let legacy_vision_dir = self.root.join("image-search");
        if !legacy_vision_dir.exists() {
            return;
        }

        let _ = fs::create_dir_all(&self.vision_dir);
        if let Ok(entries) = fs::read_dir(&legacy_vision_dir) {
            for entry in entries.flatten() {
                let old_path = entry.path();
                if old_path.is_file()
                    && let Some(file_name) = old_path.file_name()
                {
                    let new_path = self.vision_dir.join(file_name);
                    if !new_path.exists() {
                        let _ = fs::rename(&old_path, &new_path);
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&legacy_vision_dir);
    }
}

fn state_snapshot_for_save(state: &AppState) -> AppState {
    let mut state = state.clone();
    state.macro_presets.clear();
    state.window_focus_presets.clear();
    for preset in &mut state.master_presets {
        preset.window_focus_presets.clear();
    }
    state
}

fn write_text_file(path: &Path, content: &str) -> Result<()> {
    let mut file =
        fs::File::create(path).with_context(|| format!("Failed to create {}", path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))?;
    file.sync_all().ok();
    Ok(())
}

fn is_json_file_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("json")
}

fn is_state_recovery_file_name(file_name: &str) -> bool {
    file_name.starts_with("state-recovery-") && file_name.ends_with(".json")
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

fn ensure_opencv_videoio_ffmpeg_plugin(target_path: &Path) {
    if target_path.exists() {
        return;
    }
    let Some(source_path) = find_local_opencv_videoio_ffmpeg_plugin() else {
        return;
    };
    let _ = ensure_parent_dir(target_path);
    let _ = fs::copy(source_path, target_path);
}

fn ensure_bundled_file(target_path: &Path, bytes: &[u8]) -> Result<()> {
    let needs_write = match fs::read(target_path) {
        Ok(existing) => existing != bytes,
        Err(_) => true,
    };

    if needs_write {
        ensure_parent_dir(target_path)?;
        fs::write(target_path, bytes)?;
    }

    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn find_local_opencv_videoio_ffmpeg_plugin() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(current_dir) = env::current_dir() {
        push_opencv_videoio_ffmpeg_candidates(
            &mut candidates,
            current_dir
                .join("target")
                .join("tmp")
                .join("python_pkgs")
                .join("cv2"),
        );
    }

    if let Ok(virtual_env) = env::var("VIRTUAL_ENV") {
        push_opencv_videoio_ffmpeg_candidates(
            &mut candidates,
            PathBuf::from(virtual_env)
                .join("Lib")
                .join("site-packages")
                .join("cv2"),
        );
    }

    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let python_root = PathBuf::from(local_app_data)
            .join("Programs")
            .join("Python");
        if let Ok(entries) = fs::read_dir(&python_root) {
            for entry in entries.flatten() {
                push_opencv_videoio_ffmpeg_candidates(
                    &mut candidates,
                    entry.path().join("Lib").join("site-packages").join("cv2"),
                );
            }
        }
    }

    candidates.into_iter().find(|path| path.exists())
}

fn push_opencv_videoio_ffmpeg_candidates(candidates: &mut Vec<PathBuf>, base: PathBuf) {
    candidates.push(base.join("opencv_videoio_ffmpeg4100_64.dll"));
    candidates.push(base.join("opencv_videoio_ffmpeg4130_64.dll"));
}

#[cfg(test)]
mod tests {
    use super::state_snapshot_for_save;
    use crate::model::{AppState, ProfileRecord, WindowFocusPreset};

    #[test]
    fn save_snapshot_keeps_crosshair_profiles() {
        let mut state = AppState::default();
        state.profiles.push(ProfileRecord::default());
        state
            .window_focus_presets
            .push(WindowFocusPreset::default());

        let snapshot = state_snapshot_for_save(&state);

        assert_eq!(snapshot.profiles.len(), 1);
        assert!(snapshot.window_focus_presets.is_empty());
        assert!(snapshot.macro_presets.is_empty());
    }
}
