// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

//! Code for global npm cache resolution.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use deno_ast::ModuleSpecifier;
use deno_core::error::AnyError;
use deno_core::futures::future::BoxFuture;
use deno_core::futures::FutureExt;
use deno_core::url::Url;
use deno_runtime::deno_node::PackageJson;
use deno_runtime::deno_node::TYPES_CONDITIONS;

use crate::fs_util;
use crate::lockfile::Lockfile;
use crate::npm::resolution::NpmResolution;
use crate::npm::resolution::NpmResolutionSnapshot;
use crate::npm::resolvers::common::cache_packages;
use crate::npm::NpmCache;
use crate::npm::NpmPackageId;
use crate::npm::NpmPackageReq;
use crate::npm::RealNpmRegistryApi;

use super::common::ensure_registry_read_permission;
use super::common::InnerNpmPackageResolver;

/// Resolves packages from the global npm cache.
#[derive(Debug, Clone)]
pub struct GlobalNpmPackageResolver {
  cache: NpmCache,
  resolution: Arc<NpmResolution>,
  registry_url: Url,
}

impl GlobalNpmPackageResolver {
  pub fn new(
    cache: NpmCache,
    api: RealNpmRegistryApi,
    initial_snapshot: Option<NpmResolutionSnapshot>,
  ) -> Self {
    let registry_url = api.base_url().to_owned();
    let resolution = Arc::new(NpmResolution::new(api, initial_snapshot));

    Self {
      cache,
      resolution,
      registry_url,
    }
  }

  fn package_folder(&self, id: &NpmPackageId) -> PathBuf {
    let folder_id = self
      .resolution
      .resolve_package_cache_folder_id_from_id(id)
      .unwrap();
    self
      .cache
      .package_folder_for_id(&folder_id, &self.registry_url)
  }
}

impl InnerNpmPackageResolver for GlobalNpmPackageResolver {
  fn resolve_package_folder_from_deno_module(
    &self,
    pkg_req: &NpmPackageReq,
  ) -> Result<PathBuf, AnyError> {
    let pkg = self.resolution.resolve_package_from_deno_module(pkg_req)?;
    Ok(self.package_folder(&pkg.id))
  }

  fn resolve_package_folder_from_package(
    &self,
    name: &str,
    referrer: &ModuleSpecifier,
    conditions: &[&str],
  ) -> Result<PathBuf, AnyError> {
    let referrer_pkg_id = self
      .cache
      .resolve_package_folder_id_from_specifier(referrer, &self.registry_url)?;
    let pkg_result = self
      .resolution
      .resolve_package_from_package(name, &referrer_pkg_id);
    if conditions == TYPES_CONDITIONS && !name.starts_with("@types/") {
      // When doing types resolution, the package must contain a "types"
      // entry, or else it will then search for a @types package
      if let Ok(pkg) = pkg_result {
        let package_folder = self.package_folder(&pkg.id);
        let package_json = PackageJson::load_skip_read_permission(
          package_folder.join("package.json"),
        )?;
        if package_json.types.is_some() {
          return Ok(package_folder);
        }
      }

      let name = format!("@types/{}", name);
      let pkg = self
        .resolution
        .resolve_package_from_package(&name, &referrer_pkg_id)?;
      Ok(self.package_folder(&pkg.id))
    } else {
      Ok(self.package_folder(&pkg_result?.id))
    }
  }

  fn resolve_package_folder_from_specifier(
    &self,
    specifier: &ModuleSpecifier,
  ) -> Result<PathBuf, AnyError> {
    let pkg_folder_id = self.cache.resolve_package_folder_id_from_specifier(
      specifier,
      &self.registry_url,
    )?;
    Ok(
      self
        .cache
        .package_folder_for_id(&pkg_folder_id, &self.registry_url),
    )
  }

  fn package_size(&self, package_id: &NpmPackageId) -> Result<u64, AnyError> {
    let package_folder = self.package_folder(package_id);
    Ok(fs_util::dir_size(&package_folder)?)
  }

  fn has_packages(&self) -> bool {
    self.resolution.has_packages()
  }

  fn add_package_reqs(
    &self,
    packages: Vec<NpmPackageReq>,
  ) -> BoxFuture<'static, Result<(), AnyError>> {
    let resolver = self.clone();
    async move {
      resolver.resolution.add_package_reqs(packages).await?;
      cache_packages_in_resolver(&resolver).await
    }
    .boxed()
  }

  fn set_package_reqs(
    &self,
    packages: HashSet<NpmPackageReq>,
  ) -> BoxFuture<'static, Result<(), AnyError>> {
    let resolver = self.clone();
    async move {
      resolver.resolution.set_package_reqs(packages).await?;
      cache_packages_in_resolver(&resolver).await
    }
    .boxed()
  }

  fn ensure_read_permission(&self, path: &Path) -> Result<(), AnyError> {
    let registry_path = self.cache.registry_folder(&self.registry_url);
    ensure_registry_read_permission(&registry_path, path)
  }

  fn snapshot(&self) -> NpmResolutionSnapshot {
    self.resolution.snapshot()
  }

  fn lock(&self, lockfile: &mut Lockfile) -> Result<(), AnyError> {
    let snapshot = self.resolution.snapshot();
    self.resolution.lock(lockfile, &snapshot)
  }
}

async fn cache_packages_in_resolver(
  resolver: &GlobalNpmPackageResolver,
) -> Result<(), AnyError> {
  let package_partitions = resolver.resolution.all_packages_partitioned();

  cache_packages(
    package_partitions.packages,
    &resolver.cache,
    &resolver.registry_url,
  )
  .await?;

  // create the copy package folders
  for copy in package_partitions.copy_packages {
    resolver.cache.ensure_copy_package(
      &copy.get_package_cache_folder_id(),
      &resolver.registry_url,
    )?;
  }

  Ok(())
}
