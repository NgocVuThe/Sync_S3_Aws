use crate::*;
use once_cell::sync::Lazy;
use slint::{ModelRc, VecModel};
use std::rc::Rc;
use tracing::error;

use crate::config::BucketConfig;

static BUCKET_NAME_REGEX: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"^[a-z0-9][a-z0-9.-]*[a-z0-9]$").unwrap());

static REGION_NAME_REGEX: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"^[a-z0-9-]+$").unwrap());

pub fn setup_bucket_handlers(ui: &AppWindow) {
    let ui_handle = ui.as_weak();

    let load_bucket_configs = || -> Vec<BucketConfig> {
        let config = crate::config::load_config();
        config.bucket_configs.clone()
    };

    let initial_configs = load_bucket_configs();
    let bucket_items: Vec<BucketConfigItem> = initial_configs
        .iter()
        .map(|b| BucketConfigItem {
            name: b.name.clone().into(),
            distribution_id: b.distribution_id.clone().into(),
        })
        .collect();
    let bucket_names: Vec<slint::SharedString> = initial_configs
        .iter()
        .map(|b| b.name.clone().into())
        .collect();
    ui.set_bucket_list(ModelRc::from(Rc::new(VecModel::from(bucket_items))));
    ui.set_bucket_name_list(ModelRc::from(Rc::new(VecModel::from(bucket_names))));

    let refresh_buckets = {
        let ui_handle = ui_handle.clone();
        move |configs: Vec<BucketConfig>| {
            let bucket_items: Vec<BucketConfigItem> = configs
                .iter()
                .map(|b| BucketConfigItem {
                    name: b.name.clone().into(),
                    distribution_id: b.distribution_id.clone().into(),
                })
                .collect();

            let bucket_names: Vec<slint::SharedString> =
                configs.iter().map(|b| b.name.clone().into()).collect();

            let mut config = crate::config::load_config();
            config.bucket_configs = configs;
            if let Err(e) = crate::config::save_config(&config) {
                error!("Failed to save bucket config: {:?}", e);
            }

            let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                ui.set_bucket_list(ModelRc::from(Rc::new(VecModel::from(bucket_items))));
                ui.set_bucket_name_list(ModelRc::from(Rc::new(VecModel::from(bucket_names))));
            });
        }
    };

    let validate_bucket_name = |name: &str,
                                current_buckets: &[BucketConfig],
                                skip_index: Option<usize>|
     -> Result<(), String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("Bucket name cannot be empty".to_string());
        }
        if trimmed.len() < 3 || trimmed.len() > 63 {
            return Err("Bucket name must be between 3 and 63 characters long".to_string());
        }
        if !BUCKET_NAME_REGEX.is_match(trimmed) {
            return Err("Invalid characters (only a-z, 0-9, . and - allowed)".to_string());
        }
        if trimmed.contains("..") {
            return Err("Bucket name cannot contain consecutive periods".to_string());
        }
        if trimmed.starts_with("xn--") || trimmed.starts_with("sthree-") {
            return Err("Bucket name cannot start with 'xn--' or 'sthree-'".to_string());
        }
        if trimmed.ends_with("-s3alias") || trimmed.ends_with("--ol-s3") {
            return Err("Bucket name cannot end with '-s3alias' or '--ol-s3'".to_string());
        }
        if trimmed.chars().all(|c| c.is_ascii_digit() || c == '.')
            && trimmed.split('.').count() == 4
        {
            return Err("Bucket name cannot be formatted as an IP address".to_string());
        }

        for (i, b) in current_buckets.iter().enumerate() {
            if Some(i) != skip_index && b.name == trimmed {
                return Err("Bucket name already exists".to_string());
            }
        }

        Ok(())
    };

    ui.on_add_bucket({
        let ui_handle = ui_handle.clone();
        let refresh_buckets = refresh_buckets.clone();
        move |name: slint::SharedString, dist_id: slint::SharedString| {
            let Some(ui) = ui_handle.upgrade() else {
                return;
            };
            let name_str = name.to_string();
            let dist_id_str = dist_id.to_string();
            let mut config = crate::config::load_config();

            match validate_bucket_name(&name_str, &config.bucket_configs, None) {
                Ok(_) => {
                    config.bucket_configs.push(BucketConfig {
                        name: name_str.trim().to_string(),
                        distribution_id: dist_id_str.trim().to_string(),
                    });
                    refresh_buckets(config.bucket_configs);
                    ui.set_new_bucket_name("".into());
                    ui.set_new_distribution_id("".into());
                    ui.set_bucket_manager_error("".into());
                    ui.set_show_add_input(false);
                }
                Err(e) => {
                    ui.set_bucket_manager_error(e.into());
                }
            }
        }
    });

    ui.on_update_bucket({
        let ui_handle = ui_handle.clone();
        let refresh_buckets = refresh_buckets.clone();
        move |index: i32, name: slint::SharedString, dist_id: slint::SharedString| {
            let Some(ui) = ui_handle.upgrade() else {
                return;
            };
            let mut config = crate::config::load_config();
            let idx = index as usize;

            if idx >= config.bucket_configs.len() {
                return;
            }

            let name_str = name.to_string();
            let dist_id_str = dist_id.to_string();

            match validate_bucket_name(&name_str, &config.bucket_configs, Some(idx)) {
                Ok(_) => {
                    let old_name = config.bucket_configs[idx].name.clone();
                    let new_name = name_str.trim().to_string();
                    config.bucket_configs[idx].name = new_name.clone();
                    config.bucket_configs[idx].distribution_id = dist_id_str.trim().to_string();

                    if config.selected_bucket == old_name {
                        config.selected_bucket = new_name.clone();
                        if let Err(e) = crate::config::save_config(&config) {
                            error!("Failed to save config after bucket rename: {:?}", e);
                        }
                    }

                    refresh_buckets(config.bucket_configs);
                    ui.set_new_bucket_name("".into());
                    ui.set_new_distribution_id("".into());
                    ui.set_editing_bucket_index(-1);
                    ui.set_bucket_manager_error("".into());
                }
                Err(e) => {
                    ui.set_bucket_manager_error(e.into());
                }
            }
        }
    });

    ui.on_delete_bucket({
        let ui_handle = ui_handle.clone();
        let refresh_buckets = refresh_buckets.clone();
        move |index: i32| {
            let Some(ui) = ui_handle.upgrade() else {
                return;
            };
            let mut config = crate::config::load_config();
            let idx = index as usize;

            if idx < config.bucket_configs.len() {
                let deleted_name = config.bucket_configs.remove(idx).name;

                if config.selected_bucket == deleted_name {
                    config.selected_bucket = String::new();
                    ui.set_bucket_name("".into());
                    if let Err(e) = crate::config::save_config(&config) {
                        error!("Failed to save config after bucket deletion: {:?}", e);
                    }
                }

                refresh_buckets(config.bucket_configs);
                ui.set_bucket_manager_error("".into());
            }
        }
    });
}

pub fn setup_region_handlers(ui: &AppWindow) {
    let ui_handle = ui.as_weak();

    let config = crate::config::load_config();
    let initial_regions: Vec<slint::SharedString> = config
        .regions
        .iter()
        .map(|s| slint::SharedString::from(s.clone()))
        .collect();
    ui.set_region_list(ModelRc::from(Rc::new(VecModel::from(initial_regions))));

    let refresh_regions = {
        let ui_handle = ui_handle.clone();
        move |regions: Vec<String>| {
            let shared_regions: Vec<slint::SharedString> = regions
                .iter()
                .map(|s| slint::SharedString::from(s.clone()))
                .collect();

            let mut config = crate::config::load_config();
            config.regions = regions;
            if let Err(e) = crate::config::save_config(&config) {
                error!("Failed to save config: {:?}", e);
            }

            let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                ui.set_region_list(ModelRc::from(Rc::new(VecModel::from(shared_regions))));
            });
        }
    };

    let validate_region_name =
        |name: &str, current_regions: &[String], skip_index: Option<usize>| -> Result<(), String> {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err("Region name cannot be empty".to_string());
            }

            if !REGION_NAME_REGEX.is_match(trimmed) {
                return Err("Invalid characters (only a-z, 0-9, and - allowed)".to_string());
            }

            for (i, r) in current_regions.iter().enumerate() {
                if Some(i) != skip_index && r == trimmed {
                    return Err("Region already exists".to_string());
                }
            }

            Ok(())
        };

    ui.on_add_region({
        let ui_handle = ui_handle.clone();
        let refresh_regions = refresh_regions.clone();
        move |name| {
            let Some(ui) = ui_handle.upgrade() else {
                return;
            };
            let mut config = crate::config::load_config();

            match validate_region_name(&name, &config.regions, None) {
                Ok(_) => {
                    config.regions.push(name.trim().to_string());
                    refresh_regions(config.regions);
                    ui.set_new_region_name("".into());
                    ui.set_region_manager_error("".into());
                    ui.set_show_add_region_input(false);
                }
                Err(e) => {
                    ui.set_region_manager_error(e.into());
                }
            }
        }
    });

    ui.on_update_region({
        let ui_handle = ui_handle.clone();
        let refresh_regions = refresh_regions.clone();
        move |index, name| {
            let Some(ui) = ui_handle.upgrade() else {
                return;
            };
            let mut config = crate::config::load_config();
            let idx = index as usize;

            if idx >= config.regions.len() {
                return;
            }

            match validate_region_name(&name, &config.regions, Some(idx)) {
                Ok(_) => {
                    let old_name = config.regions[idx].clone();
                    let new_name = name.trim().to_string();
                    config.regions[idx] = new_name.clone();

                    if config.selected_region == old_name {
                        config.selected_region = new_name.clone();
                        ui.set_region(new_name.into());
                        if let Err(e) = crate::config::save_config(&config) {
                            error!("Failed to save config after region rename: {:?}", e);
                        }
                    }

                    refresh_regions(config.regions);
                    ui.set_new_region_name("".into());
                    ui.set_editing_region_index(-1);
                    ui.set_region_manager_error("".into());
                }
                Err(e) => {
                    ui.set_region_manager_error(e.into());
                }
            }
        }
    });

    ui.on_delete_region({
        let ui_handle = ui_handle.clone();
        let refresh_regions = refresh_regions.clone();
        move |index| {
            let Some(ui) = ui_handle.upgrade() else {
                return;
            };
            let mut config = crate::config::load_config();
            let idx = index as usize;

            if idx < config.regions.len() {
                let deleted_name = config.regions.remove(idx);

                if config.selected_region == deleted_name {
                    config.selected_region = String::new();
                    ui.set_region("".into());
                    if let Err(e) = crate::config::save_config(&config) {
                        error!("Failed to save config after region deletion: {:?}", e);
                    }
                }

                refresh_regions(config.regions);
                ui.set_region_manager_error("".into());
            }
        }
    });
}
