// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

mod common;
mod global;
mod local;

use deno_ast::ModuleSpecifier;
use deno_core::anyhow::bail;
use deno_core::error::custom_error;
use deno_core::error::AnyError;
use deno_runtime::deno_node::PathClean;
use deno_runtime::deno_node::RequireNpmResolver;
use global::GlobalNpmPackageResolver;

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::fs_util;

use self::common::InnerNpmPackageResolver;
use self::local::LocalNpmPackageResolver;
use super::NpmCache;
use super::NpmPackageReq;
use super::NpmRegistryApi;

#[derive(Clone)]
pub struct NpmPackageResolver {
  unstable: bool,
  no_npm: bool,
  inner: Arc<dyn InnerNpmPackageResolver>,
}

impl NpmPackageResolver {
  pub fn new(
    cache: NpmCache,
    api: NpmRegistryApi,
    unstable: bool,
    no_npm: bool,
    local_node_modules_path: Option<PathBuf>,
  ) -> Self {
    let inner: Arc<dyn InnerNpmPackageResolver> = match local_node_modules_path
    {
      Some(node_modules_folder) => Arc::new(LocalNpmPackageResolver::new(
        cache,
        api,
        node_modules_folder,
      )),
      None => Arc::new(GlobalNpmPackageResolver::new(cache, api)),
    };
    Self {
      unstable,
      no_npm,
      inner,
    }
  }

  /// Resolves an npm package folder path from a Deno module.
  pub fn resolve_package_folder_from_deno_module(
    &self,
    pkg_req: &NpmPackageReq,
  ) -> Result<PathBuf, AnyError> {
    let path = self
      .inner
      .resolve_package_folder_from_deno_module(pkg_req)?;
    let path = fs_util::canonicalize_path_maybe_not_exists(&path)?;
    log::debug!("Resolved {} to {}", pkg_req, path.display());
    Ok(path)
  }

  /// Resolves an npm package folder path from an npm package referrer.
  pub fn resolve_package_folder_from_package(
    &self,
    name: &str,
    referrer: &ModuleSpecifier,
  ) -> Result<PathBuf, AnyError> {
    let path = self
      .inner
      .resolve_package_folder_from_package(name, referrer)?;
    log::debug!("Resolved {} from {} to {}", name, referrer, path.display());
    Ok(path)
  }

  /// Resolve the root folder of the package the provided specifier is in.
  ///
  /// This will error when the provided specifier is not in an npm package.
  pub fn resolve_package_folder_from_specifier(
    &self,
    specifier: &ModuleSpecifier,
  ) -> Result<PathBuf, AnyError> {
    let path = self
      .inner
      .resolve_package_folder_from_specifier(specifier)?;
    log::debug!("Resolved {} to {}", specifier, path.display());
    Ok(path)
  }

  /// Gets if the provided specifier is in an npm package.
  pub fn in_npm_package(&self, specifier: &ModuleSpecifier) -> bool {
    self
      .resolve_package_folder_from_specifier(specifier)
      .is_ok()
  }

  /// If the resolver has resolved any npm packages.
  pub fn has_packages(&self) -> bool {
    self.inner.has_packages()
  }

  /// Adds a package requirement to the resolver and ensures everything is setup.
  pub async fn add_package_reqs(
    &self,
    packages: Vec<NpmPackageReq>,
  ) -> Result<(), AnyError> {
    assert!(!packages.is_empty());

    if !self.unstable {
      bail!(
        "Unstable use of npm specifiers. The --unstable flag must be provided."
      )
    }

    if self.no_npm {
      let fmt_reqs = packages
        .iter()
        .map(|p| format!("\"{}\"", p))
        .collect::<Vec<_>>()
        .join(", ");
      return Err(custom_error(
        "NoNpm",
        format!(
          "Following npm specifiers were requested: {}; but --no-npm is specified.",
          fmt_reqs
        ),
      ));
    }

    self.inner.add_package_reqs(packages).await
  }
}

impl RequireNpmResolver for NpmPackageResolver {
  fn resolve_package_folder_from_package(
    &self,
    specifier: &str,
    referrer: &std::path::Path,
  ) -> Result<PathBuf, AnyError> {
    let referrer = path_to_specifier(referrer)?;
    self.resolve_package_folder_from_package(specifier, &referrer)
  }

  fn resolve_package_folder_from_path(
    &self,
    path: &Path,
  ) -> Result<PathBuf, AnyError> {
    let specifier = path_to_specifier(path)?;
    self.resolve_package_folder_from_specifier(&specifier)
  }

  fn in_npm_package(&self, path: &Path) -> bool {
    let specifier =
      match ModuleSpecifier::from_file_path(&path.to_path_buf().clean()) {
        Ok(p) => p,
        Err(_) => return false,
      };
    self
      .resolve_package_folder_from_specifier(&specifier)
      .is_ok()
  }

  fn ensure_read_permission(&self, path: &Path) -> Result<(), AnyError> {
    self.inner.ensure_read_permission(path)
  }
}

fn path_to_specifier(path: &Path) -> Result<ModuleSpecifier, AnyError> {
  match ModuleSpecifier::from_file_path(&path.to_path_buf().clean()) {
    Ok(specifier) => Ok(specifier),
    Err(()) => bail!("Could not convert '{}' to url.", path.display()),
  }
}
