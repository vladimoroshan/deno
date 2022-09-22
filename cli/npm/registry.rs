// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;

use deno_core::anyhow::bail;
use deno_core::anyhow::Context;
use deno_core::error::custom_error;
use deno_core::error::AnyError;
use deno_core::parking_lot::Mutex;
use deno_core::serde::Deserialize;
use deno_core::serde_json;
use deno_core::url::Url;
use deno_runtime::colors;
use deno_runtime::deno_fetch::reqwest;
use serde::Serialize;

use crate::file_fetcher::CacheSetting;
use crate::fs_util;
use crate::http_cache::CACHE_PERM;
use crate::progress_bar::ProgressBar;

use super::cache::NpmCache;
use super::semver::NpmVersionReq;

// npm registry docs: https://github.com/npm/registry/blob/master/docs/REGISTRY-API.md

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NpmPackageInfo {
  pub name: String,
  pub versions: HashMap<String, NpmPackageVersionInfo>,
  #[serde(rename = "dist-tags")]
  pub dist_tags: HashMap<String, String>,
}

pub struct NpmDependencyEntry {
  pub bare_specifier: String,
  pub name: String,
  pub version_req: NpmVersionReq,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct NpmPackageVersionInfo {
  pub version: String,
  pub dist: NpmPackageVersionDistInfo,
  // Bare specifier to version (ex. `"typescript": "^3.0.1") or possibly
  // package and version (ex. `"typescript-3.0.1": "npm:typescript@3.0.1"`).
  #[serde(default)]
  pub dependencies: HashMap<String, String>,
}

impl NpmPackageVersionInfo {
  pub fn dependencies_as_entries(
    &self,
  ) -> Result<Vec<NpmDependencyEntry>, AnyError> {
    fn entry_as_bare_specifier_and_reference(
      entry: (&String, &String),
    ) -> Result<NpmDependencyEntry, AnyError> {
      let bare_specifier = entry.0.clone();
      let (name, version_req) =
        if let Some(package_and_version) = entry.1.strip_prefix("npm:") {
          if let Some((name, version)) = package_and_version.rsplit_once('@') {
            (name.to_string(), version.to_string())
          } else {
            bail!("could not find @ symbol in npm url '{}'", entry.1);
          }
        } else {
          (entry.0.clone(), entry.1.clone())
        };
      let version_req =
        NpmVersionReq::parse(&version_req).with_context(|| {
          format!(
            "error parsing version requirement for dependency: {}@{}",
            bare_specifier, version_req
          )
        })?;
      Ok(NpmDependencyEntry {
        bare_specifier,
        name,
        version_req,
      })
    }

    self
      .dependencies
      .iter()
      .map(entry_as_bare_specifier_and_reference)
      .collect::<Result<Vec<_>, AnyError>>()
  }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct NpmPackageVersionDistInfo {
  /// URL to the tarball.
  pub tarball: String,
  pub shasum: String,
  pub integrity: Option<String>,
}

#[derive(Clone)]
pub struct NpmRegistryApi {
  base_url: Url,
  cache: NpmCache,
  mem_cache: Arc<Mutex<HashMap<String, Option<NpmPackageInfo>>>>,
  cache_setting: CacheSetting,
  progress_bar: ProgressBar,
}

impl NpmRegistryApi {
  pub fn default_url() -> Url {
    let env_var_name = "DENO_NPM_REGISTRY";
    if let Ok(registry_url) = std::env::var(env_var_name) {
      // ensure there is a trailing slash for the directory
      let registry_url = format!("{}/", registry_url.trim_end_matches('/'));
      match Url::parse(&registry_url) {
        Ok(url) => url,
        Err(err) => {
          eprintln!("{}: Invalid {} environment variable. Please provide a valid url.\n\n{:#}",
          colors::red_bold("error"),
          env_var_name, err);
          std::process::exit(1);
        }
      }
    } else {
      Url::parse("https://registry.npmjs.org").unwrap()
    }
  }

  pub fn new(
    base_url: Url,
    cache: NpmCache,
    cache_setting: CacheSetting,
    progress_bar: ProgressBar,
  ) -> Self {
    Self {
      base_url,
      cache,
      mem_cache: Default::default(),
      cache_setting,
      progress_bar,
    }
  }

  pub fn base_url(&self) -> &Url {
    &self.base_url
  }

  pub async fn package_info(
    &self,
    name: &str,
  ) -> Result<NpmPackageInfo, AnyError> {
    let maybe_package_info = self.maybe_package_info(name).await?;
    match maybe_package_info {
      Some(package_info) => Ok(package_info),
      None => bail!("npm package '{}' does not exist", name),
    }
  }

  pub async fn maybe_package_info(
    &self,
    name: &str,
  ) -> Result<Option<NpmPackageInfo>, AnyError> {
    let maybe_info = self.mem_cache.lock().get(name).cloned();
    if let Some(info) = maybe_info {
      Ok(info)
    } else {
      let mut maybe_package_info = None;
      if self.cache_setting.should_use_for_npm_package(name) {
        // attempt to load from the file cache
        maybe_package_info = self.load_file_cached_package_info(name);
      }

      if maybe_package_info.is_none() {
        maybe_package_info = self
          .load_package_info_from_registry(name)
          .await
          .with_context(|| {
          format!("Error getting response at {}", self.get_package_url(name))
        })?;
      }

      // Not worth the complexity to ensure multiple in-flight requests
      // for the same package only request once because with how this is
      // used that should never happen.
      let mut mem_cache = self.mem_cache.lock();
      Ok(match mem_cache.get(name) {
        // another thread raced here, so use its result instead
        Some(info) => info.clone(),
        None => {
          mem_cache.insert(name.to_string(), maybe_package_info.clone());
          maybe_package_info
        }
      })
    }
  }

  fn load_file_cached_package_info(
    &self,
    name: &str,
  ) -> Option<NpmPackageInfo> {
    match self.load_file_cached_package_info_result(name) {
      Ok(value) => value,
      Err(err) => {
        if cfg!(debug_assertions) {
          panic!(
            "error loading cached npm package info for {}: {:#}",
            name, err
          );
        } else {
          None
        }
      }
    }
  }

  fn load_file_cached_package_info_result(
    &self,
    name: &str,
  ) -> Result<Option<NpmPackageInfo>, AnyError> {
    let file_cache_path = self.get_package_file_cache_path(name);
    let file_text = match fs::read_to_string(file_cache_path) {
      Ok(file_text) => file_text,
      Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
      Err(err) => return Err(err.into()),
    };
    match serde_json::from_str(&file_text) {
      Ok(package_info) => Ok(Some(package_info)),
      Err(err) => {
        // This scenario might mean we need to load more data from the
        // npm registry than before. So, just debug log while in debug
        // rather than panic.
        log::debug!(
          "error deserializing registry.json for '{}'. Reloading. {:?}",
          name,
          err
        );
        Ok(None)
      }
    }
  }

  fn save_package_info_to_file_cache(
    &self,
    name: &str,
    package_info: &NpmPackageInfo,
  ) {
    if let Err(err) =
      self.save_package_info_to_file_cache_result(name, package_info)
    {
      if cfg!(debug_assertions) {
        panic!(
          "error saving cached npm package info for {}: {:#}",
          name, err
        );
      }
    }
  }

  fn save_package_info_to_file_cache_result(
    &self,
    name: &str,
    package_info: &NpmPackageInfo,
  ) -> Result<(), AnyError> {
    let file_cache_path = self.get_package_file_cache_path(name);
    let file_text = serde_json::to_string(&package_info)?;
    std::fs::create_dir_all(&file_cache_path.parent().unwrap())?;
    fs_util::atomic_write_file(&file_cache_path, file_text, CACHE_PERM)?;
    Ok(())
  }

  async fn load_package_info_from_registry(
    &self,
    name: &str,
  ) -> Result<Option<NpmPackageInfo>, AnyError> {
    if self.cache_setting == CacheSetting::Only {
      return Err(custom_error(
        "NotCached",
        format!(
          "An npm specifier not found in cache: \"{}\", --cached-only is specified.",
          name
        )
      )
      );
    }

    let package_url = self.get_package_url(name);
    let _guard = self.progress_bar.update(package_url.as_str());

    let response = match reqwest::get(package_url).await {
      Ok(response) => response,
      Err(err) => {
        // attempt to use the local cache
        if let Some(info) = self.load_file_cached_package_info(name) {
          return Ok(Some(info));
        } else {
          return Err(err.into());
        }
      }
    };

    if response.status() == 404 {
      Ok(None)
    } else if !response.status().is_success() {
      let status = response.status();
      let maybe_response_text = response.text().await.ok();
      bail!(
        "Bad response: {:?}{}",
        status,
        match maybe_response_text {
          Some(text) => format!("\n\n{}", text),
          None => String::new(),
        }
      );
    } else {
      let bytes = response.bytes().await?;
      let package_info = serde_json::from_slice(&bytes)?;
      self.save_package_info_to_file_cache(name, &package_info);
      Ok(Some(package_info))
    }
  }

  fn get_package_url(&self, name: &str) -> Url {
    self.base_url.join(name).unwrap()
  }

  fn get_package_file_cache_path(&self, name: &str) -> PathBuf {
    let name_folder_path = self.cache.package_name_folder(name, &self.base_url);
    name_folder_path.join("registry.json")
  }
}
