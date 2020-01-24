// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use crate::deno_error::DenoError;
use crate::deno_error::ErrorKind;
use crate::deno_error::GetErrorKind;
use crate::disk_cache::DiskCache;
use crate::http_util;
use crate::http_util::create_http_client;
use crate::http_util::FetchOnceResult;
use crate::msg;
use crate::progress::Progress;
use deno_core::ErrBox;
use deno_core::ModuleSpecifier;
use futures::future::Either;
use futures::future::FutureExt;
use reqwest;
use serde_json;
use std;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::result::Result;
use std::str;
use std::sync::Arc;
use std::sync::Mutex;
use url;
use url::Url;

pub fn source_header_cache_failed_error(
  module_name: &str,
  reason: &str,
) -> ErrBox {
  DenoError::new(
    ErrorKind::Other,
    format!(
      "Source code header cache failed for '{}': {}",
      module_name, reason
    ),
  )
  .into()
}

pub fn source_cache_failed_error(module_name: &str, reason: &str) -> ErrBox {
  DenoError::new(
    ErrorKind::Other,
    format!("Source code cache failed for '{}': {}", module_name, reason),
  )
  .into()
}

/// Structure representing local or remote file.
///
/// In case of remote file `url` might be different than originally requested URL, if so
/// `redirect_source_url` will contain original URL and `url` will be equal to final location.
#[derive(Debug, Clone)]
pub struct SourceFile {
  pub url: Url,
  pub filename: PathBuf,
  pub media_type: msg::MediaType,
  pub source_code: Vec<u8>,
}

pub type SourceFileFuture =
  dyn Future<Output = Result<SourceFile, ErrBox>> + Send;

/// Simple struct implementing in-process caching to prevent multiple
/// fs reads/net fetches for same file.
#[derive(Clone, Default)]
pub struct SourceFileCache(Arc<Mutex<HashMap<String, SourceFile>>>);

impl SourceFileCache {
  pub fn set(&self, key: String, source_file: SourceFile) {
    let mut c = self.0.lock().unwrap();
    c.insert(key, source_file);
  }

  pub fn get(&self, key: String) -> Option<SourceFile> {
    let c = self.0.lock().unwrap();
    match c.get(&key) {
      Some(source_file) => Some(source_file.clone()),
      None => None,
    }
  }
}

const SUPPORTED_URL_SCHEMES: [&str; 3] = ["http", "https", "file"];

/// `DenoDir` serves as coordinator for multiple `DiskCache`s containing them
/// in single directory that can be controlled with `$DENO_DIR` env variable.
#[derive(Clone)]
pub struct SourceFileFetcher {
  deps_cache: DiskCache,
  progress: Progress,
  source_file_cache: SourceFileCache,
  cache_blacklist: Vec<String>,
  use_disk_cache: bool,
  no_remote: bool,
  cached_only: bool,
  http_client: reqwest::Client,
}

impl SourceFileFetcher {
  pub fn new(
    deps_cache: DiskCache,
    progress: Progress,
    use_disk_cache: bool,
    cache_blacklist: Vec<String>,
    no_remote: bool,
    cached_only: bool,
  ) -> std::io::Result<Self> {
    let file_fetcher = Self {
      deps_cache,
      progress,
      source_file_cache: SourceFileCache::default(),
      cache_blacklist,
      use_disk_cache,
      no_remote,
      cached_only,
      http_client: create_http_client(),
    };

    Ok(file_fetcher)
  }

  fn check_if_supported_scheme(url: &Url) -> Result<(), ErrBox> {
    if !SUPPORTED_URL_SCHEMES.contains(&url.scheme()) {
      return Err(
        DenoError::new(
          ErrorKind::Other,
          format!("Unsupported scheme \"{}\" for module \"{}\". Supported schemes: {:#?}", url.scheme(), url, SUPPORTED_URL_SCHEMES),
        ).into()
      );
    }

    Ok(())
  }

  /// Required for TS compiler and source maps.
  pub fn fetch_cached_source_file(
    &self,
    specifier: &ModuleSpecifier,
  ) -> Option<SourceFile> {
    let maybe_source_file = self.source_file_cache.get(specifier.to_string());

    if maybe_source_file.is_some() {
      return maybe_source_file;
    }

    // If file is not in memory cache check if it can be found
    // in local cache - which effectively means trying to fetch
    // using "--cached-only" flag. We can safely block on this
    // future, because it doesn't do any asynchronous action
    // it that path.
    let fut = self.get_source_file_async(specifier.as_url(), true, false, true);

    futures::executor::block_on(fut).ok()
  }

  pub fn fetch_source_file_async(
    &self,
    specifier: &ModuleSpecifier,
    maybe_referrer: Option<ModuleSpecifier>,
  ) -> Pin<Box<SourceFileFuture>> {
    let module_url = specifier.as_url().to_owned();
    debug!("fetch_source_file. specifier {} ", &module_url);

    // Check if this file was already fetched and can be retrieved from in-process cache.
    if let Some(source_file) = self.source_file_cache.get(specifier.to_string())
    {
      return Box::pin(async { Ok(source_file) });
    }

    let source_file_cache = self.source_file_cache.clone();
    let specifier_ = specifier.clone();

    let source_file = self.get_source_file_async(
      &module_url,
      self.use_disk_cache,
      self.no_remote,
      self.cached_only,
    );

    Box::pin(async move {
      match source_file.await {
        Ok(mut file) => {
          // TODO: move somewhere?
          if file.source_code.starts_with(b"#!") {
            file.source_code = filter_shebang(file.source_code);
          }

          // Cache in-process for subsequent access.
          source_file_cache.set(specifier_.to_string(), file.clone());

          Ok(file)
        }
        Err(err) => {
          let err_kind = err.kind();
          let referrer_suffix = if let Some(referrer) = maybe_referrer {
            format!(r#" from "{}""#, referrer)
          } else {
            "".to_owned()
          };
          let err = if err_kind == ErrorKind::NotFound {
            let msg = format!(
              r#"Cannot resolve module "{}"{}"#,
              module_url, referrer_suffix
            );
            DenoError::new(ErrorKind::NotFound, msg).into()
          } else if err_kind == ErrorKind::PermissionDenied {
            let msg = format!(
              r#"Cannot find module "{}"{} in cache, --cached-only is specified"#,
              module_url, referrer_suffix
            );
            DenoError::new(ErrorKind::PermissionDenied, msg).into()
          } else {
            err
          };
          Err(err)
        }
      }
    })
  }

  /// This is main method that is responsible for fetching local or remote files.
  ///
  /// If this is a remote module, and it has not yet been cached, the resulting
  /// download will be cached on disk for subsequent access.
  ///
  /// If `use_disk_cache` is true then remote files are fetched from disk cache.
  ///
  /// If `no_remote` is true then this method will fail for remote files.
  ///
  /// If `cached_only` is true then this method will fail for remote files
  /// not already cached.
  fn get_source_file_async(
    &self,
    module_url: &Url,
    use_disk_cache: bool,
    no_remote: bool,
    cached_only: bool,
  ) -> impl Future<Output = Result<SourceFile, ErrBox>> {
    let url_scheme = module_url.scheme();
    let is_local_file = url_scheme == "file";

    if let Err(err) = SourceFileFetcher::check_if_supported_scheme(&module_url)
    {
      return Either::Left(futures::future::err(err));
    }

    // Local files are always fetched from disk bypassing cache entirely.
    if is_local_file {
      match self.fetch_local_file(&module_url) {
        Ok(source_file) => {
          return Either::Left(futures::future::ok(source_file));
        }
        Err(err) => {
          return Either::Left(futures::future::err(err));
        }
      }
    }

    // The file is remote, fail if `no_remote` is true.
    if no_remote {
      return Either::Left(futures::future::err(
        std::io::Error::new(
          std::io::ErrorKind::NotFound,
          format!(
            "Not allowed to get remote file '{}'",
            module_url.to_string()
          ),
        )
        .into(),
      ));
    }

    // Fetch remote file and cache on-disk for subsequent access
    Either::Right(self.fetch_remote_source_async(
      &module_url,
      use_disk_cache,
      cached_only,
      10,
    ))
  }

  /// Fetch local source file.
  fn fetch_local_file(&self, module_url: &Url) -> Result<SourceFile, ErrBox> {
    let filepath = module_url.to_file_path().map_err(|()| {
      ErrBox::from(DenoError::new(
        ErrorKind::InvalidPath,
        "File URL contains invalid path".to_owned(),
      ))
    })?;

    let source_code = match fs::read(filepath.clone()) {
      Ok(c) => c,
      Err(e) => return Err(e.into()),
    };

    let media_type = map_content_type(&filepath, None);
    Ok(SourceFile {
      url: module_url.clone(),
      filename: filepath,
      media_type,
      source_code,
    })
  }

  /// Fetch cached remote file.
  ///
  /// This is a recursive operation if source file has redirections.
  ///
  /// It will keep reading <filename>.headers.json for information about redirection.
  /// `module_initial_source_name` would be None on first call,
  /// and becomes the name of the very first module that initiates the call
  /// in subsequent recursions.
  ///
  /// AKA if redirection occurs, module_initial_source_name is the source path
  /// that user provides, and the final module_name is the resolved path
  /// after following all redirections.
  fn fetch_cached_remote_source(
    &self,
    module_url: &Url,
  ) -> Result<Option<SourceFile>, ErrBox> {
    let source_code_headers = self.get_source_code_headers(&module_url);
    // If source code headers says that it would redirect elsewhere,
    // (meaning that the source file might not exist; only .headers.json is present)
    // Abort reading attempts to the cached source file and and follow the redirect.
    if let Some(redirect_to) = source_code_headers.redirect_to {
      // E.g.
      // module_name https://import-meta.now.sh/redirect.js
      // filename /Users/kun/Library/Caches/deno/deps/https/import-meta.now.sh/redirect.js
      // redirect_to https://import-meta.now.sh/sub/final1.js
      // real_filename /Users/kun/Library/Caches/deno/deps/https/import-meta.now.sh/sub/final1.js
      // real_module_name = https://import-meta.now.sh/sub/final1.js
      let redirect_url = Url::parse(&redirect_to).expect("Should be valid URL");

      // Recurse.
      // TODO(bartlomieju): I'm pretty sure we should call `fetch_remote_source_async` here.
      // Should we expect that all redirects are cached?
      return self.fetch_cached_remote_source(&redirect_url);
    }

    // No redirect needed or end of redirects.
    // We can try read the file
    let filepath = self
      .deps_cache
      .location
      .join(self.deps_cache.get_cache_filename(&module_url));
    let source_code = match fs::read(filepath.clone()) {
      Err(e) => {
        if e.kind() == std::io::ErrorKind::NotFound {
          return Ok(None);
        } else {
          return Err(e.into());
        }
      }
      Ok(c) => c,
    };
    let media_type = map_content_type(
      &filepath,
      source_code_headers.mime_type.as_ref().map(String::as_str),
    );
    Ok(Some(SourceFile {
      url: module_url.clone(),
      filename: filepath,
      media_type,
      source_code,
    }))
  }

  /// Asynchronously fetch remote source file specified by the URL following redirects.
  fn fetch_remote_source_async(
    &self,
    module_url: &Url,
    use_disk_cache: bool,
    cached_only: bool,
    redirect_limit: i64,
  ) -> Pin<Box<SourceFileFuture>> {
    if redirect_limit < 0 {
      let e = DenoError::new(ErrorKind::Http, "too many redirects".to_string());
      return futures::future::err(e.into()).boxed();
    }

    let is_blacklisted =
      check_cache_blacklist(module_url, self.cache_blacklist.as_ref());
    // First try local cache
    if use_disk_cache && !is_blacklisted {
      match self.fetch_cached_remote_source(&module_url) {
        Ok(Some(source_file)) => {
          return futures::future::ok(source_file).boxed();
        }
        Ok(None) => {
          // there's no cached version
        }
        Err(err) => {
          return futures::future::err(err).boxed();
        }
      }
    }

    // If file wasn't found in cache check if we can fetch it
    if cached_only {
      // We can't fetch remote file - bail out
      return futures::future::err(
        std::io::Error::new(
          std::io::ErrorKind::PermissionDenied,
          format!(
            "cannot find remote file '{}' in cache",
            module_url.to_string()
          ),
        )
        .into(),
      )
      .boxed();
    }

    let download_job = self.progress.add("Download", &module_url.to_string());
    let dir = self.clone();
    let module_url = module_url.clone();
    let headers = self.get_source_code_headers(&module_url);
    let module_etag = headers.etag;
    let http_client = self.http_client.clone();
    // Single pass fetch, either yields code or yields redirect.
    let f = async move {
      match http_util::fetch_string_once(http_client, &module_url, module_etag)
        .await?
      {
        FetchOnceResult::NotModified => {
          let source_file =
            dir.fetch_cached_remote_source(&module_url)?.unwrap();

          // Explicit drop to keep reference alive until future completes.
          drop(download_job);

          Ok(source_file)
        }
        FetchOnceResult::Redirect(new_module_url) => {
          // If redirects, update module_name and filename for next looped call.
          if let Err(e) = dir.save_source_code_headers(
            &module_url,
            None,
            Some(new_module_url.to_string()),
            None,
          ) {
            return Err(source_header_cache_failed_error(
              module_url.as_str(),
              &e.to_string(),
            ));
          }

          // Explicit drop to keep reference alive until future completes.
          drop(download_job);

          // Recurse
          dir
            .fetch_remote_source_async(
              &new_module_url,
              use_disk_cache,
              cached_only,
              redirect_limit - 1,
            )
            .await
        }
        FetchOnceResult::Code(source, maybe_content_type, etag) => {
          // We land on the code.
          if let Err(e) = dir.save_source_code_headers(
            &module_url,
            maybe_content_type.clone(),
            None,
            etag,
          ) {
            return Err(source_header_cache_failed_error(
              module_url.as_str(),
              &e.to_string(),
            ));
          }

          if let Err(e) = dir.save_source_code(&module_url, &source) {
            return Err(source_cache_failed_error(
              module_url.as_str(),
              &e.to_string(),
            ));
          }

          let filepath = dir
            .deps_cache
            .location
            .join(dir.deps_cache.get_cache_filename(&module_url));

          let media_type = map_content_type(
            &filepath,
            maybe_content_type.as_ref().map(String::as_str),
          );

          let source_file = SourceFile {
            url: module_url.clone(),
            filename: filepath,
            media_type,
            source_code: source.as_bytes().to_owned(),
          };

          // Explicit drop to keep reference alive until future completes.
          drop(download_job);

          Ok(source_file)
        }
      }
    };

    f.boxed()
  }

  /// Get header metadata associated with a remote file.
  ///
  /// NOTE: chances are that the source file was downloaded due to redirects.
  /// In this case, the headers file provides info about where we should go and get
  /// the file that redirect eventually points to.
  fn get_source_code_headers(&self, url: &Url) -> SourceCodeHeaders {
    let cache_key = self
      .deps_cache
      .get_cache_filename_with_extension(url, "headers.json");

    if let Ok(bytes) = self.deps_cache.get(&cache_key) {
      if let Ok(json_string) = std::str::from_utf8(&bytes) {
        return SourceCodeHeaders::from_json_string(json_string.to_string());
      }
    }

    SourceCodeHeaders::default()
  }

  /// Save contents of downloaded remote file in on-disk cache for subsequent access.
  fn save_source_code(&self, url: &Url, source: &str) -> std::io::Result<()> {
    let cache_key = self.deps_cache.get_cache_filename(url);

    // May not exist. DON'T unwrap.
    let _ = self.deps_cache.remove(&cache_key);

    self.deps_cache.set(&cache_key, source.as_bytes())
  }

  /// Save headers related to source file to {filename}.headers.json file,
  /// only when there is actually something necessary to save.
  ///
  /// For example, if the extension ".js" already mean JS file and we have
  /// content type of "text/javascript", then we would not save the mime type.
  ///
  /// If nothing needs to be saved, the headers file is not created.
  fn save_source_code_headers(
    &self,
    url: &Url,
    mime_type: Option<String>,
    redirect_to: Option<String>,
    etag: Option<String>,
  ) -> std::io::Result<()> {
    let cache_key = self
      .deps_cache
      .get_cache_filename_with_extension(url, "headers.json");

    // Remove possibly existing stale .headers.json file.
    // May not exist. DON'T unwrap.
    let _ = self.deps_cache.remove(&cache_key);

    let headers = SourceCodeHeaders {
      mime_type,
      redirect_to,
      etag,
    };

    let cache_filename = self.deps_cache.get_cache_filename(url);
    if let Ok(maybe_json_string) = headers.to_json_string(&cache_filename) {
      if let Some(json_string) = maybe_json_string {
        return self.deps_cache.set(&cache_key, json_string.as_bytes());
      }
    }

    Ok(())
  }
}

fn map_file_extension(path: &Path) -> msg::MediaType {
  match path.extension() {
    None => msg::MediaType::Unknown,
    Some(os_str) => match os_str.to_str() {
      Some("ts") => msg::MediaType::TypeScript,
      Some("tsx") => msg::MediaType::TSX,
      Some("js") => msg::MediaType::JavaScript,
      Some("jsx") => msg::MediaType::JSX,
      Some("mjs") => msg::MediaType::JavaScript,
      Some("json") => msg::MediaType::Json,
      Some("wasm") => msg::MediaType::Wasm,
      _ => msg::MediaType::Unknown,
    },
  }
}

// convert a ContentType string into a enumerated MediaType
fn map_content_type(path: &Path, content_type: Option<&str>) -> msg::MediaType {
  match content_type {
    Some(content_type) => {
      // sometimes there is additional data after the media type in
      // Content-Type so we have to do a bit of manipulation so we are only
      // dealing with the actual media type
      let ct_vector: Vec<&str> = content_type.split(';').collect();
      let ct: &str = ct_vector.first().unwrap();
      match ct.to_lowercase().as_ref() {
        "application/typescript"
        | "text/typescript"
        | "video/vnd.dlna.mpeg-tts"
        | "video/mp2t"
        | "application/x-typescript" => {
          map_js_like_extension(path, msg::MediaType::TypeScript)
        }
        "application/javascript"
        | "text/javascript"
        | "application/ecmascript"
        | "text/ecmascript"
        | "application/x-javascript" => {
          map_js_like_extension(path, msg::MediaType::JavaScript)
        }
        "application/json" | "text/json" => msg::MediaType::Json,
        // Handle plain and possibly webassembly
        "text/plain" | "application/octet-stream" => map_file_extension(path),
        _ => {
          debug!("unknown content type: {}", content_type);
          msg::MediaType::Unknown
        }
      }
    }
    None => map_file_extension(path),
  }
}

fn map_js_like_extension(
  path: &Path,
  default: msg::MediaType,
) -> msg::MediaType {
  match path.extension() {
    None => default,
    Some(os_str) => match os_str.to_str() {
      None => default,
      Some("jsx") => msg::MediaType::JSX,
      Some("tsx") => msg::MediaType::TSX,
      Some(_) => default,
    },
  }
}

fn filter_shebang(bytes: Vec<u8>) -> Vec<u8> {
  let string = str::from_utf8(&bytes).unwrap();
  if let Some(i) = string.find('\n') {
    let (_, rest) = string.split_at(i);
    rest.as_bytes().to_owned()
  } else {
    Vec::new()
  }
}

fn check_cache_blacklist(url: &Url, black_list: &[String]) -> bool {
  let mut url_without_fragmets = url.clone();
  url_without_fragmets.set_fragment(None);
  if black_list.contains(&String::from(url_without_fragmets.as_str())) {
    return true;
  }
  let mut url_without_query_strings = url_without_fragmets;
  url_without_query_strings.set_query(None);
  let mut path_buf = PathBuf::from(url_without_query_strings.as_str());
  loop {
    if black_list.contains(&String::from(path_buf.to_str().unwrap())) {
      return true;
    }
    if !path_buf.pop() {
      break;
    }
  }
  false
}

#[derive(Debug, Default)]
/// Header metadata associated with a particular "symbolic" source code file.
/// (the associated source code file might not be cached, while remaining
/// a user accessible entity through imports (due to redirects)).
pub struct SourceCodeHeaders {
  /// MIME type of the source code.
  pub mime_type: Option<String>,
  /// Where should we actually look for source code.
  /// This should be an absolute path!
  pub redirect_to: Option<String>,
  /// ETag of the remote source file
  pub etag: Option<String>,
}

static MIME_TYPE: &str = "mime_type";
static REDIRECT_TO: &str = "redirect_to";
static ETAG: &str = "etag";

impl SourceCodeHeaders {
  pub fn from_json_string(headers_string: String) -> Self {
    // TODO: use serde for deserialization
    let maybe_headers_json: serde_json::Result<serde_json::Value> =
      serde_json::from_str(&headers_string);

    if let Ok(headers_json) = maybe_headers_json {
      let mime_type = headers_json[MIME_TYPE].as_str().map(String::from);
      let redirect_to = headers_json[REDIRECT_TO].as_str().map(String::from);
      let etag = headers_json[ETAG].as_str().map(String::from);

      return SourceCodeHeaders {
        mime_type,
        redirect_to,
        etag,
      };
    }

    SourceCodeHeaders::default()
  }

  // TODO: remove this nonsense `cache_filename` param, this should be
  //  done when instantiating SourceCodeHeaders
  pub fn to_json_string(
    &self,
    cache_filename: &Path,
  ) -> Result<Option<String>, serde_json::Error> {
    // TODO(kevinkassimo): consider introduce serde::Deserialize to make things simpler.
    // This is super ugly at this moment...
    // Had trouble to make serde_derive work: I'm unable to build proc-macro2.
    let mut value_map = serde_json::map::Map::new();

    if let Some(mime_type) = &self.mime_type {
      let resolved_mime_type =
        map_content_type(Path::new(""), Some(mime_type.clone().as_str()));

      // TODO: fix this
      let ext_based_mime_type = map_file_extension(cache_filename);

      // Add mime to headers only when content type is different from extension.
      if ext_based_mime_type == msg::MediaType::Unknown
        || resolved_mime_type != ext_based_mime_type
      {
        value_map.insert(MIME_TYPE.to_string(), json!(mime_type));
      }
    }

    if let Some(redirect_to) = &self.redirect_to {
      value_map.insert(REDIRECT_TO.to_string(), json!(redirect_to));
    }

    if let Some(etag) = &self.etag {
      value_map.insert(ETAG.to_string(), json!(etag));
    }

    if value_map.is_empty() {
      return Ok(None);
    }

    serde_json::to_string(&value_map)
      .and_then(|serialized| Ok(Some(serialized)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::fs as deno_fs;
  use crate::tokio_util;
  use tempfile::TempDir;

  fn setup_file_fetcher(dir_path: &Path) -> SourceFileFetcher {
    SourceFileFetcher::new(
      DiskCache::new(&dir_path.to_path_buf().join("deps")),
      Progress::new(),
      true,
      vec![],
      false,
      false,
    )
    .expect("setup fail")
  }

  fn test_setup() -> (TempDir, SourceFileFetcher) {
    let temp_dir = TempDir::new().expect("tempdir fail");
    let fetcher = setup_file_fetcher(temp_dir.path());
    (temp_dir, fetcher)
  }

  macro_rules! file_url {
    ($path:expr) => {
      if cfg!(target_os = "windows") {
        concat!("file:///C:", $path)
      } else {
        concat!("file://", $path)
      }
    };
  }

  #[test]
  fn test_cache_blacklist() {
    let args = crate::flags::resolve_urls(vec![
      String::from("http://deno.land/std"),
      String::from("http://github.com/example/mod.ts"),
      String::from("http://fragment.com/mod.ts#fragment"),
      String::from("http://query.com/mod.ts?foo=bar"),
      String::from("http://queryandfragment.com/mod.ts?foo=bar#fragment"),
    ]);

    let u: Url = "http://deno.land/std/fs/mod.ts".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://github.com/example/file.ts".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), false);

    let u: Url = "http://github.com/example/mod.ts".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://github.com/example/mod.ts?foo=bar".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://github.com/example/mod.ts#fragment".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://fragment.com/mod.ts".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://query.com/mod.ts".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), false);

    let u: Url = "http://fragment.com/mod.ts#fragment".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://query.com/mod.ts?foo=bar".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://queryandfragment.com/mod.ts".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), false);

    let u: Url = "http://queryandfragment.com/mod.ts?foo=bar"
      .parse()
      .unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://queryandfragment.com/mod.ts#fragment"
      .parse()
      .unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), false);

    let u: Url = "http://query.com/mod.ts?foo=bar#fragment".parse().unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);

    let u: Url = "http://fragment.com/mod.ts?foo=bar#fragment"
      .parse()
      .unwrap();
    assert_eq!(check_cache_blacklist(&u, &args), true);
  }

  #[test]
  fn test_source_code_headers_get_and_save() {
    let (_temp_dir, fetcher) = test_setup();
    let url = Url::parse("http://example.com/f.js").unwrap();
    let headers_filepath = fetcher.deps_cache.location.join(
      fetcher
        .deps_cache
        .get_cache_filename_with_extension(&url, "headers.json"),
    );

    if let Some(ref parent) = headers_filepath.parent() {
      fs::create_dir_all(parent).unwrap();
    };

    let _ = deno_fs::write_file(
      headers_filepath.as_path(),
      "{\"mime_type\":\"text/javascript\",\"redirect_to\":\"http://example.com/a.js\"}",
      0o666,
    );
    let headers = fetcher.get_source_code_headers(&url);

    assert_eq!(headers.mime_type.clone().unwrap(), "text/javascript");
    assert_eq!(headers.redirect_to.unwrap(), "http://example.com/a.js");
    assert_eq!(headers.etag, None);

    let _ = fetcher.save_source_code_headers(
      &url,
      Some("text/typescript".to_owned()),
      Some("http://deno.land/a.js".to_owned()),
      Some("W/\"04572f4749af993f4961a7e5daa1e4d5\"".to_owned()),
    );
    let headers2 = fetcher.get_source_code_headers(&url);
    assert_eq!(headers2.mime_type.clone().unwrap(), "text/typescript");
    assert_eq!(headers2.redirect_to.unwrap(), "http://deno.land/a.js");
    assert_eq!(
      headers2.etag.unwrap(),
      "W/\"04572f4749af993f4961a7e5daa1e4d5\""
    );
  }

  #[test]
  fn test_fetch_local_file_no_panic() {
    let (_temp_dir, fetcher) = test_setup();
    if cfg!(windows) {
      // Should fail: missing drive letter.
      let u = Url::parse("file:///etc/passwd").unwrap();
      fetcher.fetch_local_file(&u).unwrap_err();
    } else {
      // Should fail: local network paths are not supported on unix.
      let u = Url::parse("file://server/etc/passwd").unwrap();
      fetcher.fetch_local_file(&u).unwrap_err();
    }
  }

  #[test]
  fn test_get_source_code_1() {
    let http_server_guard = crate::test_util::http_server();
    let (temp_dir, fetcher) = test_setup();
    let fetcher_1 = fetcher.clone();
    let fetcher_2 = fetcher.clone();
    let module_url =
      Url::parse("http://localhost:4545/tests/subdir/mod2.ts").unwrap();
    let module_url_1 = module_url.clone();
    let module_url_2 = module_url.clone();
    let headers_file_name = fetcher.deps_cache.location.join(
      fetcher
        .deps_cache
        .get_cache_filename_with_extension(&module_url, "headers.json"),
    );
    let headers_file_name_1 = headers_file_name.clone();
    let headers_file_name_2 = headers_file_name.clone();
    let headers_file_name_3 = headers_file_name;

    let fut = fetcher
      .get_source_file_async(&module_url, true, false, false)
      .then(move |result| {
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(
          r.source_code,
          "export { printHello } from \"./print_hello.ts\";\n".as_bytes()
        );
        assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
        // Should not create .headers.json file due to matching ext
        assert!(fs::read_to_string(&headers_file_name_1).is_err());

        // Modify .headers.json, write using fs write and read using save_source_code_headers
        let _ = fs::write(
          &headers_file_name_1,
          "{ \"mime_type\": \"text/javascript\" }",
        );
        fetcher_1.get_source_file_async(&module_url, true, false, false)
      })
      .then(move |result2| {
        assert!(result2.is_ok());
        let r2 = result2.unwrap();
        assert_eq!(
          r2.source_code,
          "export { printHello } from \"./print_hello.ts\";\n".as_bytes()
        );
        // If get_source_file_async does not call remote, this should be JavaScript
        // as we modified before! (we do not overwrite .headers.json due to no http fetch)
        assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
        assert_eq!(
          fetcher_2
            .get_source_code_headers(&module_url_1)
            .mime_type
            .unwrap(),
          "text/javascript"
        );

        // Modify .headers.json again, but the other way around
        let _ = fetcher_2.save_source_code_headers(
          &module_url_1,
          Some("application/json".to_owned()),
          None,
          None,
        );
        fetcher_2.get_source_file_async(&module_url_1, true, false, false)
      })
      .then(move |result3| {
        assert!(result3.is_ok());
        let r3 = result3.unwrap();
        assert_eq!(
          r3.source_code,
          "export { printHello } from \"./print_hello.ts\";\n".as_bytes()
        );
        // If get_source_file_async does not call remote, this should be JavaScript
        // as we modified before! (we do not overwrite .headers.json due to no http fetch)
        assert_eq!(&(r3.media_type), &msg::MediaType::Json);
        assert!(fs::read_to_string(&headers_file_name_2)
          .unwrap()
          .contains("application/json"));

        // let's create fresh instance of DenoDir (simulating another freshh Deno process)
        // and don't use cache
        let fetcher = setup_file_fetcher(temp_dir.path());
        fetcher.get_source_file_async(&module_url_2, false, false, false)
      })
      .map(move |result4| {
        assert!(result4.is_ok());
        let r4 = result4.unwrap();
        let expected4 =
          "export { printHello } from \"./print_hello.ts\";\n".as_bytes();
        assert_eq!(r4.source_code, expected4);
        // Now the old .headers.json file should have gone! Resolved back to TypeScript
        assert_eq!(&(r4.media_type), &msg::MediaType::TypeScript);
        assert!(fs::read_to_string(&headers_file_name_3).is_err());
      });

    // http_util::fetch_sync_string requires tokio
    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_code_2() {
    let http_server_guard = crate::test_util::http_server();
    let (temp_dir, fetcher) = test_setup();
    let fetcher_1 = fetcher.clone();
    let module_url =
      Url::parse("http://localhost:4545/tests/subdir/mismatch_ext.ts").unwrap();
    let module_url_1 = module_url.clone();
    let module_url_2 = module_url.clone();
    let headers_file_name = fetcher.deps_cache.location.join(
      fetcher
        .deps_cache
        .get_cache_filename_with_extension(&module_url, "headers.json"),
    );

    let fut = fetcher
      .get_source_file_async(&module_url, true, false, false)
      .then(move |result| {
        assert!(result.is_ok());
        let r = result.unwrap();
        let expected = "export const loaded = true;\n".as_bytes();
        assert_eq!(r.source_code, expected);
        // Mismatch ext with content type, create .headers.json
        assert_eq!(&(r.media_type), &msg::MediaType::JavaScript);
        assert_eq!(
          fetcher
            .get_source_code_headers(&module_url)
            .mime_type
            .unwrap(),
          "text/javascript"
        );

        // Modify .headers.json
        let _ = fetcher.save_source_code_headers(
          &module_url,
          Some("text/typescript".to_owned()),
          None,
          None,
        );
        fetcher.get_source_file_async(&module_url, true, false, false)
      })
      .then(move |result2| {
        assert!(result2.is_ok());
        let r2 = result2.unwrap();
        let expected2 = "export const loaded = true;\n".as_bytes();
        assert_eq!(r2.source_code, expected2);
        // If get_source_file_async does not call remote, this should be TypeScript
        // as we modified before! (we do not overwrite .headers.json due to no http fetch)
        assert_eq!(&(r2.media_type), &msg::MediaType::TypeScript);
        assert!(fs::read_to_string(&headers_file_name).is_err());

        // let's create fresh instance of DenoDir (simulating another freshh Deno process)
        // and don't use cache
        let fetcher = setup_file_fetcher(temp_dir.path());
        fetcher.get_source_file_async(&module_url_1, false, false, false)
      })
      .map(move |result3| {
        assert!(result3.is_ok());
        let r3 = result3.unwrap();
        let expected3 = "export const loaded = true;\n".as_bytes();
        assert_eq!(r3.source_code, expected3);
        // Now the old .headers.json file should be overwritten back to JavaScript!
        // (due to http fetch)
        assert_eq!(&(r3.media_type), &msg::MediaType::JavaScript);
        assert_eq!(
          fetcher_1
            .get_source_code_headers(&module_url_2)
            .mime_type
            .unwrap(),
          "text/javascript"
        );
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_code_multiple_downloads_of_same_file() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let specifier = ModuleSpecifier::resolve_url(
      "http://localhost:4545/tests/subdir/mismatch_ext.ts",
    )
    .unwrap();
    let headers_file_name = fetcher.deps_cache.location.join(
      fetcher
        .deps_cache
        .get_cache_filename_with_extension(specifier.as_url(), "headers.json"),
    );

    // first download
    tokio_util::run(fetcher.fetch_source_file_async(&specifier, None).map(
      |r| {
        assert!(r.is_ok());
      },
    ));

    let result = fs::File::open(&headers_file_name);
    assert!(result.is_ok());
    let headers_file = result.unwrap();
    // save modified timestamp for headers file
    let headers_file_metadata = headers_file.metadata().unwrap();
    let headers_file_modified = headers_file_metadata.modified().unwrap();

    // download file again, it should use already fetched file even though `use_disk_cache` is set to
    // false, this can be verified using source header file creation timestamp (should be
    // the same as after first download)
    tokio_util::run(fetcher.fetch_source_file_async(&specifier, None).map(
      |r| {
        assert!(r.is_ok());
      },
    ));

    let result = fs::File::open(&headers_file_name);
    assert!(result.is_ok());
    let headers_file_2 = result.unwrap();
    // save modified timestamp for headers file
    let headers_file_metadata_2 = headers_file_2.metadata().unwrap();
    let headers_file_modified_2 = headers_file_metadata_2.modified().unwrap();

    assert_eq!(headers_file_modified, headers_file_modified_2);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_code_3() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();

    let redirect_module_url =
      Url::parse("http://localhost:4546/tests/subdir/redirects/redirect1.js")
        .unwrap();
    let redirect_source_filepath = fetcher
      .deps_cache
      .location
      .join("http/localhost_PORT4546/tests/subdir/redirects/redirect1.js");
    let redirect_source_filename =
      redirect_source_filepath.to_str().unwrap().to_string();
    let target_module_url =
      Url::parse("http://localhost:4545/tests/subdir/redirects/redirect1.js")
        .unwrap();
    let redirect_target_filepath = fetcher
      .deps_cache
      .location
      .join("http/localhost_PORT4545/tests/subdir/redirects/redirect1.js");
    let redirect_target_filename =
      redirect_target_filepath.to_str().unwrap().to_string();

    // Test basic follow and headers recording
    let fut = fetcher
      .get_source_file_async(&redirect_module_url, true, false, false)
      .map(move |result| {
        assert!(result.is_ok());
        let mod_meta = result.unwrap();
        // File that requires redirection is not downloaded.
        assert!(fs::read_to_string(&redirect_source_filename).is_err());
        // ... but its .headers.json is created.
        let redirect_source_headers =
          fetcher.get_source_code_headers(&redirect_module_url);
        assert_eq!(
          redirect_source_headers.redirect_to.unwrap(),
          "http://localhost:4545/tests/subdir/redirects/redirect1.js"
        );
        // The target of redirection is downloaded instead.
        assert_eq!(
          fs::read_to_string(&redirect_target_filename).unwrap(),
          "export const redirect = 1;\n"
        );
        let redirect_target_headers =
          fetcher.get_source_code_headers(&target_module_url);
        assert!(redirect_target_headers.redirect_to.is_none());

        // Examine the meta result.
        assert_eq!(mod_meta.url, target_module_url);
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_code_4() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let double_redirect_url =
      Url::parse("http://localhost:4548/tests/subdir/redirects/redirect1.js")
        .unwrap();
    let double_redirect_path = fetcher
      .deps_cache
      .location
      .join("http/localhost_PORT4548/tests/subdir/redirects/redirect1.js");

    let redirect_url =
      Url::parse("http://localhost:4546/tests/subdir/redirects/redirect1.js")
        .unwrap();
    let redirect_path = fetcher
      .deps_cache
      .location
      .join("http/localhost_PORT4546/tests/subdir/redirects/redirect1.js");

    let target_url =
      Url::parse("http://localhost:4545/tests/subdir/redirects/redirect1.js")
        .unwrap();
    let target_path = fetcher
      .deps_cache
      .location
      .join("http/localhost_PORT4545/tests/subdir/redirects/redirect1.js");

    // Test double redirects and headers recording
    let fut = fetcher
      .get_source_file_async(&double_redirect_url, true, false, false)
      .map(move |result| {
        assert!(result.is_ok());
        let mod_meta = result.unwrap();
        assert!(fs::read_to_string(&double_redirect_path).is_err());
        assert!(fs::read_to_string(&redirect_path).is_err());

        let double_redirect_headers =
          fetcher.get_source_code_headers(&double_redirect_url);
        assert_eq!(
          double_redirect_headers.redirect_to.unwrap(),
          redirect_url.to_string()
        );
        let redirect_headers = fetcher.get_source_code_headers(&redirect_url);
        assert_eq!(
          redirect_headers.redirect_to.unwrap(),
          target_url.to_string()
        );

        // The target of redirection is downloaded instead.
        assert_eq!(
          fs::read_to_string(&target_path).unwrap(),
          "export const redirect = 1;\n"
        );
        let redirect_target_headers =
          fetcher.get_source_code_headers(&target_url);
        assert!(redirect_target_headers.redirect_to.is_none());

        // Examine the meta result.
        assert_eq!(mod_meta.url, target_url);
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_code_5() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();

    let double_redirect_url =
      Url::parse("http://localhost:4548/tests/subdir/redirects/redirect1.js")
        .unwrap();

    let redirect_url =
      Url::parse("http://localhost:4546/tests/subdir/redirects/redirect1.js")
        .unwrap();

    let target_path = fetcher
      .deps_cache
      .location
      .join("http/localhost_PORT4545/tests/subdir/redirects/redirect1.js");
    let target_path_ = target_path.clone();

    // Test that redirect target is not downloaded twice for different redirect source.
    let fut = fetcher
      .get_source_file_async(&double_redirect_url, true, false, false)
      .then(move |result| {
        assert!(result.is_ok());
        let result = fs::File::open(&target_path);
        assert!(result.is_ok());
        let file = result.unwrap();
        // save modified timestamp for headers file of redirect target
        let file_metadata = file.metadata().unwrap();
        let file_modified = file_metadata.modified().unwrap();

        // When another file is fetched that also point to redirect target, then redirect target
        // shouldn't be downloaded again. It can be verified using source header file creation
        // timestamp (should be the same as after first `get_source_file`)
        fetcher
          .get_source_file_async(&redirect_url, true, false, false)
          .map(move |r| (r, file_modified))
      })
      .map(move |(result, file_modified)| {
        assert!(result.is_ok());
        let result = fs::File::open(&target_path_);
        assert!(result.is_ok());
        let file_2 = result.unwrap();
        // save modified timestamp for headers file
        let file_metadata_2 = file_2.metadata().unwrap();
        let file_modified_2 = file_metadata_2.modified().unwrap();

        assert_eq!(file_modified, file_modified_2);
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_code_6() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let double_redirect_url =
      Url::parse("http://localhost:4548/tests/subdir/redirects/redirect1.js")
        .unwrap();

    // Test that redirections can be limited
    let fut = fetcher
      .fetch_remote_source_async(&double_redirect_url, false, false, 2)
      .then(move |result| {
        assert!(result.is_ok());
        fetcher.fetch_remote_source_async(&double_redirect_url, false, false, 1)
      })
      .map(move |result| {
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert_eq!(err.kind(), ErrorKind::Http);
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_no_remote() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let module_url =
      Url::parse("http://localhost:4545/tests/002_hello.ts").unwrap();
    // Remote modules are not allowed
    let fut = fetcher
      .get_source_file_async(&module_url, true, true, false)
      .map(move |result| {
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert_eq!(err.kind(), ErrorKind::NotFound);
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_get_source_cached_only() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let fetcher_1 = fetcher.clone();
    let fetcher_2 = fetcher.clone();
    let module_url =
      Url::parse("http://localhost:4545/tests/002_hello.ts").unwrap();
    let module_url_1 = module_url.clone();
    let module_url_2 = module_url.clone();
    // file hasn't been cached before
    let fut = fetcher
      .get_source_file_async(&module_url, true, false, true)
      .then(move |result| {
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);

        // download and cache file
        fetcher_1.get_source_file_async(&module_url_1, true, false, false)
      })
      .then(move |result| {
        assert!(result.is_ok());
        // module is already cached, should be ok even with `cached_only`
        fetcher_2.get_source_file_async(&module_url_2, true, false, true)
      })
      .map(move |result| {
        assert!(result.is_ok());
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_fetch_source_async_1() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let module_url =
      Url::parse("http://127.0.0.1:4545/tests/subdir/mt_video_mp2t.t3.ts")
        .unwrap();
    let headers_file_name = fetcher.deps_cache.location.join(
      fetcher
        .deps_cache
        .get_cache_filename_with_extension(&module_url, "headers.json"),
    );

    let fut = fetcher
      .fetch_remote_source_async(&module_url, false, false, 10)
      .map(move |result| {
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.source_code, b"export const loaded = true;\n");
        assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
        // matching ext, no .headers.json file created
        assert!(fs::read_to_string(&headers_file_name).is_err());
        // Modify .headers.json, make sure read from local
        let _ = fetcher.save_source_code_headers(
          &module_url,
          Some("text/javascript".to_owned()),
          None,
          None,
        );
        let result2 = fetcher.fetch_cached_remote_source(&module_url);
        assert!(result2.is_ok());
        let r2 = result2.unwrap().unwrap();
        assert_eq!(r2.source_code, b"export const loaded = true;\n");
        // Not MediaType::TypeScript due to .headers.json modification
        assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_fetch_source_1() {
    let http_server_guard = crate::test_util::http_server();

    let (_temp_dir, fetcher) = test_setup();
    let module_url =
      Url::parse("http://localhost:4545/tests/subdir/mt_video_mp2t.t3.ts")
        .unwrap();
    let headers_file_name = fetcher.deps_cache.location.join(
      fetcher
        .deps_cache
        .get_cache_filename_with_extension(&module_url, "headers.json"),
    );

    let fut = fetcher
      .fetch_remote_source_async(&module_url, false, false, 10)
      .map(move |result| {
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.source_code, "export const loaded = true;\n".as_bytes());
        assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
        // matching ext, no .headers.json file created
        assert!(fs::read_to_string(&headers_file_name).is_err());

        // Modify .headers.json, make sure read from local
        let _ = fetcher.save_source_code_headers(
          &module_url,
          Some("text/javascript".to_owned()),
          None,
          None,
        );
        let result2 = fetcher.fetch_cached_remote_source(&module_url);
        assert!(result2.is_ok());
        let r2 = result2.unwrap().unwrap();
        assert_eq!(r2.source_code, "export const loaded = true;\n".as_bytes());
        // Not MediaType::TypeScript due to .headers.json modification
        assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_fetch_source_2() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let fetcher_1 = fetcher.clone();
    let fetcher_2 = fetcher.clone();
    let fetcher_3 = fetcher.clone();
    let module_url =
      Url::parse("http://localhost:4545/tests/subdir/no_ext").unwrap();
    let module_url_2 =
      Url::parse("http://localhost:4545/tests/subdir/mismatch_ext.ts").unwrap();
    let module_url_2_ = module_url_2.clone();
    let module_url_3 =
      Url::parse("http://localhost:4545/tests/subdir/unknown_ext.deno")
        .unwrap();
    let module_url_3_ = module_url_3.clone();

    let fut = fetcher
      .fetch_remote_source_async(&module_url, false, false, 10)
      .then(move |result| {
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.source_code, "export const loaded = true;\n".as_bytes());
        assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
        // no ext, should create .headers.json file
        assert_eq!(
          fetcher_1
            .get_source_code_headers(&module_url)
            .mime_type
            .unwrap(),
          "text/typescript"
        );
        fetcher_1.fetch_remote_source_async(&module_url_2, false, false, 10)
      })
      .then(move |result| {
        assert!(result.is_ok());
        let r2 = result.unwrap();
        assert_eq!(r2.source_code, "export const loaded = true;\n".as_bytes());
        assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
        // mismatch ext, should create .headers.json file
        assert_eq!(
          fetcher_2
            .get_source_code_headers(&module_url_2_)
            .mime_type
            .unwrap(),
          "text/javascript"
        );
        // test unknown extension
        fetcher_2.fetch_remote_source_async(&module_url_3, false, false, 10)
      })
      .map(move |result| {
        assert!(result.is_ok());
        let r3 = result.unwrap();
        assert_eq!(r3.source_code, "export const loaded = true;\n".as_bytes());
        assert_eq!(&(r3.media_type), &msg::MediaType::TypeScript);
        // unknown ext, should create .headers.json file
        assert_eq!(
          fetcher_3
            .get_source_code_headers(&module_url_3_)
            .mime_type
            .unwrap(),
          "text/typescript"
        );
      });

    tokio_util::run(fut);
    drop(http_server_guard);
  }

  #[test]
  fn test_fetch_source_file() {
    let (_temp_dir, fetcher) = test_setup();

    // Test failure case.
    let specifier =
      ModuleSpecifier::resolve_url(file_url!("/baddir/hello.ts")).unwrap();
    tokio_util::run(fetcher.fetch_source_file_async(&specifier, None).map(
      |r| {
        assert!(r.is_err());
      },
    ));

    let p =
      std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("js/main.ts");
    let specifier =
      ModuleSpecifier::resolve_url_or_path(p.to_str().unwrap()).unwrap();
    tokio_util::run(fetcher.fetch_source_file_async(&specifier, None).map(
      |r| {
        assert!(r.is_ok());
      },
    ));
  }

  #[test]
  fn test_fetch_source_file_1() {
    /*recompile ts file*/
    let (_temp_dir, fetcher) = test_setup();

    // Test failure case.
    let specifier =
      ModuleSpecifier::resolve_url(file_url!("/baddir/hello.ts")).unwrap();
    tokio_util::run(fetcher.fetch_source_file_async(&specifier, None).map(
      |r| {
        assert!(r.is_err());
      },
    ));

    let p =
      std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("js/main.ts");
    let specifier =
      ModuleSpecifier::resolve_url_or_path(p.to_str().unwrap()).unwrap();
    tokio_util::run(fetcher.fetch_source_file_async(&specifier, None).map(
      |r| {
        assert!(r.is_ok());
      },
    ));
  }

  #[test]
  fn test_resolve_module_3() {
    // unsupported schemes
    let test_cases = [
      "ftp://localhost:4545/testdata/subdir/print_hello.ts",
      "blob:https://whatwg.org/d0360e2f-caee-469f-9a2f-87d5b0456f6f",
    ];

    for &test in test_cases.iter() {
      let url = Url::parse(test).unwrap();
      assert_eq!(
        SourceFileFetcher::check_if_supported_scheme(&url)
          .unwrap_err()
          .kind(),
        ErrorKind::Other
      );
    }
  }

  #[test]
  fn test_map_file_extension() {
    assert_eq!(
      map_file_extension(Path::new("foo/bar.ts")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.tsx")),
      msg::MediaType::TSX
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.d.ts")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.js")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.jsx")),
      msg::MediaType::JSX
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.json")),
      msg::MediaType::Json
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.wasm")),
      msg::MediaType::Wasm
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.txt")),
      msg::MediaType::Unknown
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar")),
      msg::MediaType::Unknown
    );
  }

  #[test]
  fn test_map_content_type_extension_only() {
    // Extension only
    assert_eq!(
      map_content_type(Path::new("foo/bar.ts"), None),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.tsx"), None),
      msg::MediaType::TSX
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.d.ts"), None),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.js"), None),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.txt"), None),
      msg::MediaType::Unknown
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.jsx"), None),
      msg::MediaType::JSX
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.json"), None),
      msg::MediaType::Json
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.wasm"), None),
      msg::MediaType::Wasm
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), None),
      msg::MediaType::Unknown
    );
  }

  #[test]
  fn test_map_content_type_media_type_with_no_extension() {
    // Media Type
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/typescript")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/typescript")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("video/vnd.dlna.mpeg-tts")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("video/mp2t")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/x-typescript")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/javascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/javascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/ecmascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/ecmascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/x-javascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/json")),
      msg::MediaType::Json
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/json")),
      msg::MediaType::Json
    );
  }

  #[test]
  fn test_map_file_extension_media_type_with_extension() {
    assert_eq!(
      map_content_type(Path::new("foo/bar.ts"), Some("text/plain")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.ts"), Some("foo/bar")),
      msg::MediaType::Unknown
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.tsx"),
        Some("application/typescript"),
      ),
      msg::MediaType::TSX
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.tsx"),
        Some("application/javascript"),
      ),
      msg::MediaType::TSX
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.tsx"),
        Some("application/x-typescript"),
      ),
      msg::MediaType::TSX
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.tsx"),
        Some("video/vnd.dlna.mpeg-tts"),
      ),
      msg::MediaType::TSX
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.tsx"), Some("video/mp2t")),
      msg::MediaType::TSX
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.jsx"),
        Some("application/javascript"),
      ),
      msg::MediaType::JSX
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.jsx"),
        Some("application/x-typescript"),
      ),
      msg::MediaType::JSX
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.jsx"),
        Some("application/ecmascript"),
      ),
      msg::MediaType::JSX
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.jsx"), Some("text/ecmascript")),
      msg::MediaType::JSX
    );
    assert_eq!(
      map_content_type(
        Path::new("foo/bar.jsx"),
        Some("application/x-javascript"),
      ),
      msg::MediaType::JSX
    );
  }

  #[test]
  fn test_filter_shebang() {
    assert_eq!(filter_shebang(b"#!"[..].to_owned()), b"");
    assert_eq!(
      filter_shebang("#!\n\n".as_bytes().to_owned()),
      "\n\n".as_bytes()
    );
    let code = "#!/usr/bin/env deno\nconsole.log('hello');\n"
      .as_bytes()
      .to_owned();
    assert_eq!(filter_shebang(code), "\nconsole.log('hello');\n".as_bytes());
  }

  #[test]
  fn test_fetch_with_etag() {
    let http_server_guard = crate::test_util::http_server();
    let (_temp_dir, fetcher) = test_setup();
    let module_url =
      Url::parse("http://127.0.0.1:4545/etag_script.ts").unwrap();

    let fut = async move {
      let source = fetcher
        .fetch_remote_source_async(&module_url, false, false, 1)
        .await;
      assert!(source.is_ok());
      let source = source.unwrap();
      assert_eq!(source.source_code, b"console.log('etag')");
      assert_eq!(&(source.media_type), &msg::MediaType::JavaScript);

      let headers = fetcher.get_source_code_headers(&module_url);
      assert_eq!(headers.etag, Some("33a64df551425fcc55e".to_string()));

      let header_path = fetcher.deps_cache.location.join(
        fetcher
          .deps_cache
          .get_cache_filename_with_extension(&module_url, "headers.json"),
      );

      let modified1 = header_path.metadata().unwrap().modified().unwrap();

      // Forcibly change the contents of the cache file and request
      // it again with the cache parameters turned off.
      // If the fetched content changes, the cached content is used.
      fetcher
        .save_source_code(&module_url, "changed content")
        .unwrap();
      let cached_source = fetcher
        .fetch_remote_source_async(&module_url, false, false, 1)
        .await
        .unwrap();
      assert_eq!(cached_source.source_code, b"changed content");

      let modified2 = header_path.metadata().unwrap().modified().unwrap();

      // Assert that the file has not been modified
      assert_eq!(modified1, modified2);
    };

    tokio_util::run(fut);
    drop(http_server_guard);
  }
}
